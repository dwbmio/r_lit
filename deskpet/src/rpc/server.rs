//! NDJSON RPC server: `TcpListener` on `127.0.0.1:47800` (loopback only),
//! one thread per connection, NDJSON request/response loop. Replaces the
//! old `notify.rs` listener. Public so `deskpet send` and the HTTP handler
//! can use the same dispatch path (HTTP just wraps each request in a
//! oneshot reply channel and forwards).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;
use std::thread;

use log::{info, warn};

use super::bevy_bridge::{make_reply_channel, RpcTask, RpcTaskInbox};
use super::{parse_request, Request, Response};

/// Default loopback port for the NDJSON RPC surface. Override via
/// `DESKPET_RPC_PORT` so dev/test/prod can coexist on one machine.
pub const DEFAULT_NDJSON_PORT: u16 = 47800;

pub fn listen_addr() -> String {
    let port = std::env::var("DESKPET_RPC_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_NDJSON_PORT);
    format!("127.0.0.1:{port}")
}

/// Spawn the NDJSON listener thread. The Bevy-side `RpcTaskInbox` is the
/// other half — listener threads push tasks to it, Bevy drains each frame.
pub fn spawn_listener(inbox: RpcTaskInbox) -> std::io::Result<()> {
    let addr = listen_addr();
    let listener = TcpListener::bind(&addr)?;
    info!("deskpet: NDJSON RPC listening on {addr}");

    thread::Builder::new()
        .name("deskpet-rpc-ndjson".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(300)))
                    .ok();
                stream
                    .set_write_timeout(Some(std::time::Duration::from_secs(5)))
                    .ok();
                let inbox = inbox.clone();
                let _ = thread::Builder::new()
                    .name("deskpet-rpc-ndjson-conn".into())
                    .spawn(move || handle_conn(stream, inbox));
            }
        })
        .map_err(|e| std::io::Error::other(format!("spawn listener thread: {e}")))?;

    Ok(())
}

/// Handle one TCP connection: read NDJSON lines, dispatch each as a request,
/// write the response back as NDJSON.
fn handle_conn(stream: TcpStream, inbox: RpcTaskInbox) {
    let peer = stream.peer_addr().ok();
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut writer = stream;

    for line in reader.lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse the request. Malformed JSON is a wire-level error; we send
        // back a Response with code -32700 (parse error) per JSON-RPC 2.0.
        let request: Request = match serde_json::from_str::<serde_json::Value>(line) {
            Err(e) => {
                let resp = Response::err(
                    0,
                    super::error::RpcError::Parse(e.to_string()),
                );
                write_response_line(&mut writer, &resp);
                continue;
            }
            Ok(v) => match parse_request(v) {
                Ok(r) => r,
                Err(e) => {
                    // We don't know the id (couldn't parse); use 0.
                    let resp = Response::err(0, e);
                    write_response_line(&mut writer, &resp);
                    continue;
                }
            },
        };

        let (tx, rx) = make_reply_channel();
        let task = RpcTask {
            request,
            reply: tx,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        if let Err(e) = inbox.enqueue(task) {
            warn!("deskpet: rpc inbox closed, dropping connection {peer:?}: {e}");
            break;
        }

        // Wait for Bevy to dispatch and reply. The 5-second write timeout
        // caps the wait indirectly — if we don't get a reply in time, the
        // caller will time out on write.
        let response = match rx.recv() {
            Ok(r) => r,
            Err(e) => {
                warn!("deskpet: rpc reply channel closed for {peer:?}: {e}");
                break;
            }
        };

        write_response_line(&mut writer, &response);
    }
}

fn write_response_line<W: Write>(writer: &mut W, response: &Response) {
    let line = match serde_json::to_string(response) {
        Ok(s) => s,
        Err(e) => {
            warn!("deskpet: response serialize failed: {e}");
            return;
        }
    };
    if let Err(e) = writeln!(writer, "{line}") {
        warn!("deskpet: response write failed: {e}");
    }
    let _ = writer.flush();
}

// `TryRecvError` import is reserved for a future notification-only mode.
#[allow(dead_code)]
fn _try_recv_used() -> TryRecvError {
    TryRecvError::Empty
}