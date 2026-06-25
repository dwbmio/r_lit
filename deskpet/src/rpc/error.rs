use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::ErrorData;

/// JSON-RPC 2.0 standard error codes + the app-specific reserved range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Invalid JSON was received by the server.
    ParseError,
    /// The JSON sent is not a valid Request object.
    InvalidRequest,
    /// The method does not exist or is not available.
    MethodNotFound,
    /// Invalid method parameter(s).
    InvalidParams,
    /// Internal JSON-RPC error.
    InternalError,
    /// App-specific error in the reserved server-error range
    /// (`-32000` to `-32099`).
    ServerError(i32),
}

impl ErrorCode {
    pub fn code(self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
            Self::InternalError => -32603,
            Self::ServerError(c) => c,
        }
    }
}

/// RPC-layer error. All variants are caller-visible (turn into wire-level
/// `error.code`/`error.message`). The Bevy-side errors that happen INSIDE
/// a method should be wrapped as `RpcError::Internal` rather than leaked.
#[derive(Debug, Error)]
pub enum RpcError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("method not found: {0}")]
    MethodNotFound(String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("internal error: {0}")]
    Internal(String),

    /// Server-initiated application error in the reserved range.
    /// `data` is forwarded to the client verbatim (must be JSON-serializable).
    #[error("{code} {message}")]
    Server {
        code: i32,
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        data: Option<Value>,
    },
}

impl RpcError {
    pub fn code(&self) -> i32 {
        match self {
            Self::Parse(_) => ErrorCode::ParseError.code(),
            Self::InvalidRequest(_) => ErrorCode::InvalidRequest.code(),
            Self::MethodNotFound(_) => ErrorCode::MethodNotFound.code(),
            Self::InvalidParams(_) => ErrorCode::InvalidParams.code(),
            Self::Internal(_) => ErrorCode::InternalError.code(),
            Self::Server { code, .. } => *code,
        }
    }

    /// Convert into wire-level `ErrorData`. Drops the source chain (not
    /// JSON-serializable) but keeps `data` for the `Server` variant.
    pub fn into_data(self) -> ErrorData {
        let code = self.code();
        let message = self.to_string();
        let data = match self {
            Self::Server { data, .. } => data,
            _ => None,
        };
        ErrorData {
            code,
            message,
            data,
        }
    }
}

impl From<&str> for RpcError {
    fn from(s: &str) -> Self {
        Self::Internal(s.to_string())
    }
}

// `Serialize`/`Deserialize` impls so we can use `RpcError` directly in
// `serde_json::Value` paths (e.g., the `data` field of a server error).
impl Serialize for RpcError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for RpcError {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self::Internal(s))
    }
}

/// Application-specific error codes in the reserved server-error range.
/// Add new variants here when a method needs to surface a domain failure
/// that callers should distinguish from generic `Internal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppError {
    /// `notification/show` was called while the bubble is busy AND the
    /// queue is full — caller should retry or surface to user.
    NotificationDropped,
    /// `pet/control` action is not supported in the current pet state
    /// (e.g., walk_to while dragging).
    ActionNotApplicable,
    /// `pet/control` action would put the pet into an invalid state
    /// (e.g., walk_speed out of range).
    InvalidAction,
}

impl AppError {
    pub fn code(self) -> i32 {
        match self {
            Self::NotificationDropped => -32001,
            Self::ActionNotApplicable => -32002,
            Self::InvalidAction => -32003,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::NotificationDropped => "notification dropped: queue full",
            Self::ActionNotApplicable => "action not applicable in current state",
            Self::InvalidAction => "invalid action parameters",
        }
    }
}