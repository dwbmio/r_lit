//! gamereel-farm — batch video rendering with a pool of long-lived
//! workers. The point of this crate is to amortize the 284 ms CUDA
//! initialization tax measured in M3: a worker holds its
//! `CudaConverter`, `CudaHwContext`, and ffmpeg encoder context across
//! many jobs, paying init exactly once per worker process.
//!
//! Architecture
//!   * [`RenderJob`] — the unit of work submitted to the queue.
//!   * [`RenderResult`] — what the worker reports back per job.
//!   * [`Worker`] — async trait abstracting "thing that can render a job".
//!     - `LocalWorker` lives here in M5-2; runs gamereel-core in-process.
//!     - `RemoteWorker` is a stub today (see [`worker::remote`]); M5+1
//!       will plug in gRPC to a cloud GPU node without touching the
//!       dispatcher or job-queue code.
//!   * [`Supervisor`], [`JobQueue`] — actix actors wired in M5-3..4.
//!
//! Why actix not tokio-channels: the user explicitly asked for actor
//! semantics. actix 0.13 runs on top of tokio so we don't pay a second
//! runtime; the surrounding workspace stays tokio-based.

pub mod job;
pub mod pool;
pub mod probe;
pub mod worker;

pub use job::{JobPriority, RenderJob, RenderResult};
pub use pool::WorkerPool;
pub use probe::{probe_first_gpu, recommended_worker_count, GpuInfo};
pub use worker::{Worker, WorkerError, WorkerKind};

/// Top-level error type for the farm runtime.
#[derive(thiserror::Error, Debug)]
pub enum FarmError {
    #[error("worker error: {0}")]
    Worker(#[from] WorkerError),

    #[error("queue full (mailbox capacity {capacity}, backpressure)")]
    QueueFull { capacity: usize },

    #[error("supervisor error: {0}")]
    Supervisor(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde_json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type FarmResult<T> = Result<T, FarmError>;
