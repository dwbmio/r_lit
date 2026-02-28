//! iroh-based discovery implementation
//!
//! Uses iroh's built-in LocalSwarmDiscovery instead of mdns-sd

use crate::{Error, Result};
use iroh_net::Endpoint;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, debug};

/// 发现的节点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    /// 节点 ID
    pub node_id: String,
    /// 用户昵称
    pub nickname: String,
    /// 群组 ID
    pub group_id: String,
    /// 节点地址（用于连接）
    pub node_addr: String,
}

/// iroh-based discovery
pub struct IrohDiscovery {
    endpoint: Endpoint,
}

impl IrohDiscovery {
    /// Create a new iroh discovery instance
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    /// Discover peers in a specific group
    ///
    /// Note: iroh's LocalSwarmDiscovery discovers ALL nodes on the local network,
    /// but doesn't have group filtering built-in. We need to implement group
    /// filtering at a higher level (e.g., through custom metadata exchange).
    ///
    /// For now, this returns all discovered nodes.
    pub async fn discover_group(&self, group_id: &str, timeout_secs: u64) -> Result<Vec<DiscoveredPeer>> {
        info!("🔍 Searching for peers on local network (timeout: {}s)...", timeout_secs);

        // Wait for discovery to happen
        tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

        // Get all known remote nodes from the endpoint
        let remote_infos = self.endpoint.remote_info_iter();

        let mut peers = Vec::new();
        for (node_id, info) in remote_infos {
            debug!("Found node: {:?}", node_id);

            // For now, we can't filter by group_id because iroh's LocalSwarmDiscovery
            // doesn't include custom metadata. We'll need to connect and exchange
            // group info, or use a different approach.

            // Create a peer entry
            let peer = DiscoveredPeer {
                node_id: node_id.to_string(),
                nickname: "Unknown".to_string(), // We don't have nickname yet
                group_id: group_id.to_string(),  // Assume same group for now
                node_addr: format!("{:?}", info.node_addr),
            };

            peers.push(peer);
        }

        info!("🔍 Found {} peers in group '{}'", peers.len(), group_id);
        Ok(peers)
    }
}
