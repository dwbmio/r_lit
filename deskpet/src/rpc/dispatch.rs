//! Method trait + registry + dispatch. The registry is a flat map from
//! method name to `Arc<dyn Method>`. `dispatch_request` is the single
//! entry point both the NDJSON listener and the HTTP handler call into.
//!
//! Methods own their params/result types (typed structs with utoipa
//! schemas). The wire-level `Request::params` is `serde_json::Value` and
//! gets deserialized inside the method via `serde_json::from_value`.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::{Resource, World};
use serde_json::Value;

use super::{Request, Response, RpcError};

/// RPC method contract. Implementors must be thread-safe (`Send + Sync`)
/// and live for `'static` so the registry can hand out `Arc<dyn Method>`.
pub trait Method: Send + Sync + 'static {
    /// Method name as called on the wire, e.g. `"notification/show"`.
    fn name(&self) -> &'static str;

    /// Human-readable description for `help/methods` and the OpenAPI spec.
    fn description(&self) -> &'static str;

    /// Protocol version this method was introduced in. Bumped when the
    /// method signature changes in a backward-incompatible way.
    fn since(&self) -> u32 {
        1
    }

    /// Invoke the method. `world` is the Bevy `World`, giving methods
    /// direct read/write access to Resources and Entities.
    ///
    /// Implementations should:
    /// 1. Deserialize `params` into a typed params struct (return
    ///    `RpcError::InvalidParams` on failure).
    /// 2. Do the work, returning any domain error wrapped in
    ///    `RpcError::Server` (with a code in `-32000..-32099`).
    /// 3. Serialize the result to `serde_json::Value` and return it.
    fn invoke(&self, world: &mut World, params: Value) -> Result<Value, RpcError>;
}

/// Flat method registry. Lookups are O(1). Iteration is sorted by method
/// name so `help/methods` and the OpenAPI spec are deterministic.
#[derive(Default, Clone, Resource)]
pub struct MethodRegistry {
    inner: Arc<BTreeMap<String, Arc<dyn Method>>>,
}

impl MethodRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a method. Overwrites any existing entry with the same name.
    pub fn insert(&mut self, method: Arc<dyn Method>) {
        let name = method.name().to_string();
        let map = Arc::make_mut(&mut self.inner);
        map.insert(name, method);
    }

    /// Look up a method by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Method>> {
        self.inner.get(name).cloned()
    }

    /// Iterate methods in sorted order. Used by `help/methods`.
    pub fn iter(&self) -> impl Iterator<Item = Arc<dyn Method>> + '_ {
        self.inner.values().cloned()
    }

    /// Total registered methods.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl std::fmt::Debug for MethodRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MethodRegistry")
            .field("methods", &self.inner.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Run a single request through the registry. Always returns a `Response`
/// (errors are turned into wire-level error responses, never propagated).
pub fn dispatch_request(registry: &MethodRegistry, world: &mut World, req: Request) -> Response {
    let Some(method) = registry.get(&req.method) else {
        // Attach the unknown method name in `data` so clients can
        // distinguish "typo" from "deprecated and removed".
        return Response::err(
            req.id,
            RpcError::Server {
                code: RpcError::MethodNotFound(String::new()).code(),
                message: format!("method not found: {}", req.method),
                source: None,
                data: Some(serde_json::json!({ "method": req.method })),
            },
        );
    };

    match method.invoke(world, req.params) {
        Ok(result) => Response::ok(req.id, result)
            .unwrap_or_else(|e| Response::err(req.id, e)),
        Err(e) => Response::err(req.id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct EchoMethod;
    impl Method for EchoMethod {
        fn name(&self) -> &'static str {
            "test/echo"
        }
        fn description(&self) -> &'static str {
            "returns its params unchanged (test fixture)"
        }
        fn invoke(
            &self,
            _world: &mut World,
            params: Value,
        ) -> Result<Value, RpcError> {
            Ok(params)
        }
    }

    struct FailingMethod;
    impl Method for FailingMethod {
        fn name(&self) -> &'static str {
            "test/fail"
        }
        fn description(&self) -> &'static str {
            "always errors"
        }
        fn invoke(
            &self,
            _world: &mut World,
            _params: Value,
        ) -> Result<Value, RpcError> {
            Err(RpcError::Internal("nope".into()))
        }
    }

    #[test]
    fn registry_insert_and_get() {
        let mut reg = MethodRegistry::new();
        reg.insert(Arc::new(EchoMethod));
        assert!(reg.get("test/echo").is_some());
        assert!(reg.get("test/missing").is_none());
    }

    #[test]
    fn registry_iter_is_sorted() {
        let mut reg = MethodRegistry::new();
        reg.insert(Arc::new(FailingMethod));
        reg.insert(Arc::new(EchoMethod));
        let names: Vec<&str> = reg.iter().map(|m| m.name()).collect();
        assert_eq!(names, vec!["test/echo", "test/fail"]);
    }

    #[test]
    fn dispatch_returns_result() {
        let mut reg = MethodRegistry::new();
        reg.insert(Arc::new(EchoMethod));
        // Use a stub World just to satisfy the signature.
        let mut world = World::new();
        let req: Request = serde_json::from_value(json!({
            "id": 7,
            "method": "test/echo",
            "params": { "hello": "world" }
        }))
        .unwrap();
        let resp = dispatch_request(&reg, &mut world, req);
        match resp.outcome {
            super::super::Outcome::Ok { result } => {
                assert_eq!(result, json!({ "hello": "world" }));
            }
            _ => panic!("expected ok"),
        }
        assert_eq!(resp.id, 7);
    }

    #[test]
    fn dispatch_unknown_method_returns_error() {
        let reg = MethodRegistry::new();
        let mut world = World::new();
        let req: Request = serde_json::from_value(json!({
            "id": 9,
            "method": "does/not/exist",
            "params": {}
        }))
        .unwrap();
        let resp = dispatch_request(&reg, &mut world, req);
        match resp.outcome {
            super::super::Outcome::Err { error } => {
                assert_eq!(error.code, -32601);
                assert_eq!(error.data.unwrap(), json!({ "method": "does/not/exist" }));
            }
            _ => panic!("expected err"),
        }
    }

    #[test]
    fn dispatch_method_error_passthrough() {
        let mut reg = MethodRegistry::new();
        reg.insert(Arc::new(FailingMethod));
        let mut world = World::new();
        let req: Request = serde_json::from_value(json!({
            "id": 11,
            "method": "test/fail",
            "params": {}
        }))
        .unwrap();
        let resp = dispatch_request(&reg, &mut world, req);
        match resp.outcome {
            super::super::Outcome::Err { error } => {
                assert_eq!(error.code, -32603); // Internal
                assert!(error.message.contains("nope"));
            }
            _ => panic!("expected err"),
        }
    }
}