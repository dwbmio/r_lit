//! Global Swarm Manager
//!
//! Ensures only one Swarm instance exists throughout the application lifecycle.
//! This prevents redb database lock conflicts when switching between pages.

use murmur::{Swarm, Result as MurmurResult};
use std::sync::OnceLock;
use tokio::sync::RwLock;
use std::path::PathBuf;

/// Global Swarm instance
static GLOBAL_SWARM: OnceLock<RwLock<Option<Swarm>>> = OnceLock::new();

/// Swarm configuration
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub storage_path: PathBuf,
    pub group_id: String,
}

/// Initialize or get the global Swarm instance
///
/// # Arguments
/// - `config`: Swarm configuration (storage path and group ID)
///
/// # Returns
/// A clone of the global Swarm instance
///
/// # Behavior
/// - First call: Creates a new Swarm with the provided config
/// - Subsequent calls: Returns the existing Swarm (ignores new config)
/// - Thread-safe: Multiple concurrent calls are safe
pub async fn get_or_init_swarm(config: SwarmConfig) -> MurmurResult<Swarm> {
    let swarm_lock = GLOBAL_SWARM.get_or_init(|| RwLock::new(None));

    let mut swarm_opt = swarm_lock.write().await;

    if let Some(swarm) = swarm_opt.as_ref() {
        // Swarm already exists, return a clone
        log::info!("Using existing global Swarm instance");
        return Ok(swarm.clone());
    }

    // Create new Swarm
    log::info!(
        "Creating new global Swarm instance: group_id={}, storage_path={:?}",
        config.group_id,
        config.storage_path
    );

    let swarm = Swarm::builder()
        .storage_path(config.storage_path)
        .group_id(config.group_id)
        .build()
        .await?;

    swarm.start().await?;

    *swarm_opt = Some(swarm.clone());

    Ok(swarm)
}

/// Get the existing Swarm instance without creating a new one
///
/// # Returns
/// - `Some(Swarm)` if a Swarm has been initialized
/// - `None` if no Swarm exists yet
pub async fn get_swarm() -> Option<Swarm> {
    let swarm_lock = GLOBAL_SWARM.get()?;
    let swarm_opt = swarm_lock.read().await;
    swarm_opt.clone()
}

/// Shutdown and clear the global Swarm instance
///
/// This should be called when the application exits or when switching to a different group.
pub async fn shutdown_swarm() -> MurmurResult<()> {
    if let Some(swarm_lock) = GLOBAL_SWARM.get() {
        let mut swarm_opt = swarm_lock.write().await;

        if let Some(swarm) = swarm_opt.take() {
            log::info!("Shutting down global Swarm instance");
            swarm.shutdown().await?;
        }
    }

    Ok(())
}

/// Check if a Swarm instance exists
pub async fn has_swarm() -> bool {
    if let Some(swarm_lock) = GLOBAL_SWARM.get() {
        let swarm_opt = swarm_lock.read().await;
        swarm_opt.is_some()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_global_swarm_singleton() {
        let config1 = SwarmConfig {
            storage_path: PathBuf::from("/tmp/test_swarm_1"),
            group_id: "test_group_1".to_string(),
        };

        let config2 = SwarmConfig {
            storage_path: PathBuf::from("/tmp/test_swarm_2"),
            group_id: "test_group_2".to_string(),
        };

        // First call creates Swarm with config1
        let swarm1 = get_or_init_swarm(config1.clone()).await.unwrap();
        let node_id1 = swarm1.node_id().await;

        // Second call returns the same Swarm (ignores config2)
        let swarm2 = get_or_init_swarm(config2).await.unwrap();
        let node_id2 = swarm2.node_id().await;

        // Both should have the same node ID
        assert_eq!(node_id1, node_id2);

        // Cleanup
        shutdown_swarm().await.unwrap();
        let _ = std::fs::remove_dir_all("/tmp/test_swarm_1");
    }
}
