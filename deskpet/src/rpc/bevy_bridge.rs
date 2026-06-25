//! Bridge between the network listener threads (NDJSON, HTTP) and the Bevy
//! ECS. Listener threads parse + validate an incoming request, push an
//! `RpcTask` into a shared inbox, and block waiting for the reply. A Bevy
//! exclusive system drains the inbox each frame, invokes the method with
//! `World` access, and sends the reply back via the per-task oneshot.
//!
//! # Why exclusive systems?
//!
//! Methods need `&mut World` to read/write Bevy resources, and a regular
//! system can't take that. Bevy's exclusive systems (`fn(world: &mut
//! World)`) give us the full World while still running inside the
//! schedule — so the dispatch system interleaves with the rest of the
//! app's update loop instead of racing against it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use super::dispatch::{dispatch_request, MethodRegistry};
use super::Request;
use super::Response as RpcResponse;
use super::Response;

/// Maximum number of in-flight RPC tasks queued in the inbox. Listener
/// threads block on `SyncSender::send` when the inbox is full, providing
/// natural backpressure.
const INBOX_CAPACITY: usize = 64;

/// One in-flight RPC, sitting in the inbox between listener thread and
/// the Bevy drain system.
pub struct RpcTask {
    pub request: Request,
    /// Sender for the response. Always present for request/response, but
    /// kept optional to support future one-way notifications cleanly.
    pub reply: SyncSender<Response>,
    /// Cancellation flag. Set by `$/cancelRequest` from any listener thread
    /// to ask the drain system to skip this task. The Bevy drain checks
    /// this flag before invoking the method.
    pub cancelled: Arc<AtomicBool>,
}

/// Shared inbox between listener threads and the Bevy drain system.
/// Cloneable (`Arc<Mutex<...>>` inside) so each listener thread holds
/// its own handle.
#[derive(Resource, Clone)]
pub struct RpcTaskInbox {
    inner: Arc<Mutex<std::collections::VecDeque<RpcTask>>>,
    /// `SyncSender` handed to listener threads when they enqueue a task.
    /// Cloning a `SyncSender` lets N listener threads share one channel
    /// endpoint; the Bevy side owns the `Receiver`.
    tx: SyncSender<RpcTask>,
    _rx: Arc<Mutex<mpsc::Receiver<RpcTask>>>,
    /// Map of in-flight request id → its cancellation flag. Populated on
    /// enqueue, drained on task completion, queried by `$/cancelRequest`.
    cancelled: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
}

impl Default for RpcTaskInbox {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcTaskInbox {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<RpcTask>(INBOX_CAPACITY);
        Self {
            inner: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            tx,
            _rx: Arc::new(Mutex::new(rx)),
            cancelled: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enqueue a task from a listener thread. Blocks if the Bevy drain
    /// system isn't keeping up (backpressure). The corresponding drain
    /// call is in `drain_rpc_tasks`.
    pub fn enqueue(&self, task: RpcTask) -> Result<(), RpcErrorSend> {
        // Register the cancellation flag under this task's request id so
        // `$/cancelRequest` can find it from any listener thread.
        if let Ok(mut map) = self.cancelled.lock() {
            map.insert(task.request.id, Arc::clone(&task.cancelled));
        }
        self.tx
            .send(task)
            .map_err(|_| RpcErrorSend("inbox closed (app shutting down?)".into()))
    }

    /// Cancel an in-flight task by id. Returns `true` if a task was found
    /// and flagged, `false` if the id wasn't in the in-flight set (either
    /// not yet enqueued, already dispatched, or already completed).
    pub fn cancel(&self, id: u64) -> bool {
        if let Ok(map) = self.cancelled.lock() {
            if let Some(flag) = map.get(&id) {
                flag.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Drain all pending tasks into the internal queue. Called by the Bevy
    /// drain system once per frame. We split the send side (tx) and the
    /// drain side (inner Mutex<VecDeque>) so listener threads don't hold
    /// the mutex across network I/O — they just push to the sync channel.
    fn drain_from_channel(&self) -> Vec<RpcTask> {
        let mut out = Vec::new();
        // Receiver is behind an Arc<Mutex<...>> for 'static. Lock it,
        // drain everything available, drop the lock.
        if let Ok(rx) = self._rx.lock() {
            while let Ok(task) = rx.try_recv() {
                out.push(task);
            }
        }
        if let Ok(mut q) = self.inner.lock() {
            out.append(&mut q.drain(..).collect::<Vec<_>>());
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
#[error("rpc send: {0}")]
pub struct RpcErrorSend(pub String);

/// Drain system. Exclusive (takes `&mut World`) so it can call any method
/// that wants full resource access. Runs once per frame in `Update`.
pub fn drain_rpc_tasks(world: &mut World) {
    // Get a handle to the inbox without holding a borrow across .run().
    let Some(inbox) = world.get_resource::<RpcTaskInbox>().cloned() else {
        return;
    };

    let tasks = inbox.drain_from_channel();
    if tasks.is_empty() {
        return;
    }

    let registry = world
        .get_resource::<MethodRegistry>()
        .cloned()
        .unwrap_or_default();

    for task in tasks {
        // Remove the cancellation entry regardless of outcome — even if
        // cancelled, we've now "handled" the request.
        if let Ok(mut map) = inbox.cancelled.lock() {
            map.remove(&task.request.id);
        }
        // Per JSON-RPC 2.0 spec, a cancelled request returns no response
        // (or null result). We pick null + a marker in `note` so callers
        // can distinguish from a real "result: null".
        if task.cancelled.load(Ordering::Relaxed) {
            let cancelled_resp = RpcResponse::ok(
                task.request.id,
                serde_json::json!({ "cancelled": true }),
            )
            .unwrap_or_else(|_| {
                RpcResponse::err(
                    task.request.id,
                    super::error::RpcError::Internal("response serialize".into()),
                )
            });
            if let Err(e) = task.reply.send(cancelled_resp) {
                warn!("deskpet: cancelled reply dropped: {e}");
            }
            continue;
        }
        let response = dispatch_request(&registry, world, task.request);
        // Send can fail if the listener thread gave up (TCP closed). That's
        // fine — log and move on, no need to panic.
        if let Err(e) = task.reply.send(response) {
            warn!("deskpet: rpc reply dropped (listener gone): {e}");
        }
    }
}

/// Convenience for listener threads: build a oneshot-style reply channel
/// for one request. Bounded to 1 so a missed reply doesn't pile up.
pub fn make_reply_channel() -> (SyncSender<Response>, mpsc::Receiver<Response>) {
    mpsc::sync_channel(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inbox_enqueues_and_drains() {
        let inbox = RpcTaskInbox::new();
        let (tx, _rx) = make_reply_channel();
        let task = RpcTask {
            request: serde_json::from_value(json!({
                "id": 1, "method": "x", "params": {}
            }))
            .unwrap(),
            reply: tx,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        inbox.enqueue(task).expect("enqueue ok");
        let drained = inbox.drain_from_channel();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].request.id, 1);
    }
}