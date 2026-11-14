//! 审计：每次发信 / 每次完整性异常都追加一行 JSONL。

use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;
use std::path::Path;

pub fn append(path: &Path, row: serde_json::Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let line = format!("{}\n", serde_json::to_string(&row)?);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("审计日志写入失败：{}", path.display()))?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

pub fn now() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let fmt = time::format_description::well_known::Rfc3339;
    now.format(&fmt).unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

pub fn sent(path: &Path, account: &str, from: &str, to: &str, cc: &str, subject: &str,
            message_id: &str, in_reply_to: &str, smtp_host: &str) -> Result<()> {
    append(
        path,
        json!({
            "ts": now(),
            "kind": "sent",
            "account": account,
            "from": from,
            "to": to,
            "cc": cc,
            "subject": subject,
            "message_id": message_id,
            "in_reply_to": in_reply_to,
            "smtp_host": smtp_host,
        }),
    )
}

pub fn selftest(path: &Path, account: &str, from: &str, to: &str, subject: &str,
                message_id: &str, smtp_host: &str, delivered: bool) -> Result<()> {
    append(
        path,
        json!({
            "ts": now(),
            "kind": "selftest",
            "account": account,
            "from": from,
            "to": to,
            "subject": subject,
            "message_id": message_id,
            "smtp_host": smtp_host,
            "delivered_back": delivered,
        }),
    )
}

pub fn integrity(path: &Path, reason: &str) -> Result<()> {
    append(path, json!({"ts": now(), "kind": "integrity_failure", "reason": reason}))
}
