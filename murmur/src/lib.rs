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
//! async fn main() -> anyhow::Result<()> {
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

pub use error::{Error, Result};
pub use storage_trait::StorageBackend;

use std::path::PathBuf;
use tokio::sync::RwLock;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{info, error, debug, warn};

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
        network.broadcast(message).await?;
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
        network.broadcast(message).await?;
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

    /// Get this node's address (for sharing with other peers).
    pub async fn node_addr(&self) -> String {
        let network = self.inner.network.read().await;
        format!("{:?}", network.node_addr().await)
    }

    /// Connect to another peer by their node address.
    pub async fn connect_peer(&self, peer_addr_str: &str) -> Result<()> {
        use iroh::net::NodeAddr;

        // Parse the node address string
        // Format: NodeAddr { node_id: <id>, relay_url: <url>, direct_addresses: [...] }
        // For simplicity, we'll need a better parsing method or use a different format

        // Temporary: just log the error
        return Err(Error::Network(
            "connect_peer not fully implemented - need proper NodeAddr parsing".to_string()
        ));
    }

    /// Get list of connected peers.
    pub async fn connected_peers(&self) -> Vec<String> {
        let network = self.inner.network.read().await;
        network.peers().await
    }

    /// Shutdown the swarm gracefully.
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

    /// Handle an incoming message from a peer.
    async fn handle_message(&self, peer_id: network::NodeId, message: network::Message) -> Result<()> {
        match message {
            network::Message::CrdtUpdate { key, operation, seq_num, vector_clock } => {
                debug!("Received CRDT update: key={}, seq={}", key, seq_num);

                // Merge vector clocks
                let network = self.inner.network.read().await;
                network.merge_vector_clock(&vector_clock).await;
                drop(network);

                // Apply CRDT changes
                let mut sync = self.inner.sync.write().await;
                sync.apply_changes(&operation)?;
                drop(sync);

                // Update local storage
                let sync = self.inner.sync.read().await;
                if let Some(value) = sync.get(&key)? {
                    self.inner.storage.put(&key, &value)?;
                }

                // Send ACK
                let network = self.inner.network.read().await;
                let ack_msg = network::Message::Ack { seq_num };
                if let Err(e) = network.send(&peer_id.to_string(), ack_msg).await {
                    warn!("Failed to send ACK: {}", e);
                }
            }

            network::Message::Ack { seq_num } => {
                debug!("Received ACK for seq={}", seq_num);
                // TODO: Remove from pending retransmission queue
            }

            network::Message::SyncRequest => {
                // Send full state to requesting peer
                let mut sync = self.inner.sync.write().await;
                let all_changes = sync.get_all_changes()?;
                drop(sync);

                let network = self.inner.network.read().await;
                network.send(
                    &peer_id.to_string(),
                    network::Message::SyncResponse { data: all_changes },
                ).await?;
            }

            network::Message::SyncResponse { data } => {
                // Merge received state
                let mut sync = self.inner.sync.write().await;
                sync.load_document(&data)?;
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

        Ok(Swarm {
            inner: Arc::new(SwarmInner {
                storage,
                network: RwLock::new(network),
                election: RwLock::new(election),
                sync: RwLock::new(sync),
                shutdown_tx,
            }),
        })
    }
}
