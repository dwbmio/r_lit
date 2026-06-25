//! RPC method implementations. Each method declares typed params + result
//! structs with `utoipa::ToSchema` so they show up in the OpenAPI spec,
//! and a `Method` impl that wires the wire-level `Value` to Bevy state.
//!
//! Add a new method by:
//! 1. Adding a params struct + a result struct in this module's sub-file
//!    (`notification.rs`, `pet.rs`, etc.) with the right derives.
//! 2. Annotating the params/result with `#[derive(utoipa::ToSchema)]`.
//! 3. Annotating the `Method` impl method body with `#[utoipa::path(...)]`.
//! 4. Registering the method in `register_all()` below — it goes into the
//!    runtime `MethodRegistry` AND the OpenAPI doc automatically (the
//!    `ApiDoc` derive re-exports every `#[utoipa::path]` in scope).

pub mod help;
pub mod notification;
pub mod pet;
pub mod rpc;

use std::sync::Arc;

use bevy::prelude::World;
use serde_json::Value;
use utoipa::OpenApi;

use crate::rpc::{Method, MethodRegistry, RpcError};

/// All RPC methods, registered. Called once at app startup to build the
/// `MethodRegistry` resource.
pub fn register_all() -> MethodRegistry {
    let mut reg = MethodRegistry::new();
    reg.insert(Arc::new(notification::ShowMethod));
    reg.insert(Arc::new(notification::ClearMethod));
    reg.insert(Arc::new(notification::ListMethod));
    reg.insert(Arc::new(pet::StateMethod));
    reg.insert(Arc::new(pet::ControlMethod));
    reg.insert(Arc::new(help::MethodsMethod));
    reg.insert(Arc::new(rpc::CancelMethod));
    reg
}

/// OpenAPI doc aggregating every `#[utoipa::path]` annotation in this
/// module. Served at `/openapi.json` and consumed by Swagger UI.
#[derive(OpenApi)]
#[openapi(
    paths(
        notification::show_path,
        notification::clear_path,
        notification::list_path,
        pet::state_path,
        pet::control_path,
        help::methods_path,
        rpc::cancel_path,
    ),
    components(schemas(
        notification::ShowParams,
        notification::ShowResult,
        notification::ClearResult,
        notification::ListResult,
        pet::StateResult,
        pet::ControlParams,
        pet::ControlResult,
        help::MethodsResult,
        rpc::CancelParams,
        rpc::CancelResult,
    )),
    info(
        title = "deskpet RPC",
        version = "1.0.0",
        description = "Remote control surface for the deskpet desktop mascot. \
                       All methods are JSON-RPC 2.0-inspired over NDJSON (port 47800) \
                       or HTTP (port 47801). Methods marked *stateful* read or mutate \
                       the running Bevy world; their effects are visible on the next \
                       frame.",
    ),
)]
pub struct ApiDoc;

/// Helper used by every `#[utoipa::path]`-annotated method to satisfy the
/// derive macro's signature requirement (it wants an actual `fn` pointer
/// it can attach docs to). The real implementation lives in the
/// `Method::invoke` impl; this stub is purely for OpenAPI generation.
#[allow(dead_code)]
fn _openapi_stub(_world: &mut World, _params: Value) -> Result<Value, RpcError> {
    unreachable!("openapi_stub is only here to satisfy utoipa::path")
}

/// Apply `#[allow(dead_code)]` to every `#[utoipa::path]` stub. Used as an
/// attribute on the `ApiDoc` paths list — see module-level usage below.
#[allow(dead_code)]
struct AllowDeadCode;