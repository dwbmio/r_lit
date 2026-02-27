use crate::{Error, Result};
use crate::network::{Network, Message};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Node role in the distributed system.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeRole {
    Leader,
    Follower { leader_id: String },
    Candidate,
}

/// Leader election coordinator using Bully algorithm.
pub struct Election {
    node_id: String,
    role: NodeRole,
    term: u64,
    last_heartbeat: Option<Instant>,
    election_timeout: Duration,
    heartbeat_interval: Duration,
}

impl Election {
    /// Create a new election coordinator with the given node ID.
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            role: NodeRole::Candidate,
            term: 0,
            last_heartbeat: None,
            election_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(2),
        }
    }

    /// Get the current node ID.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Get the current role.
    pub fn role(&self) -> &NodeRole {
        &self.role
    }

    /// Get the current term.
    pub fn term(&self) -> u64 {
        self.term
    }

    /// Check if this node is the leader.
    pub fn is_leader(&self) -> bool {
        matches!(self.role, NodeRole::Leader)
    }

    /// Get the current leader ID if known.
    pub fn leader_id(&self) -> Option<String> {
        match &self.role {
            NodeRole::Leader => Some(self.node_id.clone()),
            NodeRole::Follower { leader_id } => Some(leader_id.clone()),
            NodeRole::Candidate => None,
        }
    }

    /// Start an election using Bully algorithm.
    ///
    /// Bully algorithm:
    /// 1. Send ELECTION message to all nodes with higher IDs
    /// 2. If no response within timeout, become leader
    /// 3. If any node responds with OK, wait for COORDINATOR message
    /// 4. If timeout waiting for COORDINATOR, restart election
    pub async fn start_election(&mut self, network: &Network) -> Result<()> {
        info!("Starting election (term {})", self.term + 1);
        self.term += 1;
        self.role = NodeRole::Candidate;

        let my_id = self.node_id.clone();
        let peers = network.peers().await;

        // Find peers with higher IDs (in Bully algorithm, higher ID = higher priority)
        let higher_peers: Vec<String> = peers.into_iter()
            .filter(|peer_id| peer_id > &my_id)
            .collect();

        if higher_peers.is_empty() {
            // No higher peers, become leader immediately
            self.become_leader(network).await?;
            return Ok(());
        }

        // Send ELECTION message to higher peers
        let election_msg = Message::Election {
            candidate_id: my_id.clone(),
        };

        for peer_id in &higher_peers {
            if let Err(e) = network.send(peer_id, election_msg.clone()).await {
                warn!("Failed to send election message to {}: {}", peer_id, e);
            }
        }

        // Wait for responses (handled in handle_message)
        // If no COORDINATOR message received within timeout, restart election
        Ok(())
    }

    /// Handle incoming election-related messages.
    pub async fn handle_message(
        &mut self,
        from: &str,
        message: &Message,
        network: &Network,
    ) -> Result<()> {
        match message {
            Message::Election { candidate_id } => {
                debug!("Received ELECTION from {}", candidate_id);

                // If sender has lower ID, respond with OK and start own election
                if candidate_id < &self.node_id {
                    network.send(from, Message::ElectionOk).await?;
                    self.start_election(network).await?;
                }
            }

            Message::ElectionOk => {
                debug!("Received ELECTION_OK from {}", from);
                // A higher node responded, so we won't become leader
                // Wait for COORDINATOR message
            }

            Message::Coordinator { leader_id, term } => {
                info!("Received COORDINATOR from {} (term {})", leader_id, term);

                if *term >= self.term {
                    self.term = *term;
                    self.become_follower(leader_id.clone());
                    self.last_heartbeat = Some(Instant::now());
                }
            }

            Message::Heartbeat { leader_id, term } => {
                debug!("Received HEARTBEAT from {} (term {})", leader_id, term);

                if *term >= self.term {
                    self.term = *term;
                    if !self.is_leader() {
                        self.become_follower(leader_id.clone());
                    }
                    self.last_heartbeat = Some(Instant::now());
                }
            }

            _ => {}
        }

        Ok(())
    }

    /// Become the leader and announce to all peers.
    async fn become_leader(&mut self, network: &Network) -> Result<()> {
        info!("Becoming leader (term {})", self.term);
        self.role = NodeRole::Leader;

        // Broadcast COORDINATOR message
        let coordinator_msg = Message::Coordinator {
            leader_id: self.node_id.clone(),
            term: self.term,
        };

        network.broadcast(coordinator_msg).await?;
        Ok(())
    }

    /// Become a follower of the given leader.
    fn become_follower(&mut self, leader_id: String) {
        if self.role != (NodeRole::Follower { leader_id: leader_id.clone() }) {
            info!("Becoming follower of {}", leader_id);
            self.role = NodeRole::Follower { leader_id };
        }
    }

    /// Send heartbeat if this node is the leader.
    pub async fn send_heartbeat_if_leader(&self, network: &Network) -> Result<()> {
        if self.is_leader() {
            let heartbeat_msg = Message::Heartbeat {
                leader_id: self.node_id.clone(),
                term: self.term,
            };

            network.broadcast(heartbeat_msg).await?;
            debug!("Sent heartbeat (term {})", self.term);
        }

        Ok(())
    }

    /// Check if leader heartbeat has timed out.
    pub fn is_heartbeat_timeout(&self) -> bool {
        if self.is_leader() {
            return false;
        }

        if let Some(last_heartbeat) = self.last_heartbeat {
            last_heartbeat.elapsed() > self.election_timeout
        } else {
            // No heartbeat received yet, consider it timed out
            true
        }
    }

    /// Get the heartbeat interval for leaders.
    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_election_creation() {
        let election = Election::new("node-1".to_string());
        assert_eq!(election.node_id(), "node-1");
        assert_eq!(election.role(), &NodeRole::Candidate);
        assert_eq!(election.term(), 0);
    }

    #[test]
    fn test_leader_id() {
        let mut election = Election::new("node-1".to_string());
        assert_eq!(election.leader_id(), None);

        election.become_follower("node-2".to_string());
        assert_eq!(election.leader_id(), Some("node-2".to_string()));
    }
}
