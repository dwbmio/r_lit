//! `help/methods` — runtime method discovery. Returns the catalog of
//! registered methods (name + description + since-version) so clients
//! can probe capability without reading source.

use bevy::prelude::*;
use serde::Serialize;
use utoipa::ToSchema;

use crate::rpc::{Method, MethodRegistry, RpcError};
use serde_json::Value;

#[derive(Debug, Serialize, ToSchema)]
pub struct MethodEntry {
    pub name: String,
    pub description: String,
    /// Protocol version this method was introduced in.
    pub since: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MethodsResult {
    /// Bumped when the protocol changes in a backward-incompatible way.
    /// Clients SHOULD refuse to operate against an unknown protocol_version.
    pub protocol_version: u32,
    /// Build-time string for diagnostics.
    pub build_version: &'static str,
    pub methods: Vec<MethodEntry>,
}

/// Current protocol version. Bump when:
/// - Method signatures change (params/result shape)
/// - Methods are removed
/// - Error codes are reassigned
/// Adding a new method does NOT require a bump (it's a backward-compat addition).
pub const PROTOCOL_VERSION: u32 = 1;

/// Build version string from `CARGO_PKG_VERSION` at compile time.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct MethodsMethod;

impl Method for MethodsMethod {
    fn name(&self) -> &'static str {
        "help/methods"
    }
    fn description(&self) -> &'static str {
        "List all registered RPC methods. First call after connecting \
         SHOULD be this — it returns the protocol version + method catalog."
    }
    fn invoke(&self, world: &mut World, _params: Value) -> Result<Value, RpcError> {
        let registry = world
            .get_resource::<MethodRegistry>()
            .ok_or_else(|| RpcError::Internal("MethodRegistry resource missing".into()))?;
        let methods = registry
            .iter()
            .map(|m| MethodEntry {
                name: m.name().to_string(),
                description: m.description().to_string(),
                since: m.since(),
            })
            .collect();
        Ok(serde_json::to_value(MethodsResult {
            protocol_version: PROTOCOL_VERSION,
            build_version: BUILD_VERSION,
            methods,
        })
        .map_err(|e| RpcError::Internal(e.to_string()))?)
    }
}

#[utoipa::path(
    post,
    path = "/m/help/methods",
    tag = "help",
    responses(
        (status = 200, description = "Method catalog", body = MethodsResult),
    ),
)]
#[allow(dead_code)]
pub fn methods_path() -> MethodsResult {
    unimplemented!("openapi stub")
}