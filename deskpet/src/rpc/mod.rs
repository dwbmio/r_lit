//! deskpet RPC: NDJSON + JSON-RPC 2.0-inspired protocol over loopback TCP,
//! plus an HTTP surface (tiny_http) for Swagger UI and a `POST /m/<method>`
//! wrapper so third-party tools can hit deskpet without speaking NDJSON.
//!
//! # Wire format
//!
//! Request (NDJSON line or HTTP body):
//! ```json
//! {"id": 1, "method": "notification/show", "params": {"title": "Build", "body": "3 errors"}}
//! ```
//!
//! Response (NDJSON line or HTTP body):
//! ```json
//! {"id": 1, "result": {"shown": true}}
//! {"id": 1, "error": {"code": -32601, "message": "method not found", "data": {"method": "foo"}}}
//! ```
//!
//! # Architectural invariants
//!
//! - **Listener threads do not touch Bevy state.** They parse + validate,
//!   then enqueue an `RpcTask` into a shared inbox. A Bevy system drains
//!   the inbox each frame, invokes the method with `World` access, and
//!   sends the reply back via a per-task oneshot. This keeps the ECS as
//!   the single source of truth and avoids `Mutex<World>` deadlocks.
//! - **Method implementations own their types.** Each method declares a
//!   typed params struct with `#[derive(Deserialize, utoipa::ToSchema)]` and
//!   a typed result struct with `#[derive(Serialize, utoipa::ToSchema)]`.
//!   The wire-level `Request::params` is `serde_json::Value` and gets
//!   deserialized into the typed struct inside the method.
//! - **Errors are centralized.** `RpcError` maps to JSON-RPC standard codes
//!   + an app-specific range (`-32000` to `-32099`) for things like
//!   "notification suppressed because bubble busy".

pub mod bevy_bridge;
pub mod cli;
pub mod dispatch;
pub mod error;
pub mod http;
pub mod openapi;
pub mod server;
pub mod swagger_html;

pub use dispatch::{Method, MethodRegistry, dispatch_request};
pub use error::{AppError, ErrorCode, RpcError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0-inspired request. `id` is always `u64` (we don't support
/// string or null ids — those exist in the spec for reasons that don't
/// apply to a loopback control surface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    /// `method` is optional at parse time: HTTP callers put the method in
    /// the URL path (`POST /m/<method>`) and send only `{id, params}` in
    /// the body. The HTTP server fills `method` from the path before
    /// dispatch. NDJSON callers always include it explicitly.
    #[serde(default)]
    pub method: String,
    /// Method params. Each method's typed params struct is deserialized
    /// from this; unknown fields are tolerated (forward-compat with new
    /// sender-side fields).
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC 2.0-inspired response. Exactly one of `result` or `error` is set
/// — encoded as a flattened object so wire shape stays `{id, result, error}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Ok { result: Value },
    Err { error: ErrorData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorData {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Response {
    /// Build a successful response from any `Serialize` value.
    pub fn ok<T: Serialize>(id: u64, result: T) -> Result<Self, RpcError> {
        let result = serde_json::to_value(result)
            .map_err(|e| RpcError::Internal(format!("response serialize: {e}")))?;
        Ok(Self {
            id,
            outcome: Outcome::Ok { result },
        })
    }

    /// Build an error response from any `RpcError`.
    pub fn err(id: u64, err: RpcError) -> Self {
        Self {
            id,
            outcome: Outcome::Err {
                error: err.into_data(),
            },
        }
    }
}

/// Parse a JSON-RPC request from a raw JSON value. Distinguishes parse
/// failures (`-32700`) from invalid-shape failures (`-32600`) per spec.
pub fn parse_request(value: Value) -> Result<Request, RpcError> {
    // serde_json::from_value::<Request> already reports shape errors as
    // "invalid request" via the missing-field message. The two cases we
    // actually want to distinguish are "couldn't even parse JSON" (which
    // happens at the caller, before we get a Value) and "JSON parsed but
    // shape is wrong" (here). We surface the latter as InvalidRequest.
    serde_json::from_value::<Request>(value).map_err(|e| RpcError::InvalidRequest(e.to_string()))
}