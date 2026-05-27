//! `Worker` — the abstraction that lets the dispatcher treat local
//! gamereel-core renders and (future) remote-GPU renders identically.
//!
//! Today only [`local::LocalWorker`] is implemented (M5-2). The
//! [`remote::RemoteWorker`] module exists as a stub + module documentation
//! describing the contract a future cloud-GPU implementation must
//! honor. Once that lands, no other code in this crate changes.

use crate::job::{RenderJob, RenderResult};
use async_trait::async_trait;

/// Worker-flavored errors the dispatcher needs to distinguish.
#[derive(thiserror::Error, Debug)]
pub enum WorkerError {
    #[error("worker init failed: {0}")]
    Init(String),

    #[error("render failed for job '{job_id}': {reason}")]
    Render { job_id: String, reason: String },

    #[error("worker is shutting down, refusing new jobs")]
    Shutdown,

    #[error("transport error: {0}")]
    Transport(String),
}

/// Self-describing worker classification — used by the supervisor to
/// pick a homogeneous pool size and by metrics to label per-worker
/// telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    /// In-process gamereel-core render. Holds persistent CUDA + ffmpeg
    /// contexts; one per OS thread is the typical deployment.
    Local,
    /// gRPC / HTTP to a remote GPU node. Stub today (see
    /// [`remote::RemoteWorker`]).
    Remote,
}

/// Workers are async because remote workers will spend most of their
/// time awaiting network IO; local workers `block_in_place` around the
/// CPU-bound section so the executor can keep the queue draining.
#[async_trait]
pub trait Worker: Send + Sync {
    /// Stable identifier. Convention: `"local-{n}"` / `"remote-{host}-{n}"`.
    fn id(&self) -> &str;

    fn kind(&self) -> WorkerKind;

    /// Render one job. Implementations MUST NOT panic on a failed job —
    /// return [`WorkerError::Render`] so the supervisor can decide
    /// whether to retry / drop / blacklist.
    async fn render(&mut self, job: RenderJob) -> Result<RenderResult, WorkerError>;

    /// Best-effort drain. Implementations may use this to flush in-flight
    /// state before being dropped; the default is a no-op.
    async fn shutdown(&mut self) -> Result<(), WorkerError> {
        Ok(())
    }
}

pub mod local;
pub mod remote;
