//! RPC introspection methods. The "rpc" namespace is for protocol-level
//! controls, distinct from domain methods (notification/*, pet/*, help/*).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::rpc::bevy_bridge::RpcTaskInbox;
use crate::rpc::{Method, RpcError};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CancelParams {
    /// Request id to cancel. Must match the id of a currently in-flight
    /// request (one that hasn't been dispatched yet).
    pub id: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CancelResult {
    /// True if the request was found and flagged for cancellation.
    pub cancelled: bool,
    /// Echoed from request.
    pub id: u64,
    /// When `cancelled=false`, explains why: "not_in_flight" (already
    /// dispatched / completed / never existed) or "cancelled" (already
    /// cancelled before). Absent when cancelled=true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub struct CancelMethod;

impl Method for CancelMethod {
    fn name(&self) -> &'static str {
        "rpc/cancel"
    }
    fn description(&self) -> &'static str {
        "Cancel an in-flight RPC request by id. Returns `cancelled=true` if \
         the request was found and flagged. If the request already \
         completed or never existed, returns `cancelled=false` with reason."
    }
    fn invoke(&self, world: &mut World, params: Value) -> Result<Value, RpcError> {
        let p: CancelParams = serde_json::from_value(params)
            .map_err(|e| RpcError::InvalidParams(e.to_string()))?;

        let Some(inbox) = world.get_resource::<RpcTaskInbox>() else {
            return Err(RpcError::Internal("RpcTaskInbox resource missing".into()));
        };

        let cancelled = inbox.cancel(p.id);
        let result = if cancelled {
            CancelResult {
                cancelled: true,
                id: p.id,
                reason: None,
            }
        } else {
            CancelResult {
                cancelled: false,
                id: p.id,
                reason: Some("not_in_flight".into()),
            }
        };
        Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
    }
}

#[utoipa::path(
    post,
    path = "/m/rpc/cancel",
    tag = "rpc",
    request_body = CancelParams,
    responses(
        (status = 200, description = "Cancel attempted", body = CancelResult),
        (status = 400, description = "Invalid params"),
    ),
)]
#[allow(dead_code)]
pub fn cancel_path(_params: CancelParams) -> CancelResult {
    unimplemented!("openapi stub")
}