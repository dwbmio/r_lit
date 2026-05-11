use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartRunRequest {
    pub tag: Option<String>,
    pub source: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    pub client_id: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunResponse {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppendLine {
    pub ts: Option<String>,
    pub stream: Option<String>,
    pub level: Option<String>,
    pub event: Option<String>,
    pub text: String,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinishRunRequest {
    pub status: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub status: &'static str,
    pub error: String,
}
