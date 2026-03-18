//! # Murmur
//!
//! A distributed P2P collaboration library with automatic leader election and CRDT synchronization.
//!
//! ## Architecture
//!
//! - **P2P Networking**: Built on `iroh` for NAT traversal and relay selection
//! - **CRDT Sync**: Uses `automerge` for conflict-free state synchronization
//! - **Local Storage**: SQLite for persistent local replicas
//! - **Leader Election**: Bully algorithm for automatic coordinator selection
//!
//! ## Example
//!
//! ```rust,no_run
//! use murmur::Swarm;
//!
//! #[tokio::main]
//! async fn main() -> murmur::Result<()> {
//!     // Create a new swarm instance
//!     let swarm = Swarm::builder()
//!         .storage_path("./data")
//!         .build()
//!         .await?;
//!
//!     // Start the swarm
//!     swarm.start().await?;
//!
//!     // Put a value
//!     swarm.put("key", b"value").await?;
//!
//!     // Get a value
//!     let value = swarm.get("key").await?;
//!
//!     Ok(())
//! }
//! ```

mod error;
mod storage;
mod storage_trait;
mod election;
mod sync;
mod network;
mod vector_clock;

#[cfg(feature = "file-ops")]
pub mod file;

pub use error::{Error, Result};
pub(crate) use storage_trait::StorageBackend;

#[cfg(feature = "file-ops")]
pub use file::{FileOps, FileMetadata, FileVersion, FileOperation};

use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{info, error, debug, warn};

/// Application-level events emitted by the Swarm.
#[derive(Debug, Clone)]
pub enum SwarmEvent {
    /// A peer connected (incoming or outgoing).
    PeerConnected { node_id: String },
    /// A peer disconnected.
    PeerDisconnected { node_id: String },
    /// CRDT data was synced from a peer (SyncResponse merged or CrdtUpdate applied).
    /// `key` is the specific key updated, or `"*"` for bulk sync.
    DataSynced { key: String, value: Vec<u8> },
    /// A file conflict was detected; the file is now locked until the resolver resolves it.
    ConflictDetected {
        file_name: String,
        resolver_node: String,
        expected_version: u64,
        current_version: u64,
    },
    /// A file conflict was resolved and the file is unlocked.
    ConflictResolved {
        file_name: String,
        resolved_by: String,
        new_version: u64,
    },
}

/// Information about an active file conflict lock.
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub file_name: String,
    /// The node responsible for resolving this conflict.
    pub resolver_node: String,
    pub expected_version: u64,
    pub current_version: u64,
    pub detected_at: u64,
}

/// How to resolve a file conflict.
#[cfg(feature = "file-ops")]
pub enum ConflictResolution {
    /// Keep the local version of the file.
    KeepLocal,
    /// Accept the remote version of the file.
    KeepRemote,
    /// Provide custom merged content.
    MergeWith(Vec<u8>),
}

/// Main entry point for the Murmur library.
///
/// A `Swarm` represents a node in the distributed network.
#[derive(Clone)]
pub struct Swarm {
    inner: Arc<SwarmInner>,
}

struct SwarmInner {
    storage: storage::Storage,
    network: RwLock<network::Network>,
    election: RwLock<election::Election>,
    sync: RwLock<sync::Sync>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    event_tx: tokio::sync::broadcast::Sender<SwarmEvent>,
    /// Per-file conflict locks. Key = file name (without prefix).
    conflict_locks: RwLock<HashMap<String, ConflictInfo>>,
}

impl Swarm {
    /// Create a new builder for configuring a Swarm.
    pub fn builder() -> SwarmBuilder {
        SwarmBuilder::default()
    }

    /// Start the swarm and begin participating in the network.
    pub async fn start(&self) -> Result<()> {
        info!("Starting swarm");

        // Start network accepting connections
        let network = self.inner.network.read().await;
        network.start_accepting().await?;
        drop(network);

        // Start election
        let network = self.inner.network.read().await;
        let mut election = self.inner.election.write().await;
        election.start_election(&*network).await?;
        drop(election);
        drop(network);

        // Spawn background tasks
        self.spawn_heartbeat_task();
        self.spawn_message_handler_task();
        self.spawn_retransmit_task();
        self.spawn_peer_event_forwarder().await;
        self.spawn_auto_discovery_task();

        info!("Swarm started");
        Ok(())
    }

    /// Put a key-value pair into the distributed store.
    pub async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        info!("Put: key={}", key);

        // 1. Update CRDT and get changes
        let mut sync = self.inner.sync.write().await;
        let changes = sync.put(key, value)?;
        drop(sync);

        // 2. Store locally
        self.inner.storage.put(key, value)?;

        // 3. Broadcast changes to all peers with sequence number and vector clock
        let network = self.inner.network.read().await;
        let seq_num = network.next_seq_num().await;
        let vector_clock = network.get_vector_clock().await;

        let message = network::Message::CrdtUpdate {
            key: key.to_string(),
            operation: changes,
            seq_num,
            vector_clock,
        };

        // Track pending ACK for each peer before broadcast
        let peer_ids = network.peers().await;
        network.broadcast(message.clone()).await?;
        for peer_id_str in &peer_ids {
            if let Ok(pid) = peer_id_str.parse::<network::NodeId>() {
                network.track_pending(seq_num, pid, message.clone()).await;
            }
        }
        network.increment_vector_clock().await;

        Ok(())
    }

    /// Get a value by key from the distributed store.
    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // Try local storage first
        let value = self.inner.storage.get(key)?;

        // If not found, try CRDT (might have been synced but not persisted)
        if value.is_none() {
            let sync = self.inner.sync.read().await;
            return sync.get(key);
        }

        Ok(value)
    }

    /// Delete a key from the distributed store.
    pub async fn delete(&self, key: &str) -> Result<()> {
        info!("Delete: key={}", key);

        // 1. Update CRDT and get changes
        let mut sync = self.inner.sync.write().await;
        let changes = sync.delete(key)?;
        drop(sync);

        // 2. Delete locally
        self.inner.storage.delete(key)?;

        // 3. Broadcast changes with sequence number and vector clock
        let network = self.inner.network.read().await;
        let seq_num = network.next_seq_num().await;
        let vector_clock = network.get_vector_clock().await;

        let message = network::Message::CrdtUpdate {
            key: key.to_string(),
            operation: changes,
            seq_num,
            vector_clock,
        };

        let peer_ids = network.peers().await;
        network.broadcast(message.clone()).await?;
        for peer_id_str in &peer_ids {
            if let Ok(pid) = peer_id_str.parse::<network::NodeId>() {
                network.track_pending(seq_num, pid, message.clone()).await;
            }
        }
        network.increment_vector_clock().await;

        Ok(())
    }

    /// Check if this node is currently the leader.
    pub async fn is_leader(&self) -> bool {
        let election = self.inner.election.read().await;
        election.is_leader()
    }

    /// Get the current leader's node ID.
    pub async fn leader_id(&self) -> Option<String> {
        let election = self.inner.election.read().await;
        election.leader_id()
    }

    /// Get this node's ID.
    pub async fn node_id(&self) -> String {
        let network = self.inner.network.read().await;
        network.node_id_string()
    }

    /// Get this node's address as JSON string (for sharing with other peers).
    ///
    /// The returned string can be passed to [`Swarm::connect_peer`] on another node.
    pub async fn node_addr(&self) -> Result<String> {
        let network = self.inner.network.read().await;
        let addr = network.node_addr().await?;
        serde_json::to_string(&addr)
            .map_err(|e| Error::Serialization(format!("Failed to serialize NodeAddr: {}", e)))
    }

    /// Connect to another peer by their node address.
    ///
    /// Accepts a JSON-serialized `NodeAddr`, e.g.:
    /// ```json
    /// {"node_id":"<base32-public-key>","relay_url":"https://...","direct_addresses":["ip:port"]}
    /// ```
    ///
    /// You can obtain this string from [`Swarm::node_addr`] on the remote peer.
    pub async fn connect_peer(&self, peer_addr_str: &str) -> Result<()> {
        let node_addr: iroh_net::NodeAddr = serde_json::from_str(peer_addr_str)
            .map_err(|e| Error::Network(format!("Failed to parse NodeAddr JSON: {}", e)))?;

        let peer_id = node_addr.node_id;

        let my_id = self.node_id().await;
        if peer_id.to_string() == my_id {
            debug!("Skipping connection to self");
            return Ok(());
        }

        let network = self.inner.network.read().await;
        network.connect(node_addr).await?;

        // Request full sync from the newly connected peer
        if let Err(e) = network.send(&peer_id.to_string(), network::Message::SyncRequest).await {
            warn!("Failed to send SyncRequest after connect_peer: {}", e);
        }

        info!("Connected to peer via address: {}", peer_id);
        Ok(())
    }

    /// Connect to a discovered peer by node ID
    ///
    /// # Arguments
    /// - `node_id_str`: The node ID string from iroh discovery
    ///
    /// # Returns
    /// - `Ok(())` if connection successful or already connected
    /// - `Err` if connection failed
    pub async fn connect_peer_by_id(&self, node_id_str: &str) -> Result<()> {
        // Skip connecting to ourselves
        let my_node_id = self.node_id().await;
        if node_id_str == my_node_id {
            debug!("Skipping connection to self");
            return Ok(());
        }

        // Parse NodeAddr from the node_id string
        use iroh_net::key::PublicKey;
        use std::str::FromStr;

        let node_id = PublicKey::from_str(node_id_str)
            .map_err(|e| Error::Network(format!("Invalid node ID: {}", e)))?;

        // Get the network and try to connect
        let network = self.inner.network.read().await;

        // Create a minimal NodeAddr with just the node_id
        // iroh will use relay servers and local discovery to establish connection
        let node_addr = iroh_net::NodeAddr::new(node_id);

        network.connect(node_addr).await?;

        if let Err(e) = network.send(node_id_str, network::Message::SyncRequest).await {
            warn!("Failed to send SyncRequest after connect_peer_by_id: {}", e);
        }

        info!("Connected to peer: {}", node_id_str);
        Ok(())
    }

    /// Get list of connected peers.
    pub async fn connected_peers(&self) -> Vec<String> {
        let network = self.inner.network.read().await;
        network.peers().await
    }

    /// Discover and connect to peers found by iroh's LocalSwarmDiscovery.
    ///
    /// After connecting to new peers, automatically sends a SyncRequest
    /// so CRDT state is exchanged.
    ///
    /// Returns the number of new connections established.
    pub async fn discover_and_connect_local_peers(&self) -> Result<usize> {
        let peers_before: std::collections::HashSet<String> = {
            let network = self.inner.network.read().await;
            network.peers().await.into_iter().collect()
        };

        let count = {
            let network = self.inner.network.read().await;
            network.discover_and_connect_peers().await?
        };

        if count > 0 {
            let peers_after = {
                let network = self.inner.network.read().await;
                network.peers().await
            };

            for peer_id in &peers_after {
                if !peers_before.contains(peer_id) {
                    debug!("Requesting full sync from new peer: {}", peer_id);
                    let network = self.inner.network.read().await;
                    if let Err(e) = network.send(peer_id, network::Message::SyncRequest).await {
                        warn!("Failed to send SyncRequest to {}: {}", peer_id, e);
                    }
                }
            }
        }

        Ok(count)
    }

    /// Announce this node's presence in the group.
    ///
    /// Writes metadata (group_id, nickname) to a well-known key so other
    /// peers can discover who is in the group after syncing.
    pub async fn announce(&self, nickname: &str) -> Result<()> {
        let node_id = self.node_id().await;
        let group_id = {
            let network = self.inner.network.read().await;
            network.group_id().to_string()
        };

        let meta = serde_json::json!({
            "node_id": node_id,
            "nickname": nickname,
            "group_id": group_id,
            "ts": chrono::Utc::now().timestamp(),
        });

        let key = format!("_meta:{}", node_id);
        let value = serde_json::to_vec(&meta)
            .map_err(|e| Error::Serialization(format!("Failed to serialize metadata: {}", e)))?;

        self.put(&key, &value).await?;
        info!("Announced presence: group={}, nickname={}", group_id, nickname);
        Ok(())
    }

    /// List all announced peers (from CRDT metadata keys).
    ///
    /// Returns a vec of (node_id, nickname, group_id) tuples for all peers
    /// that have called [`announce`].
    pub async fn list_announced_peers(&self) -> Result<Vec<(String, String, String)>> {
        let sync = self.inner.sync.read().await;
        let keys = sync.keys();
        drop(sync);

        let mut peers = Vec::new();
        for key in keys {
            if let Some(node_id_suffix) = key.strip_prefix("_meta:") {
                if let Some(value) = self.get(&key).await? {
                    if let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&value) {
                        let nickname = meta.get("nickname")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let group_id = meta.get("group_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        peers.push((node_id_suffix.to_string(), nickname, group_id));
                    }
                }
            }
        }

        Ok(peers)
    }

    /// Get the group_id this swarm was created with.
    pub async fn group_id(&self) -> String {
        let network = self.inner.network.read().await;
        network.group_id().to_string()
    }

    /// Check whether a file is currently conflict-locked.
    pub async fn is_file_locked(&self, file_name: &str) -> bool {
        self.inner.conflict_locks.read().await.contains_key(file_name)
    }

    /// Get conflict info for a locked file (if any).
    pub async fn get_conflict_info(&self, file_name: &str) -> Option<ConflictInfo> {
        self.inner.conflict_locks.read().await.get(file_name).cloned()
    }

    /// List all currently conflict-locked files.
    pub async fn locked_files(&self) -> Vec<ConflictInfo> {
        self.inner.conflict_locks.read().await.values().cloned().collect()
    }

    /// Lock a file due to conflict and broadcast the lock to all peers.
    ///
    /// Typically called automatically by [`FileOps::put_file`] when a version
    /// mismatch is detected. Applications may also call this directly to
    /// enforce a conflict lock from external logic.
    pub async fn lock_file_conflict(
        &self,
        file_name: &str,
        resolver_node: &str,
        expected_version: u64,
        current_version: u64,
    ) -> Result<()> {
        let info = ConflictInfo {
            file_name: file_name.to_string(),
            resolver_node: resolver_node.to_string(),
            expected_version,
            current_version,
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.inner.conflict_locks.write().await
            .insert(file_name.to_string(), info);

        self.emit_event(SwarmEvent::ConflictDetected {
            file_name: file_name.to_string(),
            resolver_node: resolver_node.to_string(),
            expected_version,
            current_version,
        });

        let network = self.inner.network.read().await;
        let msg = network::Message::ConflictLock {
            file_name: file_name.to_string(),
            resolver_node: resolver_node.to_string(),
            expected_version,
            current_version,
        };
        if let Err(e) = network.broadcast(msg).await {
            warn!("Failed to broadcast ConflictLock: {}", e);
        }

        info!(
            "File conflict locked: {} (resolver={})",
            file_name, resolver_node
        );
        Ok(())
    }

    /// Resolve a file conflict. Only the resolver node should call this.
    ///
    /// `resolved_content` is the final file content to write.
    /// After resolution, the lock is removed and all peers are notified.
    #[cfg(feature = "file-ops")]
    pub async fn resolve_conflict(
        &self,
        file_name: &str,
        resolution: ConflictResolution,
    ) -> Result<()> {
        let my_id = self.node_id().await;

        {
            let locks = self.inner.conflict_locks.read().await;
            let info = locks.get(file_name).ok_or_else(|| {
                Error::Other(format!("File '{}' is not conflict-locked", file_name))
            })?;
            if info.resolver_node != my_id {
                return Err(Error::Other(format!(
                    "Only the resolver node ({}) can resolve this conflict",
                    info.resolver_node
                )));
            }
        }

        let content_key = format!("file:data:{}", file_name);
        let meta_key = format!("file:meta:{}", file_name);

        let resolved_content = match resolution {
            ConflictResolution::KeepLocal => {
                self.inner.storage.get(&content_key)?
                    .ok_or_else(|| Error::Other("Local file content not found".into()))?
            }
            ConflictResolution::KeepRemote => {
                let sync = self.inner.sync.read().await;
                sync.get(&content_key)?
                    .ok_or_else(|| Error::Sync("Remote file content not found in CRDT".into()))?
            }
            ConflictResolution::MergeWith(content) => content,
        };

        let current_version = {
            if let Some(meta_bytes) = self.get(&meta_key).await? {
                serde_json::from_slice::<file::FileMetadata>(&meta_bytes)
                    .map(|m| m.version)
                    .unwrap_or(0)
            } else {
                0
            }
        };
        let new_version = current_version + 1;

        let metadata = file::FileMetadata {
            name: file_name.to_string(),
            size: resolved_content.len(),
            modified: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            checksum: format!("{}", resolved_content.len()),
            version: new_version,
            author: my_id.clone(),
        };

        let versioned_key = format!("file:data:{}:v{}", file_name, new_version);
        self.put(&versioned_key, &resolved_content).await?;
        self.put(&content_key, &resolved_content).await?;

        let meta_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&meta_key, &meta_bytes).await?;

        let version_entry = file::FileVersion {
            version: new_version,
            content_key: versioned_key,
            timestamp: metadata.modified,
            author: my_id.clone(),
            size: resolved_content.len(),
            operation: file::FileOperation::Update,
        };
        let history_key = format!("file:history:{}", file_name);
        let mut history: Vec<file::FileVersion> = self.get(&history_key).await?
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        history.push(version_entry);
        let history_bytes = serde_json::to_vec(&history)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&history_key, &history_bytes).await?;

        self.inner.conflict_locks.write().await.remove(file_name);

        self.emit_event(SwarmEvent::ConflictResolved {
            file_name: file_name.to_string(),
            resolved_by: my_id.clone(),
            new_version,
        });

        let network = self.inner.network.read().await;
        let msg = network::Message::ConflictUnlock {
            file_name: file_name.to_string(),
            resolved_by: my_id,
            new_version,
        };
        if let Err(e) = network.broadcast(msg).await {
            warn!("Failed to broadcast ConflictUnlock: {}", e);
        }

        info!("File conflict resolved: {} → v{}", file_name, new_version);
        Ok(())
    }

    /// Shutdown the swarm gracefully.
    /// Subscribe to swarm events (PeerConnected, PeerDisconnected, DataSynced).
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SwarmEvent> {
        self.inner.event_tx.subscribe()
    }

    fn emit_event(&self, event: SwarmEvent) {
        let _ = self.inner.event_tx.send(event);
    }

    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down swarm");
        let _ = self.inner.shutdown_tx.send(());
        Ok(())
    }

    /// Spawn background task for sending heartbeats (if leader).
    fn spawn_heartbeat_task(&self) {
        let swarm = self.clone();
        tokio::spawn(async move {
            let mut shutdown_rx = swarm.inner.shutdown_tx.subscribe();
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                        let election = swarm.inner.election.read().await;
                        let network = swarm.inner.network.read().await;

                        if let Err(e) = election.send_heartbeat_if_leader(&*network).await {
                            error!("Failed to send heartbeat: {}", e);
                        }
                    }
                }
            }
        });
    }

    /// Spawn background task for handling incoming messages.
    fn spawn_message_handler_task(&self) {
        let swarm = self.clone();
        tokio::spawn(async move {
            let mut shutdown_rx = swarm.inner.shutdown_tx.subscribe();
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    msg = async {
                        let network = swarm.inner.network.read().await;
                        network.recv().await
                    } => {
                        if let Some((peer_id, message)) = msg {
                            if let Err(e) = swarm.handle_message(peer_id, message).await {
                                error!("Failed to handle message: {}", e);
                            }
                        }
                    }
                }
            }
        });
    }

    /// Forward network-layer PeerEvents into SwarmEvents,
    /// and send SyncRequest to newly connected peers.
    async fn spawn_peer_event_forwarder(&self) {
        let inner = self.inner.clone();
        let network = self.inner.network.read().await;
        let mut peer_rx = network.subscribe_peer_events();
        drop(network);

        let event_tx = self.inner.event_tx.clone();
        let mut shutdown_rx = self.inner.shutdown_tx.subscribe();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    result = peer_rx.recv() => {
                        match result {
                            Ok(network::PeerEvent::Connected(id)) => {
                                let _ = event_tx.send(SwarmEvent::PeerConnected { node_id: id.clone() });
                                // Send SyncRequest to the newly connected peer
                                let network = inner.network.read().await;
                                if let Err(e) = network.send(&id, network::Message::SyncRequest).await {
                                    warn!("Failed to send SyncRequest to new peer {}: {}", id, e);
                                } else {
                                    info!("Sent SyncRequest to new peer {}", &id[..8.min(id.len())]);
                                }
                            }
                            Ok(network::PeerEvent::Disconnected(id)) => {
                                let _ = event_tx.send(SwarmEvent::PeerDisconnected { node_id: id });
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });
    }

    /// Spawn background task for retransmitting unacknowledged messages.
    fn spawn_retransmit_task(&self) {
        let swarm = self.clone();
        tokio::spawn(async move {
            let mut shutdown_rx = swarm.inner.shutdown_tx.subscribe();
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                        let network = swarm.inner.network.read().await;
                        let (resent, failed) = network.retransmit_timed_out().await;
                        if resent > 0 {
                            debug!("Retransmitted {} messages", resent);
                        }
                        if !failed.is_empty() {
                            warn!("Dropped {} messages after max retransmit attempts", failed.len());
                        }
                    }
                }
            }
        });
    }

    /// Periodically discover and connect to new local peers via mDNS.
    fn spawn_auto_discovery_task(&self) {
        let swarm = self.clone();
        tokio::spawn(async move {
            let mut shutdown_rx = swarm.inner.shutdown_tx.subscribe();
            // Initial short delay to let mDNS register
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            loop {
                {
                    let network = swarm.inner.network.read().await;
                    match network.discover_and_connect_peers().await {
                        Ok(n) if n > 0 => {
                            info!("Auto-discovery: connected to {} new peer(s)", n);
                        }
                        Err(e) => {
                            debug!("Auto-discovery error: {}", e);
                        }
                        _ => {}
                    }
                }

                tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(3)) => {}
                }
            }
        });
    }

    /// Handle an incoming message from a peer.
    async fn handle_message(&self, peer_id: network::NodeId, message: network::Message) -> Result<()> {
        match message {
            network::Message::CrdtUpdate { key, operation, seq_num, vector_clock } => {
                debug!("Received CRDT update: key={}, seq={}", key, seq_num);

                // Merge vector clocks
                let network = self.inner.network.read().await;
                network.merge_vector_clock(&vector_clock).await;
                drop(network);

                // Apply CRDT changes and read back value while holding write lock
                let mut sync = self.inner.sync.write().await;
                sync.apply_changes(&operation)?;

                // Read the synced value immediately
                let synced_value = sync.get(&key)?.unwrap_or_default();
                debug!("DataSynced: key={} value_len={} value={:?}",
                    key, synced_value.len(), String::from_utf8_lossy(&synced_value));

                // Detect file-level conflicts via Automerge multi-value check
                #[cfg(feature = "file-ops")]
                let file_conflict = if key.starts_with("file:meta:") {
                    sync.has_conflicts(&key)
                } else {
                    false
                };
                #[cfg(not(feature = "file-ops"))]
                let file_conflict = false;

                drop(sync);

                // Update local storage
                self.inner.storage.put(&key, &synced_value)?;

                // If a file:meta conflict was detected, lock the file
                #[cfg(feature = "file-ops")]
                if file_conflict {
                    let file_name = key.strip_prefix("file:meta:").unwrap();
                    let already_locked = self.is_file_locked(file_name).await;
                    if !already_locked {
                        let my_id = self.node_id().await;
                        let remote_id = peer_id.to_string();
                        let resolver = std::cmp::min(my_id.clone(), remote_id.clone());
                        warn!(
                            "CRDT conflict detected on file:meta:{} between {} and {}",
                            file_name, my_id, remote_id
                        );
                        let _ = self.lock_file_conflict(file_name, &resolver, 0, 0).await;
                    }
                }

                // Send ACK
                let network = self.inner.network.read().await;
                let ack_msg = network::Message::Ack { seq_num };
                if let Err(e) = network.send(&peer_id.to_string(), ack_msg).await {
                    warn!("Failed to send ACK: {}", e);
                }
                self.emit_event(SwarmEvent::DataSynced { key, value: synced_value });
            }

            network::Message::Ack { seq_num } => {
                debug!("Received ACK for seq={}", seq_num);
                let network = self.inner.network.read().await;
                network.ack_received(seq_num).await;
            }

            network::Message::SyncRequest => {
                info!("SyncRequest from peer {}", peer_id);
                let mut sync = self.inner.sync.write().await;
                let all_changes = sync.get_all_changes()?;
                drop(sync);
                info!("SyncRequest: sending SyncResponse ({} bytes)", all_changes.len());

                let network = self.inner.network.read().await;
                network.send(
                    &peer_id.to_string(),
                    network::Message::SyncResponse { data: all_changes },
                ).await?;
            }

            network::Message::SyncResponse { data } => {
                let mut sync = self.inner.sync.write().await;
                match sync.load_document(&data) {
                    Ok(()) => {
                        info!("SyncResponse: loaded document, scanning keys...");
                        // Populate local storage from all CRDT keys and emit per-key events
                        let keys = sync.keys();
                        info!("SyncResponse: found {} keys", keys.len());
                        let mut synced_pairs = Vec::new();
                        for k in &keys {
                            if let Some(value) = sync.get(k)? {
                                self.inner.storage.put(k, &value)?;
                                synced_pairs.push((k.clone(), value));
                            }
                        }
                        drop(sync);

                        // Emit per-key DataSynced events so consumers get individual updates
                        for (k, v) in &synced_pairs {
                            info!("SyncResponse: emitting DataSynced key={} value_len={}", k, v.len());
                            self.emit_event(SwarmEvent::DataSynced { key: k.clone(), value: v.clone() });
                        }
                        info!("SyncResponse: done, emitted {} events", synced_pairs.len());
                    }
                    Err(e) => {
                        warn!("SyncResponse: load_document failed: {}", e);
                        drop(sync);
                        // Still emit a bulk sync event so consumers know sync was attempted
                        self.emit_event(SwarmEvent::DataSynced { key: "*".into(), value: vec![] });
                    }
                }
            }

            network::Message::ConflictLock {
                file_name,
                resolver_node,
                expected_version,
                current_version,
            } => {
                info!(
                    "Received ConflictLock for '{}' (resolver={})",
                    file_name, resolver_node
                );
                let mut locks = self.inner.conflict_locks.write().await;
                locks.entry(file_name.clone()).or_insert_with(|| ConflictInfo {
                    file_name: file_name.clone(),
                    resolver_node: resolver_node.clone(),
                    expected_version,
                    current_version,
                    detected_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                });
                drop(locks);

                self.emit_event(SwarmEvent::ConflictDetected {
                    file_name,
                    resolver_node,
                    expected_version,
                    current_version,
                });
            }

            network::Message::ConflictUnlock {
                file_name,
                resolved_by,
                new_version,
            } => {
                info!(
                    "Received ConflictUnlock for '{}' (resolved by {} → v{})",
                    file_name, resolved_by, new_version
                );
                self.inner.conflict_locks.write().await.remove(&file_name);

                self.emit_event(SwarmEvent::ConflictResolved {
                    file_name,
                    resolved_by,
                    new_version,
                });
            }

            // Election messages
            _ => {
                let network = self.inner.network.read().await;
                let mut election = self.inner.election.write().await;
                election.handle_message(&peer_id.to_string(), &message, &*network).await?;
            }
        }

        Ok(())
    }
}

/// Builder for configuring a Swarm instance.
#[derive(Default)]
pub struct SwarmBuilder {
    storage_path: Option<PathBuf>,
    group_id: Option<String>,
}

impl SwarmBuilder {
    /// Set the storage path for local data persistence.
    pub fn storage_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.storage_path = Some(path.into());
        self
    }

    /// Set the group ID for isolating different swarms.
    /// Nodes with different group IDs won't communicate.
    pub fn group_id<S: Into<String>>(mut self, id: S) -> Self {
        self.group_id = Some(id.into());
        self
    }

    /// Build the Swarm instance.
    pub async fn build(self) -> Result<Swarm> {
        let storage_path = self.storage_path
            .unwrap_or_else(|| PathBuf::from("./murmur_data"));

        let group_id = self.group_id
            .unwrap_or_else(|| "default".to_string());

        // Initialize components
        let storage = storage::Storage::new(&storage_path)?;
        let network = network::Network::new(group_id).await?;
        let node_id = network.node_id_string();
        let election = election::Election::new(node_id);
        let sync = sync::Sync::new();

        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        let (event_tx, _) = tokio::sync::broadcast::channel(64);

        Ok(Swarm {
            inner: Arc::new(SwarmInner {
                storage,
                network: RwLock::new(network),
                election: RwLock::new(election),
                sync: RwLock::new(sync),
                shutdown_tx,
                event_tx,
                conflict_locks: RwLock::new(HashMap::new()),
            }),
        })
    }
}
