//! HTTP server (tiny_http) for the Swagger UI + `POST /m/<method>` RPC
//! bridge. Runs on `127.0.0.1:47801` (loopback only, separate port from
//! the NDJSON surface). One thread serves all connections — tiny_http is
//! sync and the load is trivial.
//!
//! # Routes
//!
//! | Method | Path                | Behavior                                      |
//! |--------|---------------------|-----------------------------------------------|
//! | GET    | `/`                 | Swagger UI HTML (loads spec from `/openapi.json`) |
//! | GET    | `/openapi.json`     | OpenAPI 3.0 spec (utoipa-derived)              |
//! | POST   | `/m/<method>`       | RPC call; body is JSON-RPC request, response is JSON-RPC response |
//! | GET    | `/health`           | `{"ok": true}` — for client liveness checks    |
//!
//! # Trust model
//!
//! Bound to loopback only. Any process on the same machine can hit these
//! endpoints and invoke RPC methods. The `deskpet send` CLI exercises
//! full RPC power; treat loopback access as "anyone who can run code as
//! the current user". Add bearer-token auth if you ever relax the bind
//! address.

use std::io::Read;
use std::sync::Arc;
use std::thread;

use log::{info, warn};
use serde_json::json;
use tiny_http::{Header, Method, Response, Server, StatusCode};

use super::bevy_bridge::{make_reply_channel, RpcTask, RpcTaskInbox};
use super::openapi::build_openapi_json;
use super::swagger_html::SWAGGER_HTML;
use super::{parse_request, Request, Response as RpcResponse};

pub const DEFAULT_HTTP_PORT: u16 = 47801;

pub fn listen_addr() -> String {
    let port = std::env::var("DESKPET_HTTP_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_HTTP_PORT);
    format!("127.0.0.1:{port}")
}

/// Spawn the HTTP server thread. Returns immediately; the thread lives
/// for the duration of the process.
pub fn spawn_server(inbox: RpcTaskInbox) -> std::io::Result<()> {
    let addr = listen_addr();
    let server = Server::http(&addr)
        .map_err(|e| std::io::Error::other(format!("bind {addr}: {e}")))?;
    info!("deskpet: HTTP RPC + Swagger UI on http://{addr}/");

    thread::Builder::new()
        .name("deskpet-rpc-http".into())
        .spawn(move || {
            for req in server.incoming_requests() {
                handle_request(req, &inbox);
            }
        })
        .map_err(|e| std::io::Error::other(format!("spawn http thread: {e}")))?;

    Ok(())
}

fn handle_request(mut req: tiny_http::Request, inbox: &RpcTaskInbox) {
    let url = req.url().to_string();
    let method = req.method().clone();
    let path = url.split('?').next().unwrap_or("/").to_string();

    // CORS preflight for Swagger UI / browser-based clients.
    if method == Method::Options {
        let _ = req.respond(Response::empty(204).with_header(cors_header()));
        return;
    }

    let resp: Response<std::io::Cursor<Vec<u8>>> = match (method.as_str(), path.as_str()) {
        (m, "/") if m == "GET" => serve_swagger_ui(),
        (m, "/openapi.json") if m == "GET" => serve_openapi(),
        (m, "/health") if m == "GET" => serve_health(),
        (m, p) if m == "POST" && p.starts_with("/m/") => {
            serve_rpc(req.as_reader(), inbox, &p[3..])
        }
        (m, "/batch") if m == "POST" => serve_batch(req.as_reader(), inbox),
        _ => not_found(&path),
    };

    if let Err(e) = req.respond(resp) {
        warn!("deskpet: http response write failed: {e}");
    }
}

fn serve_swagger_ui() -> Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
        .expect("static header");
    Response::from_string(SWAGGER_HTML)
        .with_status_code(StatusCode(200))
        .with_header(header)
        .with_header(cors_header())
}

fn serve_openapi() -> Response<std::io::Cursor<Vec<u8>>> {
    match build_openapi_json() {
        Ok(s) => {
            let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .expect("static header");
            Response::from_string(s)
                .with_status_code(StatusCode(200))
                .with_header(header)
                .with_header(cors_header())
        }
        Err(e) => {
            warn!("deskpet: openapi generation failed: {e}");
            server_error("openapi generation failed")
        }
    }
}

fn serve_health() -> Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    Response::from_string(json!({ "ok": true }).to_string())
        .with_status_code(StatusCode(200))
        .with_header(header)
        .with_header(cors_header())
}

/// JSON-RPC 2.0 batch handler. Body MUST be a JSON array of request
/// objects: `[{"id":1,"method":"x","params":{}}, {"id":2,...}]`. Response
/// is a JSON array of response objects in the same order.
///
/// Per JSON-RPC 2.0: a single batched request failing does NOT fail the
/// whole batch — each request gets its own response (success or error).
/// Empty arrays return an empty response (technically valid per spec,
/// though odd).
fn serve_batch<R: Read>(
    reader: R,
    inbox: &RpcTaskInbox,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = String::new();
    if let Err(e) = reader.take(1_048_576).read_to_string(&mut body) {
        return bad_request(&format!("read body: {e}"));
    }
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            let resp = RpcResponse::err(0, super::error::RpcError::Parse(e.to_string()));
            return json_response(400, &resp);
        }
    };
    let arr = match parsed {
        serde_json::Value::Array(a) => a,
        _ => {
            return bad_request("batch body must be a JSON array of request objects");
        }
    };
    if arr.is_empty() {
        // Per JSON-RPC 2.0: empty batch returns empty response array.
        return json_response_value(200, &serde_json::json!([]));
    }

    let mut responses = Vec::with_capacity(arr.len());
    for (idx, item) in arr.into_iter().enumerate() {
        // Parse each request; malformed individual request → error response,
        // don't abort the whole batch.
        let request: Request = match parse_request(item) {
            Ok(r) => r,
            Err(e) => {
                responses.push(RpcResponse::err(0, e));
                continue;
            }
        };
        let (tx, rx) = make_reply_channel();
        let task = RpcTask {
            request,
            reply: tx,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        if let Err(e) = inbox.enqueue(task) {
            // Per JSON-RPC 2.0, internal errors are encoded in the response,
            // not as HTTP errors (so the client can correlate).
            let resp = RpcResponse::err(0, super::error::RpcError::Internal(e.0));
            responses.push(resp);
            continue;
        }
        let response = match rx.recv() {
            Ok(r) => r,
            Err(_) => {
                let resp = RpcResponse::err(
                    0,
                    super::error::RpcError::Internal("reply channel closed".into()),
                );
                responses.push(resp);
                continue;
            }
        };
        responses.push(response);
        // Touch idx to silence unused warning (kept for future per-index
        // logging or rate limiting).
        let _ = idx;
    }

    json_response_value(200, &serde_json::to_value(&responses).unwrap_or(serde_json::json!([])))
}

fn json_response_value(
    status: u16,
    body: &serde_json::Value,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    Response::from_string(body.to_string())
        .with_status_code(StatusCode(status))
        .with_header(header)
        .with_header(cors_header())
}

fn serve_rpc<R: Read>(
    reader: R,
    inbox: &RpcTaskInbox,
    method: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // Read body (cap at 1 MB to avoid memory abuse from loopback peers).
    let mut body = String::new();
    if let Err(e) = reader.take(1_048_576).read_to_string(&mut body) {
        return bad_request(&format!("read body: {e}"));
    }
    if body.trim().is_empty() {
        return bad_request("empty body");
    }

    // Parse JSON, then construct a Request. We synthesize the method from
    // the URL path so Swagger UI's "Try it out" can target `/m/notification/show`
    // without needing the client to set `method` in the body too.
    let mut request: Request = match serde_json::from_str::<serde_json::Value>(&body) {
        Err(e) => {
            let resp = RpcResponse::err(0, super::error::RpcError::Parse(e.to_string()));
            return json_response(400, &resp);
        }
        Ok(v) => match parse_request(v) {
            Ok(r) => r,
            Err(e) => {
                let resp = RpcResponse::err(0, e);
                return json_response(400, &resp);
            }
        },
    };
    // URL path takes precedence — clients can either put the method in the
    // body OR in the URL, not both, but the URL wins if both are set so
    // Swagger's per-path operations route correctly.
    request.method = method.to_string();

    let (tx, rx) = make_reply_channel();
    let task = RpcTask {
        request,
        reply: tx,
        cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    if let Err(e) = inbox.enqueue(task) {
        warn!("deskpet: http rpc enqueue failed: {e}");
        return server_error("rpc inbox closed");
    }
    let response = match rx.recv() {
        Ok(r) => r,
        Err(e) => {
            warn!("deskpet: http rpc reply wait failed: {e}");
            return server_error("rpc reply channel closed");
        }
    };

    let status = match &response.outcome {
        super::Outcome::Ok { .. } => 200,
        super::Outcome::Err { error } => http_status_for_code(error.code),
    };
    json_response(status, &response)
}

fn http_status_for_code(code: i32) -> u16 {
    // Map JSON-RPC error codes to HTTP status codes. Mostly 400 (bad
    // request), with parse/invalid getting 400, internal 500.
    match code {
        -32700 | -32600 | -32601 | -32602 => 400,
        -32603 => 500,
        // App-specific errors: 409 (conflict) for "queue full" / "not
        // applicable", 422 (unprocessable) for "invalid action".
        -32001 => 429, // notification dropped — too many
        -32002 => 409, // action not applicable
        -32003 => 422, // invalid action
        // Generic server errors
        _ if code >= -32099 && code <= -32000 => 500,
        _ => 500,
    }
}

fn json_response(
    status: u16,
    body: &RpcResponse,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    Response::from_string(
        serde_json::to_string(body).unwrap_or_else(|_| "{}".into()),
    )
    .with_status_code(StatusCode(status))
    .with_header(header)
    .with_header(cors_header())
}

fn bad_request(msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({ "error": msg }).to_string();
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    Response::from_string(body)
        .with_status_code(StatusCode(400))
        .with_header(header)
        .with_header(cors_header())
}

fn not_found(path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({ "error": "not found", "path": path }).to_string();
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    Response::from_string(body)
        .with_status_code(StatusCode(404))
        .with_header(header)
        .with_header(cors_header())
}

fn server_error(msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({ "error": msg }).to_string();
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    Response::from_string(body)
        .with_status_code(StatusCode(500))
        .with_header(header)
        .with_header(cors_header())
}

fn cors_header() -> Header {
    Header::from_bytes(
        &b"Access-Control-Allow-Origin"[..],
        &b"*"[..],
    )
    .expect("static header")
}