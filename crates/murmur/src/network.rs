use crate::{Error, Result};
use crate::vector_clock::VectorClock;
use iroh_net::endpoint::{Endpoint, Connection, ServerConfig, TransportConfig};
use iroh_net::key::PublicKey;
use iroh_net::NodeAddr;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Type alias for node ID
pub type NodeId = PublicKey;

/// Message types for P2P communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Election message (Bully algorithm)
    Election { candidate_id: String },
    /// Response to election message
    ElectionOk,
    /// Coordinator announcement
    Coordinator { leader_id: String, term: u64 },
    /// Heartbeat from leader
    Heartbeat { leader_id: String, term: u64 },
    /// CRDT state update with causality tracking
    CrdtUpdate {
        key: String,
        operation: Vec<u8>,
        seq_num: u64,           // Sequence number from sender
        vector_clock: VectorClock, // Causal ordering
    },
    /// Incremental sync message (automerge sync protocol).
    /// Multi-round: peers exchange these until both return None.
    SyncMsg { data: Vec<u8> },
    /// Acknowledgment for reliable delivery
    Ack { seq_num: u64 },
    /// Lock a file due to conflict (all nodes must pause writes to this file)
    ConflictLock {
        file_name: String,
        resolver_node: String,
        expected_version: u64,
        current_version: u64,
    },
    /// Unlock a file after conflict resolution
    ConflictUnlock {
        file_name: String,
        resolved_by: String,
        new_version: u64,
    },
}

const MAX_RETRANSMIT_ATTEMPTS: u32 = 5;
const RETRANSMIT_INTERVAL_MS: u64 = 3000;

/// A message awaiting ACK from a peer.
#[derive(Debug, Clone)]
pub struct PendingMessage {
    pub peer_id: NodeId,
    pub message: Message,
    pub seq_num: u64,
    pub sent_at: Instant,
    pub attempts: u32,
}

/// Peer-level events emitted by the network layer.
#[derive(Debug, Clone)]
pub enum PeerEvent {
    Connected(String),
    Disconnected(String),
}

/// Derive a per-group ALPN protocol identifier.
/// Different group_ids produce different ALPNs, so QUIC handshakes between
/// nodes in different groups will fail at the transport layer.
fn group_alpn(group_id: &str) -> Vec<u8> {
    format!("murmur/{}", group_id).into_bytes()
}

/// P2P network layer using iroh.
pub struct Network {
    endpoint: Endpoint,
    node_id: NodeId,
    peers: Arc<RwLock<HashMap<NodeId, Connection>>>,
    message_tx: mpsc::UnboundedSender<(NodeId, Message)>,
    message_rx: Arc<RwLock<mpsc::UnboundedReceiver<(NodeId, Message)>>>,
    /// Group ID for isolating different swarms
    group_id: String,
    /// Sequence number for outgoing messages
    seq_num: Arc<RwLock<u64>>,
    /// Vector clock for causal ordering
    vector_clock: Arc<RwLock<VectorClock>>,
    /// Server config for accepting connections with correct ALPN
    server_config: Arc<ServerConfig>,
    /// Messages awaiting ACK, keyed by seq_num
    pending_acks: Arc<RwLock<HashMap<u64, PendingMessage>>>,
    /// Peer connect/disconnect events
    peer_event_tx: tokio::sync::broadcast::Sender<PeerEvent>,
    /// Peers that disconnected immediately (stale mDNS), skip during discovery
    stale_peers: Arc<RwLock<HashSet<NodeId>>>,
}

impl Network {
    /// Create a new network layer with a group ID.
    pub async fn new(group_id: String) -> Result<Self> {
        use iroh_net::discovery::local_swarm_discovery::LocalSwarmDiscovery;
        use iroh_net::discovery::ConcurrentDiscovery;
        use iroh_net::key::SecretKey;

        // Generate or load secret key
        let secret_key = SecretKey::generate();
        let node_id = secret_key.public();

        // Create discovery with LocalSwarmDiscovery for local network
        let local_discovery = LocalSwarmDiscovery::new(node_id)
            .map_err(|e| Error::Network(format!("Failed to create local discovery: {}", e)))?;

        let discovery = ConcurrentDiscovery::from_services(vec![
            Box::new(local_discovery),
        ]);

        // Create iroh endpoint with local discovery
        let endpoint = Endpoint::builder()
            .secret_key(secret_key.clone())
            .discovery(Box::new(discovery))
            .bind()
            .await
            .map_err(|e| Error::Network(format!("Failed to create endpoint: {}", e)))?;

        // Create server config with group-specific ALPN for network isolation
        let alpn_protocols = vec![group_alpn(&group_id)];
        let transport_config = Arc::new(TransportConfig::default());
        let server_config = iroh_net::endpoint::make_server_config(
            &secret_key,
            alpn_protocols,
            transport_config,
            false, // keylog disabled
        )
        .map_err(|e| Error::Network(format!("Failed to create server config: {}", e)))?;

        info!("Network initialized with NodeId: {} (group: {})", node_id, group_id);

        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let (peer_event_tx, _) = tokio::sync::broadcast::channel(32);

        Ok(Self {
            endpoint,
            node_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            group_id,
            seq_num: Arc::new(RwLock::new(0)),
            vector_clock: Arc::new(RwLock::new(VectorClock::new())),
            server_config: Arc::new(server_config),
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
            peer_event_tx,
            stale_peers: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Get the next sequence number.
    pub async fn next_seq_num(&self) -> u64 {
        let mut seq = self.seq_num.write().await;
        *seq += 1;
        *seq
    }

    /// Get a copy of the current vector clock.
    pub async fn get_vector_clock(&self) -> VectorClock {
        self.vector_clock.read().await.clone()
    }

    /// Update vector clock after sending a message.
    pub async fn increment_vector_clock(&self) {
        let mut vc = self.vector_clock.write().await;
        vc.increment(&self.node_id_string());
    }

    /// Merge received vector clock.
    pub async fn merge_vector_clock(&self, other: &VectorClock) {
        let mut vc = self.vector_clock.write().await;
        vc.merge(other);
        vc.increment(&self.node_id_string());
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get this node's ID as string.
    pub fn node_id_string(&self) -> String {
        self.node_id.to_string()
    }

    /// Get the group ID.
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// Get the node address (for sharing with other peers).
    pub async fn node_addr(&self) -> Result<NodeAddr> {
        self.endpoint.node_addr().await
            .map_err(|e| Error::Network(format!("Failed to get node address: {}", e)))
    }

    /// Connect to a peer by NodeAddr.
    pub async fn connect(&self, peer_addr: NodeAddr) -> Result<()> {
        let peer_id = peer_addr.node_id;
        debug!("Connecting to peer: {}", peer_id);

        // Check if already connected
        {
            let peers = self.peers.read().await;
            if peers.contains_key(&peer_id) {
                debug!("Already connected to {}", peer_id);
                return Ok(());
            }
        }

        // Establish connection
        let alpn = group_alpn(&self.group_id);
        let conn = self.endpoint.connect(peer_addr, &alpn)
            .await
            .map_err(|e| Error::Network(format!("Failed to connect: {}", e)))?;

        info!("Connected to peer: {}", peer_id);

        // Store connection (with deterministic duplicate resolution)
        let mut peers = self.peers.write().await;
        if peers.contains_key(&peer_id) {
            // Incoming connection arrived first — deterministic tie-break by node ID.
            // Higher node ID wins as initiator (keeps outgoing conn).
            if self.node_id < peer_id {
                // The other peer is the initiator → keep their incoming conn
                debug!("Dropping duplicate outgoing connection to {} (they are initiator)", peer_id);
                return Ok(());
            }
            debug!("Replacing incoming connection from {} with outgoing (we are initiator)", peer_id);
        }
        peers.insert(peer_id, conn.clone());
        drop(peers);

        let _ = self.peer_event_tx.send(PeerEvent::Connected(peer_id.to_string()));

        // Spawn task to handle incoming messages from this peer
        self.spawn_peer_handler(peer_id, conn);

        Ok(())
    }

    /// Spawn a task to handle messages from a peer.
    fn spawn_peer_handler(&self, peer_id: NodeId, conn: Connection) {
        let message_tx = self.message_tx.clone();
        let peers = self.peers.clone();
        let peer_event_tx = self.peer_event_tx.clone();
        let pending_acks = self.pending_acks.clone();

        tokio::spawn(async move {
            loop {
                match conn.accept_uni().await {
                    Ok(mut recv_stream) => {
                        // Read message
                        let buf = match recv_stream.read_to_end(1024 * 1024).await {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Failed to read from {}: {}", peer_id, e);
                                break;
                            }
                        };

                        // Guard empty buffers (connection closed mid-stream)
                        if buf.is_empty() {
                            debug!("Received empty stream from {}, skipping", peer_id);
                            continue;
                        }

                        // Deserialize message
                        match bincode::deserialize::<Message>(&buf) {
                            Ok(message) => {
                                debug!("Received message from {}: {:?}", peer_id, message);
                                if let Err(e) = message_tx.send((peer_id, message)) {
                                    error!("Failed to forward message: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to deserialize message from {}: {}", peer_id, e);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Connection closed with {}: {}", peer_id, e);
                        break;
                    }
                }
            }

            // Only remove peer if this handler's connection is still the active one
            // (a replaced connection's handler must not remove the new connection)
            let mut peers_lock = peers.write().await;
            let should_remove = match peers_lock.get(&peer_id) {
                Some(stored) => stored.stable_id() == conn.stable_id(),
                None => false,
            };
            if should_remove {
                peers_lock.remove(&peer_id);
                drop(peers_lock);
                // Clear pending ACKs for this peer to avoid pointless retransmits
                let mut acks = pending_acks.write().await;
                let before = acks.len();
                acks.retain(|_, msg| msg.peer_id != peer_id);
                let cleared = before - acks.len();
                if cleared > 0 {
                    debug!("Cleared {} pending ACKs for disconnected peer {}", cleared, peer_id);
                }
                info!("Peer disconnected: {}", peer_id);
                let _ = peer_event_tx.send(PeerEvent::Disconnected(peer_id.to_string()));
            } else {
                debug!("Handler exiting for replaced connection to {}", peer_id);
            }
        });
    }

    /// Send a message to a specific peer.
    pub async fn send(&self, peer_id: &str, message: Message) -> Result<()> {
        let node_id = peer_id.parse::<NodeId>()
            .map_err(|e| Error::Network(format!("Invalid NodeId: {}", e)))?;

        let peers = self.peers.read().await;
        if let Some(conn) = peers.get(&node_id) {
            let data = bincode::serialize(&message)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            let mut send_stream = conn.open_uni()
                .await
                .map_err(|e| Error::Network(format!("Failed to open stream: {}", e)))?;

            send_stream.write_all(&data)
                .await
                .map_err(|e| Error::Network(format!("Failed to send data: {}", e)))?;

            send_stream.finish()
                .map_err(|e| Error::Network(format!("Failed to finish stream: {}", e)))?;

            debug!("Sent message to {}: {:?}", peer_id, message);
        } else {
            return Err(Error::Network(format!("Peer not connected: {}", peer_id)));
        }

        Ok(())
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast(&self, message: Message) -> Result<()> {
        let peers = self.peers.read().await;
        let peer_ids: Vec<NodeId> = peers.keys().copied().collect();
        drop(peers);

        for peer_id in peer_ids {
            if let Err(e) = self.send(&peer_id.to_string(), message.clone()).await {
                error!("Failed to send to {}: {}", peer_id, e);
            }
        }

        Ok(())
    }

    /// Get list of connected peer IDs.
    pub async fn peers(&self) -> Vec<String> {
        self.peers.read().await
            .keys()
            .map(|id| id.to_string())
            .collect()
    }

    /// Track a sent message that requires an ACK.
    pub async fn track_pending(&self, seq_num: u64, peer_id: NodeId, message: Message) {
        let pending = PendingMessage {
            peer_id,
            message,
            seq_num,
            sent_at: Instant::now(),
            attempts: 1,
        };
        let mut map = self.pending_acks.write().await;
        map.insert(seq_num, pending);
    }

    /// Remove a message from the pending queue (ACK received).
    /// Returns true if the message was found and removed.
    pub async fn ack_received(&self, seq_num: u64) -> bool {
        let mut map = self.pending_acks.write().await;
        let removed = map.remove(&seq_num).is_some();
        if removed {
            debug!("ACK received for seq={}, removed from pending queue", seq_num);
        }
        removed
    }

    /// Retransmit messages that have timed out without ACK.
    /// Returns the number of messages retransmitted and the seq_nums of messages that exceeded max attempts.
    pub async fn retransmit_timed_out(&self) -> (usize, Vec<u64>) {
        let now = Instant::now();
        let timeout = std::time::Duration::from_millis(RETRANSMIT_INTERVAL_MS);

        let mut to_resend: Vec<PendingMessage> = Vec::new();
        let mut failed: Vec<u64> = Vec::new();

        {
            let mut map = self.pending_acks.write().await;
            let mut remove_keys = Vec::new();

            for (seq, pending) in map.iter_mut() {
                if now.duration_since(pending.sent_at) >= timeout {
                    if pending.attempts >= MAX_RETRANSMIT_ATTEMPTS {
                        warn!("Message seq={} exceeded max retransmit attempts ({}), dropping", seq, MAX_RETRANSMIT_ATTEMPTS);
                        failed.push(*seq);
                        remove_keys.push(*seq);
                    } else {
                        pending.attempts += 1;
                        pending.sent_at = now;
                        to_resend.push(pending.clone());
                    }
                }
            }

            for key in &remove_keys {
                map.remove(key);
            }
        }

        let mut resent_count = 0;
        for pending in &to_resend {
            debug!(
                "Retransmitting seq={} to {} (attempt {})",
                pending.seq_num, pending.peer_id, pending.attempts
            );
            if let Err(e) = self.send(&pending.peer_id.to_string(), pending.message.clone()).await {
                warn!("Retransmit failed for seq={}: {}", pending.seq_num, e);
            } else {
                resent_count += 1;
            }
        }

        (resent_count, failed)
    }

    /// Get the count of messages currently pending ACK.
    pub async fn pending_ack_count(&self) -> usize {
        self.pending_acks.read().await.len()
    }

    /// Discover and connect to all peers found by iroh's discovery
    ///
    /// This method queries the endpoint's remote info to find discovered peers
    /// and attempts to connect to any that aren't already connected.
    pub async fn discover_and_connect_peers(&self) -> Result<usize> {
        let remote_infos = self.endpoint.remote_info_iter();

        let mut connected_count = 0;
        for remote_info in remote_infos {
            let node_id = remote_info.node_id;

            if node_id == self.node_id {
                continue;
            }

            {
                let peers = self.peers.read().await;
                if peers.contains_key(&node_id) {
                    continue;
                }
            }

            {
                let stale = self.stale_peers.read().await;
                if stale.contains(&node_id) {
                    continue;
                }
            }

            match self.connect(remote_info.into()).await {
                Ok(()) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    let still_connected = {
                        let peers = self.peers.read().await;
                        peers.contains_key(&node_id)
                    };
                    if still_connected {
                        info!("Connected to discovered peer: {}", node_id);
                        connected_count += 1;
                        let _ = self.peer_event_tx.send(PeerEvent::Connected(node_id.to_string()));
                    } else {
                        debug!("Peer {} disconnected immediately (stale mDNS), blacklisted", node_id);
                        self.stale_peers.write().await.insert(node_id);
                    }
                }
                Err(e) => {
                    debug!("Failed to connect to {}: {}", node_id, e);
                }
            }
        }

        Ok(connected_count)
    }

    /// Remove a peer from the stale blacklist (e.g. if it comes back online with same NodeId).
    pub async fn clear_stale_peer(&self, node_id: &NodeId) {
        self.stale_peers.write().await.remove(node_id);
    }

    /// Receive the next message from any peer.
    pub async fn recv(&self) -> Option<(NodeId, Message)> {
        let mut rx = self.message_rx.write().await;
        rx.recv().await
    }

    pub fn subscribe_peer_events(&self) -> tokio::sync::broadcast::Receiver<PeerEvent> {
        self.peer_event_tx.subscribe()
    }

    /// Start accepting incoming connections.
    pub async fn start_accepting(&self) -> Result<()> {
        let endpoint = self.endpoint.clone();
        let peers = self.peers.clone();
        let message_tx = self.message_tx.clone();
        let server_config = self.server_config.clone();
        let peer_event_tx = self.peer_event_tx.clone();
        let my_node_id = self.node_id;
        let stale_peers = self.stale_peers.clone();
        let pending_acks = self.pending_acks.clone();

        tokio::spawn(async move {
            loop {
                match endpoint.accept().await {
                    Some(incoming) => {
                        tokio::spawn(Self::handle_incoming(
                            incoming,
                            peers.clone(),
                            message_tx.clone(),
                            server_config.clone(),
                            peer_event_tx.clone(),
                            my_node_id,
                            stale_peers.clone(),
                            pending_acks.clone(),
                        ));
                    }
                    None => {
                        warn!("Endpoint closed");
                        break;
                    }
                }
            }
        });

        info!("Started accepting connections");
        Ok(())
    }

    /// Handle an incoming connection.
    async fn handle_incoming(
        incoming: iroh_net::endpoint::Incoming,
        peers: Arc<RwLock<HashMap<NodeId, Connection>>>,
        message_tx: mpsc::UnboundedSender<(NodeId, Message)>,
        server_config: Arc<ServerConfig>,
        peer_event_tx: tokio::sync::broadcast::Sender<PeerEvent>,
        my_node_id: NodeId,
        stale_peers: Arc<RwLock<HashSet<NodeId>>>,
        pending_acks: Arc<RwLock<HashMap<u64, PendingMessage>>>,
    ) {
        // Accept the connection with the server config that has group-specific ALPN
        let connecting = match incoming.accept_with(server_config) {
            Ok(connecting) => connecting,
            Err(e) => {
                error!("Failed to accept incoming connection: {}", e);
                return;
            }
        };

        // Complete the connection
        let conn = match connecting.await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to complete connection: {}", e);
                return;
            }
        };

        // Get the remote node ID from the connection
        let peer_id = match iroh_net::endpoint::get_remote_node_id(&conn) {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to get remote node ID: {}", e);
                return;
            }
        };

        if peer_id == my_node_id {
            debug!("Rejecting self-connection via mDNS cache");
            return;
        }

        info!("Accepted incoming connection from {}", peer_id);

        // Peer came back alive — remove from stale blacklist
        stale_peers.write().await.remove(&peer_id);

        // Store the connection (with deterministic duplicate resolution)
        {
            let mut peers_lock = peers.write().await;
            if peers_lock.contains_key(&peer_id) {
                // Duplicate connection — deterministic tie-break by node ID.
                // Higher node ID wins as initiator (keeps outgoing conn).
                if my_node_id > peer_id {
                    // We should be the initiator → keep our outgoing conn, drop this incoming
                    debug!("Dropping duplicate incoming connection from {} (we are initiator)", peer_id);
                    return;
                }
                // We should be the acceptor → replace outgoing with this incoming
                debug!("Replacing outgoing connection to {} with incoming (they are initiator)", peer_id);
            }
            peers_lock.insert(peer_id, conn.clone());
        }
        let _ = peer_event_tx.send(PeerEvent::Connected(peer_id.to_string()));

        // NOTE: Incremental sync is initiated by the swarm layer when it receives PeerConnected,
        // not here. This avoids issues with open_uni on the acceptor side.

        // Handle messages from this peer
        loop {
            match conn.accept_uni().await {
                Ok(mut recv_stream) => {
                    let mut buf = Vec::new();
                    match recv_stream.read_to_end(1024 * 1024).await {
                        Ok(bytes) => buf = bytes,
                        Err(e) => {
                            error!("Failed to read from {}: {}", peer_id, e);
                            break;
                        }
                    }

                    // Guard empty buffers (connection closed mid-stream)
                    if buf.is_empty() {
                        debug!("Received empty stream from {}, skipping", peer_id);
                        continue;
                    }

                    match bincode::deserialize::<Message>(&buf) {
                        Ok(message) => {
                            debug!("Received message from {}: {:?}", peer_id, message);
                            if let Err(e) = message_tx.send((peer_id, message)) {
                                error!("Failed to forward message: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Failed to deserialize message from {}: {}", peer_id, e);
                        }
                    }
                }
                Err(e) => {
                    debug!("Connection closed with {}: {}", peer_id, e);
                    break;
                }
            }
        }

        // Only remove peer if this handler's connection is still the active one
        let mut peers_lock = peers.write().await;
        let should_remove = match peers_lock.get(&peer_id) {
            Some(stored) => stored.stable_id() == conn.stable_id(),
            None => false,
        };
        if should_remove {
            peers_lock.remove(&peer_id);
            drop(peers_lock);
            // Clear pending ACKs for this peer to avoid pointless retransmits
            let mut acks = pending_acks.write().await;
            let before = acks.len();
            acks.retain(|_, msg| msg.peer_id != peer_id);
            let cleared = before - acks.len();
            if cleared > 0 {
                debug!("Cleared {} pending ACKs for disconnected peer {}", cleared, peer_id);
            }
            info!("Peer disconnected: {}", peer_id);
            let _ = peer_event_tx.send(PeerEvent::Disconnected(peer_id.to_string()));
        } else {
            debug!("Handler exiting for replaced connection to {}", peer_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_creation() {
        let network = Network::new("test-group".to_string()).await.unwrap();
        assert!(!network.node_id_string().is_empty());
        assert_eq!(network.group_id(), "test-group");
    }

    #[tokio::test]
    async fn test_pending_ack_tracking() {
        let network = Network::new("test-ack".to_string()).await.unwrap();
        let node_id = network.node_id();

        let msg = Message::Ack { seq_num: 1 };
        network.track_pending(1, node_id, msg).await;
        assert_eq!(network.pending_ack_count().await, 1);

        let removed = network.ack_received(1).await;
        assert!(removed);
        assert_eq!(network.pending_ack_count().await, 0);

        let removed_again = network.ack_received(1).await;
        assert!(!removed_again);
    }

    #[tokio::test]
    async fn test_same_group_can_connect() {
        let net_a = Network::new("alpha".to_string()).await.unwrap();
        let net_b = Network::new("alpha".to_string()).await.unwrap();

        net_b.start_accepting().await.unwrap();

        let addr_b = net_b.node_addr().await.unwrap();
        net_a.connect(addr_b).await.unwrap();

        // Give the connection a moment to stabilize
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let peers_a = net_a.peers().await;
        assert!(
            peers_a.contains(&net_b.node_id_string()),
            "same group_id nodes should connect successfully"
        );
    }

    #[tokio::test]
    async fn test_different_group_cannot_connect() {
        let net_a = Network::new("alpha".to_string()).await.unwrap();
        let net_b = Network::new("beta".to_string()).await.unwrap();

        net_b.start_accepting().await.unwrap();

        let addr_b = net_b.node_addr().await.unwrap();
        let result = net_a.connect(addr_b).await;

        // Connection should fail due to ALPN mismatch, or succeed at transport
        // but the peer handler drops it immediately. Either way, peer list must be empty.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let peers_a = net_a.peers().await;
        assert!(
            !peers_a.contains(&net_b.node_id_string()),
            "different group_id nodes must NOT be connected, but found peer in list"
        );

        let peers_b = net_b.peers().await;
        assert!(
            !peers_b.contains(&net_a.node_id_string()),
            "different group_id nodes must NOT be connected (receiver side)"
        );
    }
}
