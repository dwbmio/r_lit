use crate::error::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub const DEFAULT_KIND: &str = "wechat_miniprogram";

#[derive(Debug, Clone, Default)]
pub struct CommonMeta {
    pub kind: Option<String>,
    pub project_path: Option<String>,
    pub appid: Option<String>,
    pub page: Option<String>,
    pub session: Option<String>,
    pub trace_id: Option<String>,
}

pub fn parse_cli_meta(entries: &[String]) -> Result<BTreeMap<String, Value>> {
    let mut meta = BTreeMap::new();
    for entry in entries {
        let Some((key, value)) = entry.split_once('=') else {
            return Err(Error::InvalidMeta(entry.clone()));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(Error::InvalidMeta(entry.clone()));
        }
        meta.insert(normalize_key(key), Value::String(value.trim().to_string()));
    }
    Ok(meta)
}

pub fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace('-', "_")
}

pub fn normalize_map(input: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    input
        .iter()
        .map(|(k, v)| (normalize_key(k), v.clone()))
        .collect()
}

pub fn extract_common(kind: Option<&str>, meta: &BTreeMap<String, Value>) -> CommonMeta {
    let meta = normalize_map(meta);
    CommonMeta {
        kind: kind
            .map(ToOwned::to_owned)
            .or_else(|| string_value(&meta, "kind"))
            .or_else(|| Some(DEFAULT_KIND.to_string())),
        project_path: string_value(&meta, "project_path"),
        appid: string_value(&meta, "appid"),
        page: string_value(&meta, "page"),
        session: string_value(&meta, "session"),
        trace_id: string_value(&meta, "trace_id"),
    }
}

fn string_value(meta: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    match meta.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(v) if !v.is_null() => Some(v.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_meta_and_normalizes_keys() {
        let meta = parse_cli_meta(&[
            "AppID=wx123".to_string(),
            "base-lib-version=3.0.0".to_string(),
        ])
        .unwrap();

        assert_eq!(meta.get("appid").unwrap(), "wx123");
        assert_eq!(meta.get("base_lib_version").unwrap(), "3.0.0");
    }
}
