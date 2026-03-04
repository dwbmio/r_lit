//! Global Swarm Manager
//!
//! Maintains a long-lived tokio runtime so the swarm's background tasks
//! (heartbeat, message handler, discovery) survive across GUI operations.

use murmur::{Swarm, Result as MurmurResult};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Long-lived tokio runtime handle (thread-safe, clone-able).
static RT_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

/// Keep the runtime alive for the entire process.
static _RT_KEEPER: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Global swarm instance, protected by Mutex.
static SWARM: OnceLock<Mutex<Option<Swarm>>> = OnceLock::new();

fn rt() -> &'static tokio::runtime::Handle {
    RT_HANDLE.get_or_init(|| {
        let runtime = _RT_KEEPER.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("swarm-rt")
                .build()
                .expect("Failed to create swarm tokio runtime")
        });
        runtime.handle().clone()
    })
}

fn swarm_slot() -> &'static Mutex<Option<Swarm>> {
    SWARM.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub storage_path: PathBuf,
    pub group_id: String,
}

/// Create or return the global swarm. Blocks the calling thread.
pub fn get_or_init_swarm(config: SwarmConfig) -> MurmurResult<Swarm> {
    let mut guard = swarm_slot().lock().unwrap();

    if let Some(ref swarm) = *guard {
        log::info!("Using existing global Swarm");
        return Ok(swarm.clone());
    }

    log::info!(
        "Creating global Swarm: group_id={}, path={:?}",
        config.group_id, config.storage_path
    );

    let swarm = rt().block_on(async {
        let s = Swarm::builder()
            .storage_path(config.storage_path)
            .group_id(&config.group_id)
            .build()
            .await?;
        s.start().await?;
        Ok::<Swarm, murmur::Error>(s)
    })?;

    *guard = Some(swarm.clone());
    Ok(swarm)
}

/// Spawn an async task on the long-lived swarm runtime (non-blocking).
pub fn spawn<F>(f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    rt().spawn(f)
}

/// Block the current thread waiting for a future on the swarm runtime.
///
/// MUST NOT be called from within the swarm runtime itself.
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    rt().block_on(f)
}

/// Shutdown and clear the global Swarm. Blocks the calling thread.
pub fn shutdown_swarm() {
    let mut guard = swarm_slot().lock().unwrap();
    if let Some(swarm) = guard.take() {
        log::info!("Shutting down global Swarm");
        let _ = rt().block_on(swarm.shutdown());
    }
}

/// Get the existing swarm (non-blocking).
pub fn get_swarm() -> Option<Swarm> {
    let guard = swarm_slot().lock().ok()?;
    guard.clone()
}
