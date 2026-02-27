use crate::{Error, Result};
use crate::vector_clock::VectorClock;
use iroh::net::endpoint::{Endpoint, Connection, Connecting};
use iroh::net::key::PublicKey;
use iroh::net::NodeAddr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
    /// Request full state sync
    SyncRequest,
    /// Response with full state
    SyncResponse { data: Vec<u8> },
    /// Acknowledgment for reliable delivery
    Ack { seq_num: u64 },
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
}

impl Network {
    /// Create a new network layer with a group ID.
    pub async fn new(group_id: String) -> Result<Self> {
        // Create iroh endpoint with default relay
        let endpoint = Endpoint::builder()
            .discovery_n0()
            .bind()
            .await
            .map_err(|e| Error::Network(format!("Failed to create endpoint: {}", e)))?;

        let node_id = endpoint.node_id();
        info!("Network initialized with NodeId: {} (group: {})", node_id, group_id);

        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Ok(Self {
            endpoint,
            node_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            group_id,
            seq_num: Arc::new(RwLock::new(0)),
            vector_clock: Arc::new(RwLock::new(VectorClock::new())),
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
    pub async fn node_addr(&self) -> NodeAddr {
        self.endpoint.node_addr().await
            .expect("Failed to get node address")
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
        let conn = self.endpoint.connect(peer_addr, b"murmur")
            .await
            .map_err(|e| Error::Network(format!("Failed to connect: {}", e)))?;

        info!("Connected to peer: {}", peer_id);

        // Store connection
        let mut peers = self.peers.write().await;
        peers.insert(peer_id, conn.clone());
        drop(peers);

        // Spawn task to handle incoming messages from this peer
        self.spawn_peer_handler(peer_id, conn);

        Ok(())
    }

    /// Spawn a task to handle messages from a peer.
    fn spawn_peer_handler(&self, peer_id: NodeId, conn: Connection) {
        let message_tx = self.message_tx.clone();
        let peers = self.peers.clone();

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
                                error!("Failed to deserialize message from {}: {}", peer_id, e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Connection closed with {}: {}", peer_id, e);
                        break;
                    }
                }
            }

            // Remove peer on disconnect
            let mut peers_lock = peers.write().await;
            peers_lock.remove(&peer_id);
            info!("Peer disconnected: {}", peer_id);
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

    /// Receive the next message from any peer.
    pub async fn recv(&self) -> Option<(NodeId, Message)> {
        let mut rx = self.message_rx.write().await;
        rx.recv().await
    }

    /// Start accepting incoming connections.
    pub async fn start_accepting(&self) -> Result<()> {
        let endpoint = self.endpoint.clone();
        let peers = self.peers.clone();
        let message_tx = self.message_tx.clone();

        tokio::spawn(async move {
            loop {
                match endpoint.accept().await {
                    Some(incoming) => {
                        tokio::spawn(Self::handle_incoming(incoming, peers.clone(), message_tx.clone()));
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
        incoming: iroh::net::endpoint::Incoming,
        peers: Arc<RwLock<HashMap<NodeId, Connection>>>,
        message_tx: mpsc::UnboundedSender<(NodeId, Message)>,
    ) {
        match incoming.await {
            Ok(conn) => {
                // Note: iroh 0.28 doesn't provide easy access to remote peer ID
                // We'll handle incoming messages without tracking the peer
                info!("Accepted incoming connection");

                // Handle messages from this peer
                loop {
                    match conn.accept_uni().await {
                        Ok(mut recv_stream) => {
                            let mut buf = Vec::new();
                            match recv_stream.read_to_end(1024 * 1024).await {
                                Ok(bytes) => buf = bytes,
                                Err(e) => {
                                    error!("Failed to read from incoming connection: {}", e);
                                    break;
                                }
                            }

                            match bincode::deserialize::<Message>(&buf) {
                                Ok(message) => {
                                    debug!("Received message from incoming connection");
                                    // Without peer_id, we skip message handling for now
                                    // This is a limitation of the current implementation
                                }
                                Err(e) => {
                                    error!("Failed to deserialize message: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Connection closed: {}", e);
                            break;
                        }
                    }
                }

                info!("Incoming connection closed");
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_creation() {
        let network = Network::new().await.unwrap();
        assert!(!network.node_id_string().is_empty());
    }
}
