use crate::config::VlogsConfig;
use anyhow::{Context, Result, anyhow};
use distill_crypto::DlpGate;
use distill_proto::{LogEntry, LogsQueryResult};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Clone)]
pub struct VictoriaLogsClient {
    cfg: Arc<RwLock<VlogsConfig>>,
    http: reqwest::Client,
}

impl VictoriaLogsClient {
    pub fn new(cfg: VlogsConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .context("failed to build reqwest client for VictoriaLogs")?;
        Ok(Self {
            cfg: Arc::new(RwLock::new(cfg)),
            http,
        })
    }

    pub fn update_config(&self, next: VlogsConfig) {
        if let Ok(mut guard) = self.cfg.write() {
            *guard = next;
        }
    }

    fn current_cfg(&self) -> VlogsConfig {
        self.cfg
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn is_enabled(&self) -> bool {
        let cfg = self.current_cfg();
        cfg.enabled && !cfg.endpoint.trim().is_empty()
    }

    pub fn endpoint(&self) -> Option<String> {
        let cfg = self.current_cfg();
        (cfg.enabled && !cfg.endpoint.trim().is_empty())
            .then(|| cfg.endpoint.trim_end_matches('/').to_string())
    }

    pub fn default_range(&self) -> String {
        self.current_cfg().default_range
    }

    /// Execute a LogsQL query against VictoriaLogs `/select/logsql/query` and return
    /// secret-redacted structured `LogEntry` rows.
    pub async fn query(
        &self,
        raw_query: &str,
        range: Option<&str>,
        start: Option<&str>,
        end: Option<&str>,
        limit: Option<usize>,
        repo_default_filter: Option<&str>,
    ) -> Result<LogsQueryResult> {
        let cfg = self.current_cfg();
        if !cfg.enabled || cfg.endpoint.trim().is_empty() {
            return Err(anyhow!("服务端未启用 VictoriaLogs 查询（[vlogs].enabled = false）"));
        }

        let eff_range = range
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&cfg.default_range)
            .to_string();
        let cap = limit.unwrap_or(cfg.default_limit).clamp(1, 200);

        let effective_query = build_effective_logsql(
            raw_query,
            &eff_range,
            start,
            end,
            cap,
            repo_default_filter,
        );

        let url = format!(
            "{}/select/logsql/query",
            cfg.endpoint.trim_end_matches('/')
        );

        let mut req = self
            .http
            .get(&url)
            .query(&[("query", effective_query.as_str())]);
        if let Some(ref tok) = cfg.bearer_token
            && !tok.trim().is_empty()
        {
            req = req.bearer_auth(tok.trim());
        }

        let resp = req
            .send()
            .await
            .with_context(|| format!("请求 VictoriaLogs 失败 ({url})"))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let snippet: String = body.chars().take(240).collect();
            return Err(anyhow!(
                "VictoriaLogs 返回 HTTP {}: {}",
                status.as_u16(),
                snippet.trim()
            ));
        }

        let mut entries = Vec::new();
        let mut dlp_redactions = 0usize;
        let mut truncated = false;

        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if entries.len() >= cap {
                truncated = true;
                break;
            }
            let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            let Some(obj) = val.as_object() else {
                continue;
            };

            let get_str = |k: &str| -> String {
                obj.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };

            let raw_msg = get_str("_msg");
            let (safe_msg, r) = DlpGate::sanitize_secrets(&raw_msg);
            dlp_redactions += r;
            let msg_capped: String = safe_msg.chars().take(600).collect();

            let mut extra = BTreeMap::new();
            for (k, v) in obj {
                if matches!(
                    k.as_str(),
                    "_msg"
                        | "_time"
                        | "_stream"
                        | "_stream_id"
                        | "cluster"
                        | "namespace"
                        | "container"
                        | "pod"
                        | "stream"
                        | "filename"
                        | "host"
                ) {
                    continue;
                }
                if let Some(s) = v.as_str()
                    && !s.is_empty()
                {
                    let (clean, rx) = DlpGate::sanitize_secrets(s);
                    dlp_redactions += rx;
                    extra.insert(k.clone(), clean.chars().take(160).collect());
                }
            }

            entries.push(LogEntry {
                timestamp: get_str("_time"),
                cluster: get_str("cluster"),
                namespace: get_str("namespace"),
                container: get_str("container"),
                pod: get_str("pod"),
                stream: get_str("stream"),
                message: msg_capped,
                extra,
            });
        }

        let note = if entries.is_empty() {
            format!("时间窗 {eff_range} 内未命中日志（查询语句：{effective_query}）")
        } else {
            format!("命中 {} 条日志（时间窗 {eff_range}）", entries.len())
        };

        Ok(LogsQueryResult {
            endpoint: cfg.endpoint.trim_end_matches('/').to_string(),
            effective_query,
            range: eff_range,
            entries,
            truncated,
            dlp_redactions,
            note,
        })
    }
}

fn build_effective_logsql(
    raw_query: &str,
    range: &str,
    start: Option<&str>,
    end: Option<&str>,
    limit: usize,
    repo_default_filter: Option<&str>,
) -> String {
    let q = raw_query.trim();
    let has_time = q.contains("_time:");
    let has_pipe = q.contains('|');

    let time_clause = if has_time {
        String::new()
    } else if let (Some(s), Some(e)) = (
        start.map(str::trim).filter(|v| !v.is_empty()),
        end.map(str::trim).filter(|v| !v.is_empty()),
    ) {
        format!("_time:[{s}, {e}]")
    } else {
        format!("_time:{range}")
    };

    let has_explicit_scope = q.contains("container:")
        || q.contains("namespace:")
        || q.contains("cluster:")
        || q.contains("pod:")
        || q.contains("_stream:");

    let scope_clause = if !has_explicit_scope
        && let Some(df) = repo_default_filter.map(str::trim).filter(|s| !s.is_empty())
    {
        format!("({df})")
    } else {
        String::new()
    };

    let mut base_parts = Vec::new();
    if !time_clause.is_empty() {
        base_parts.push(time_clause);
    }
    if !scope_clause.is_empty() {
        base_parts.push(scope_clause);
    }
    if !q.is_empty() {
        base_parts.push(q.to_string());
    }
    let combined = base_parts.join(" ");

    if has_pipe && combined.to_ascii_lowercase().contains("limit") {
        combined
    } else if has_pipe {
        format!("{combined} | limit {limit}")
    } else {
        format!("{combined} | sort by (_time) desc | limit {limit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_logsql_with_time_scope_and_limit() {
        let q = build_effective_logsql(
            "error OR 500",
            "15m",
            None,
            None,
            40,
            Some("container:twosite-api OR container:rigel"),
        );
        assert!(q.starts_with("_time:15m (container:twosite-api OR container:rigel) error OR 500"));
        assert!(q.ends_with("| sort by (_time) desc | limit 40"));
    }

    #[test]
    fn preserves_explicit_container_and_time() {
        let q = build_effective_logsql(
            "_time:5m container:gql timeout | limit 10",
            "15m",
            None,
            None,
            40,
            Some("container:twosite-api"),
        );
        assert_eq!(q, "_time:5m container:gql timeout | limit 10");
    }
}
