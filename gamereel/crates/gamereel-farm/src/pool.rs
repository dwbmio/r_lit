//! `WorkerPool` — thin abstraction over N LocalWorkers running on
//! their own OS threads. Used directly by tests and by the actix-based
//! `JobQueue` (M5-4) which adds priority + backpressure on top.
//!
//! Why threads not async: each LocalWorker owns CUDA + ffmpeg state
//! that's effectively pinned to its OS thread (CUDA context affinity).
//! tokio's work-stealing scheduler would migrate them around — fatal.
//! So each worker runs `block_in_place`-style work on a dedicated
//! `std::thread`, and the pool dispatches via crossbeam-style channels.

use crate::job::{RenderJob, RenderResult};
use crate::worker::local::LocalWorker;
use crate::worker::Worker;
use crate::FarmError;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

/// Outbound message a worker thread emits when it finishes a job.
pub type WorkerOutcome = Result<RenderResult, FarmError>;

/// Owns N worker threads and the channels to dispatch jobs to them.
/// `submit` is non-blocking; `collect_one` blocks for the next done
/// job; `shutdown` waits for all in-flight to finish.
pub struct WorkerPool {
    job_txs: Vec<mpsc::SyncSender<RenderJob>>,
    result_rx: mpsc::Receiver<WorkerOutcome>,
    handles: Vec<thread::JoinHandle<()>>,
    next_worker: usize,
}

impl WorkerPool {
    /// Spawn `count` LocalWorker threads, each bound to width × height.
    /// Returns once each worker has finished CUDA init (so submit() is
    /// safe to call immediately).
    pub fn spawn(count: usize, width: u32, height: u32) -> Result<Self, FarmError> {
        if count == 0 {
            return Err(FarmError::Supervisor("pool size 0".into()));
        }
        let (result_tx, result_rx) = mpsc::channel::<WorkerOutcome>();
        let mut job_txs = Vec::with_capacity(count);
        let mut handles = Vec::with_capacity(count);

        // Init workers serially — running NVRTC compile concurrently
        // can hammer the driver. Once init is done they run in parallel.
        for i in 0..count {
            let id = format!("local-{i}");
            let mut worker = LocalWorker::new(&id, width, height)?;
            let (tx, rx) = mpsc::sync_channel::<RenderJob>(1); // bounded=1: backpressure
            let result_tx = result_tx.clone();
            let handle = thread::Builder::new()
                .name(id.clone())
                .spawn(move || {
                    worker_loop(&mut worker, rx, result_tx);
                })
                .map_err(|e| FarmError::Supervisor(format!("thread spawn: {e}")))?;
            job_txs.push(tx);
            handles.push(handle);
        }
        // Drop the original result_tx so when all worker clones are
        // dropped at shutdown, the receiver returns Disconnected.
        drop(result_tx);

        Ok(Self {
            job_txs,
            result_rx,
            handles,
            next_worker: 0,
        })
    }

    pub fn worker_count(&self) -> usize { self.job_txs.len() }

    /// Round-robin submit. Returns Err(QueueFull) if every worker has
    /// a job queued (each worker's mailbox is bounded=1; pool capacity
    /// is `worker_count`). Caller keeps ownership of the job — clone
    /// happens internally only on the path that succeeds.
    pub fn submit(&mut self, job: &RenderJob) -> Result<(), FarmError> {
        let n = self.job_txs.len();
        for i in 0..n {
            let idx = (self.next_worker + i) % n;
            match self.job_txs[idx].try_send(job.clone()) {
                Ok(()) => {
                    self.next_worker = (idx + 1) % n;
                    return Ok(());
                }
                Err(mpsc::TrySendError::Full(_)) => continue,
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(FarmError::Supervisor(format!(
                        "worker {idx} disconnected (panicked?)"
                    )));
                }
            }
        }
        Err(FarmError::QueueFull { capacity: n })
    }

    /// Blocking submit — caller waits inside `mpsc::SyncSender::send` until
    /// the chosen worker can accept. Goes round-robin so submits spread
    /// load. Useful for the bench / batch CLI where caller doesn't want
    /// to manage backpressure explicitly.
    pub fn submit_blocking(&mut self, job: &RenderJob) -> Result<(), FarmError> {
        let n = self.job_txs.len();
        let idx = self.next_worker;
        match self.job_txs[idx].send(job.clone()) {
            Ok(()) => {
                self.next_worker = (idx + 1) % n;
                Ok(())
            }
            Err(_) => Err(FarmError::Supervisor(format!(
                "worker {idx} disconnected"
            ))),
        }
    }

    /// Block until any worker reports a result (or until the pool is
    /// fully drained, in which case Ok(None) is returned).
    pub fn collect_one(&self) -> Result<Option<WorkerOutcome>, FarmError> {
        match self.result_rx.recv() {
            Ok(r) => Ok(Some(r)),
            Err(_) => Ok(None),
        }
    }

    /// Block-with-timeout variant; returns Ok(None) on timeout.
    pub fn collect_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<WorkerOutcome>, FarmError> {
        match self.result_rx.recv_timeout(timeout) {
            Ok(r) => Ok(Some(r)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    /// Submit nothing more; wait for all in-flight to finish; join.
    pub fn shutdown(mut self) -> Result<(), FarmError> {
        // Closing all senders signals workers to exit their loops.
        let n = self.job_txs.len();
        self.job_txs.clear();

        // Drain remaining results into the void.
        while let Ok(_) = self.result_rx.recv() {}

        for h in self.handles.drain(..) {
            let name = h.thread().name().unwrap_or("?").to_string();
            if let Err(e) = h.join() {
                log::warn!("worker '{name}' panicked: {e:?}");
            }
        }
        Ok(())
    }
}

fn worker_loop(
    worker: &mut LocalWorker,
    rx: mpsc::Receiver<RenderJob>,
    tx: mpsc::Sender<WorkerOutcome>,
) {
    let id = worker.id().to_string();
    log::info!("worker '{id}' loop started");
    // Worker uses synchronous render; use a single-thread tokio runtime
    // for the async render() call.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Err(FarmError::Supervisor(format!(
                "worker '{id}' tokio init: {e}"
            ))));
            return;
        }
    };

    while let Ok(job) = rx.recv() {
        let job_id = job.id.clone();
        let started = Instant::now();
        let outcome = rt.block_on(async { worker.render(job).await });
        let elapsed = started.elapsed();
        let send_result: WorkerOutcome = match outcome {
            Ok(r) => Ok(r),
            Err(e) => Err(FarmError::Worker(e)),
        };
        log::debug!(
            "worker '{id}' completed job '{job_id}' in {} ms",
            elapsed.as_millis()
        );
        if tx.send(send_result).is_err() {
            log::warn!("worker '{id}' result channel closed; exiting");
            break;
        }
    }
    log::info!("worker '{id}' loop exited (jobs_completed={})", worker.jobs_completed());
}
