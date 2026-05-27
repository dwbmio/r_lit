//! `RemoteWorker` — stub for a future cloud GPU executor.
//!
//! The contract the eventual implementation must honor:
//!   1. Holds a persistent gRPC / HTTP / SSH-tunnel connection to a
//!      remote node running the same `gamereel-farm` server-side
//!      (also unimplemented today).
//!   2. Serializes [`crate::job::RenderJob`] across the wire — the
//!      `serde::Serialize` derive on `RenderJob` is the contract.
//!   3. The remote node renders and ships back the resulting MP4
//!      bytes (or a CDN URL if the caller supplied one in `tag`).
//!   4. Surfaces transport-level failures as
//!      [`super::WorkerError::Transport`] so the supervisor can mark
//!      the worker degraded without retrying job-side errors.
//!
//! Implementing this should require **zero changes** outside this
//! module — that's the whole point of the trait abstraction. If a
//! future implementation finds it has to leak details into the
//! supervisor or queue, treat that as a smell and refactor.

use crate::job::{RenderJob, RenderResult};
use crate::worker::{Worker, WorkerError, WorkerKind};
use async_trait::async_trait;

/// Placeholder type. Constructing one returns an explicit "not yet
/// implemented" error so accidental wire-up in a CLI fails loudly
/// rather than silently dropping jobs.
pub struct RemoteWorker {
    id: String,
    endpoint: String,
}

impl RemoteWorker {
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>) -> Result<Self, WorkerError> {
        Err(WorkerError::Init(format!(
            "RemoteWorker not implemented yet — would target {} as id={}",
            endpoint.into(),
            id.into()
        )))
    }
}

#[async_trait]
impl Worker for RemoteWorker {
    fn id(&self) -> &str { &self.id }
    fn kind(&self) -> WorkerKind { WorkerKind::Remote }
    async fn render(&mut self, job: RenderJob) -> Result<RenderResult, WorkerError> {
        Err(WorkerError::Transport(format!(
            "RemoteWorker not implemented; can't render job '{}' against {}",
            job.id, self.endpoint
        )))
    }
}
