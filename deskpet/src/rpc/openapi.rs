//! OpenAPI 3.0 schema generation. The actual `ApiDoc` struct (with
//! `#[derive(OpenApi)]` listing all paths) lives in `rpc_methods` because
//! each method's `#[utoipa::path]` annotation has to be in scope for the
//! derive macro. This module just exposes a `build_openapi_json()` helper
//! that the HTTP handler calls.
//!
//! Each method's params + result type is annotated with
//! `#[derive(Deserialize, Serialize, utoipa::ToSchema)]` and the method
//! itself with `#[utoipa::path(method = "post", path = "/m/<name>", ...)]`.
//! The OpenAPI doc then re-exports those under the unified `/m/*` prefix
//! the HTTP server exposes, so Swagger UI shows one operation per method.

use utoipa::OpenApi;

use crate::rpc_methods::ApiDoc;

/// Generate the OpenAPI 3.0 JSON. The result is served at `/openapi.json`
/// and consumed by Swagger UI at `/`.
pub fn build_openapi_json() -> Result<String, serde_json::Error> {
    ApiDoc::openapi().to_json()
}