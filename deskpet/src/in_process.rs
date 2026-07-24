//! 同进程 RPC 调用 — 直接向 `RpcTaskInbox` 入队，由 Bevy `drain_rpc_tasks` 处理。
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::rpc::bevy_bridge::{RpcTask, RpcTaskInbox, make_reply_channel};
use crate::rpc::{Outcome, Request, Response};

#[derive(Debug, thiserror::Error)]
pub enum InProcessError {
    #[error("rpc inbox closed: {0}")]
    InboxClosed(String),
    #[error("rpc reply channel closed")]
    ReplyClosed,
    #[error("deskpet rpc error: {0}")]
    Rpc(String),
}

fn next_request_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let init = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let _ = COUNTER.compare_exchange(0, init, Ordering::SeqCst, Ordering::SeqCst);
    COUNTER.fetch_add(1, Ordering::SeqCst) + 1
}

/// 同进程调用：入队后阻塞等待 Bevy drain 回写 result。
pub fn call_via_inbox(
    inbox: &RpcTaskInbox,
    method: &str,
    params: Value,
) -> Result<Value, InProcessError> {
    let (reply_tx, reply_rx) = make_reply_channel();
    let request = Request {
        id: next_request_id(),
        method: method.to_string(),
        params,
    };
    let task = RpcTask {
        request,
        reply: reply_tx,
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    inbox
        .enqueue(task)
        .map_err(|e| InProcessError::InboxClosed(e.0))?;

    let response: Response = reply_rx
        .recv()
        .map_err(|_| InProcessError::ReplyClosed)?;

    match response.outcome {
        Outcome::Ok { result } => Ok(result),
        Outcome::Err { error } => Err(InProcessError::Rpc(format!(
            "{} (code {})",
            error.message, error.code
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_id_is_monotonic() {
        let a = next_request_id();
        let b = next_request_id();
        assert!(b > a);
    }
}
