//! Log sources for `looplog tail`.
//!
//! A source knows two things:
//! 1. how to build the child command that streams lines (`build`)
//! 2. how to turn one raw line into a structured record (`parse_line`)
//!
//! Parsing is where the real value is: journald gives us microsecond timestamps
//! and syslog priorities, nginx gives us the request time and upstream timing.
//! Using those instead of the local receive time is what makes two streams line
//! up on one timeline.

use crate::error::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

const NGINX_TIME: &[FormatItem] =
    format_description!("[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]");

#[derive(Debug, Clone)]
pub enum SourceSpec {
    /// Arbitrary streaming command (ssh + journalctl / tail -F / a script).
    Cmd { command: Vec<String> },
    /// Android logcat through adb, using epoch timestamps to avoid timezone/year ambiguity.
    Logcat {
        serial: Option<String>,
        lines: usize,
    },
    /// systemd journal, ideally `-o json` so timestamps and priorities survive.
    Journald {
        remote: Option<String>,
        unit: String,
        since: Option<String>,
        lines: usize,
        sudo: bool,
    },
    /// nginx/openresty access log (combined, optionally with request timings).
    Nginx {
        remote: Option<String>,
        path: String,
        lines: usize,
        sudo: bool,
        follow: bool,
    },
    /// Local file, followed with `tail -F`.
    File {
        path: String,
        lines: usize,
        follow: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parser {
    Raw,
    Journald,
    Nginx,
    Logcat,
}

#[derive(Debug, Clone)]
pub struct ParsedLine {
    pub ts_ms: Option<i64>,
    pub level: String,
    pub text: String,
    pub meta: BTreeMap<String, Value>,
}

impl SourceSpec {
    pub fn label(&self) -> String {
        match self {
            SourceSpec::Cmd { command } => format!("cmd:{}", command.join(" ")),
            SourceSpec::Logcat { serial, .. } => {
                format!("logcat:{}", serial.as_deref().unwrap_or("default"))
            }
            SourceSpec::Journald { remote, unit, .. } => match remote {
                Some(host) => format!("journald:{}@{host}", unit),
                None => format!("journald:{unit}"),
            },
            SourceSpec::Nginx { remote, path, .. } => match remote {
                Some(host) => format!("nginx:{path}@{host}"),
                None => format!("nginx:{path}"),
            },
            SourceSpec::File { path, .. } => format!("file:{path}"),
        }
    }

    /// Build the command for a reconnect.
    ///
    /// journald is the only source that can resume precisely: we pass
    /// `--since @<epoch>.<usec>`, one microsecond past the last line we stored,
    /// so a dropped ssh session neither loses nor duplicates lines. The other
    /// sources fall back to their normal prewarm (`--lines N`).
    pub fn build_resumed(
        &self,
        resume_after_ms: Option<i64>,
    ) -> Result<(Vec<String>, Parser)> {
        match (self, resume_after_ms) {
            (
                SourceSpec::Journald {
                    remote, unit, sudo, ..
                },
                Some(ms),
            ) => {
                let argv = vec![
                    "journalctl".to_string(),
                    "-o".to_string(),
                    "json".to_string(),
                    "-f".to_string(),
                    "--unit".to_string(),
                    unit.clone(),
                    "--since".to_string(),
                    journal_since_at(ms),
                ];
                Ok((wrap_remote(argv, remote.as_deref(), *sudo), Parser::Journald))
            }
            _ => self.build(),
        }
    }

    /// Build the child command plus the parser for its stdout.
    pub fn build(&self) -> Result<(Vec<String>, Parser)> {
        match self {
            SourceSpec::Cmd { command } => {
                if command.is_empty() {
                    return Err(Error::MissingCommand);
                }
                Ok((command.clone(), Parser::Raw))
            }
            SourceSpec::Logcat { serial, lines } => {
                let mut argv = vec!["adb".to_string()];
                if let Some(serial) = serial.as_deref().filter(|s| !s.trim().is_empty()) {
                    argv.extend(["-s".to_string(), serial.to_string()]);
                }
                argv.extend([
                    "logcat".to_string(),
                    "-v".to_string(),
                    "epoch".to_string(),
                    "-T".to_string(),
                    lines.to_string(),
                ]);
                Ok((argv, Parser::Logcat))
            }
            SourceSpec::Journald {
                remote,
                unit,
                since,
                lines,
                sudo,
            } => {
                let mut argv = vec!["journalctl".to_string(), "-o".to_string(), "json".to_string(), "-f".to_string()];
                argv.push("--unit".to_string());
                argv.push(unit.clone());
                match since {
                    Some(since) => {
                        argv.push("--since".to_string());
                        argv.push(journal_since(since));
                    }
                    None => {
                        argv.push("--lines".to_string());
                        argv.push(lines.to_string());
                    }
                }
                Ok((wrap_remote(argv, remote.as_deref(), *sudo), Parser::Journald))
            }
            SourceSpec::Nginx {
                remote,
                path,
                lines,
                sudo,
                follow,
            } => {
                let mut argv = vec!["tail".to_string()];
                if *follow {
                    argv.push("-F".to_string());
                }
                argv.push("-n".to_string());
                argv.push(lines.to_string());
                argv.push(path.clone());
                Ok((wrap_remote(argv, remote.as_deref(), *sudo), Parser::Nginx))
            }
            SourceSpec::File { path, lines, follow } => {
                let mut argv = vec!["tail".to_string()];
                if *follow {
                    argv.push("-F".to_string());
                }
                argv.push("-n".to_string());
                argv.push(lines.to_string());
                argv.push(path.clone());
                Ok((argv, Parser::Raw))
            }
        }
    }
}

/// `--remote host` wraps the command in ssh; `--sudo` prepends sudo remotely.
fn wrap_remote(argv: Vec<String>, remote: Option<&str>, sudo: bool) -> Vec<String> {
    let Some(host) = remote else {
        if sudo {
            let mut with_sudo = vec!["sudo".to_string(), "-n".to_string()];
            with_sudo.extend(argv);
            return with_sudo;
        }
        return argv;
    };
    let mut inner = Vec::new();
    if sudo {
        inner.push("sudo".to_string());
        inner.push("-n".to_string());
    }
    inner.extend(argv);
    let script = inner
        .iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        "ssh".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        host.to_string(),
        script,
    ]
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@".contains(c))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `--since @<sec>.<usec>`, nudged one microsecond past the stored millisecond
/// so the boundary line is not re-ingested.
fn journal_since_at(last_ts_ms: i64) -> String {
    let micros = i64::max(last_ts_ms, 0) as i128 * 1000 + 1;
    let secs = micros / 1_000_000;
    let frac = micros % 1_000_000;
    format!("@{secs}.{frac:06}")
}

/// journalctl accepts systemd time specs; turn `15m` into `15 min ago`.
fn journal_since(since: &str) -> String {
    match humantime::parse_duration(since) {
        Ok(d) => format!("{} sec ago", d.as_secs().max(1)),
        Err(_) => since.to_string(),
    }
}

pub fn parse_line(parser: Parser, stream: &str, raw: &str) -> Option<ParsedLine> {
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return None;
    }
    let mut parsed = match parser {
        Parser::Journald if stream == "stdout" => {
            parse_journald(trimmed).unwrap_or_else(|| raw_line(stream, trimmed))
        }
        Parser::Nginx if stream == "stdout" => {
            parse_nginx(trimmed).unwrap_or_else(|| raw_line(stream, trimmed))
        }
        Parser::Logcat if stream == "stdout" => {
            parse_logcat(trimmed).unwrap_or_else(|| raw_line(stream, trimmed))
        }
        _ => raw_line(stream, trimmed),
    };
    // tracing/colorized logs carry ESC sequences into the database otherwise.
    parsed.text = strip_ansi(&parsed.text);
    Some(parsed)
}

/// Remove ANSI CSI escapes (colors, cursor movement) from a log line.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() || n == '@' || n == '~' {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn severity_rank(level: &str) -> u8 {
    match level {
        "error" => 3,
        "warn" => 2,
        "debug" => 0,
        _ => 1,
    }
}

fn raw_line(stream: &str, text: &str) -> ParsedLine {
    ParsedLine {
        ts_ms: None,
        level: infer_level(stream, text).to_string(),
        text: text.to_string(),
        meta: BTreeMap::new(),
    }
}

/// Parse `adb logcat -v epoch` and retain the device's epoch time.
/// Plain `threadtime` lines (with or without a zone) are also accepted for
/// pid/tid/tag/level, but their timestamp is left to ingest time because they
/// have no year. The native source always requests epoch to avoid that ambiguity.
fn parse_logcat(raw: &str) -> Option<ParsedLine> {
    let mut fields = raw.split_whitespace();
    let first = fields.next()?;
    let second = fields.next()?;
    let (ts_ms, pid, tid, priority, tag) = match parse_logcat_epoch_ms(first) {
        Some(ts_ms) => (
            Some(ts_ms),
            second,
            fields.next()?,
            fields.next()?,
            fields.next()?,
        ),
        None => {
            let third = fields.next()?;
            let pid = if is_logcat_zone(third) {
                fields.next()?
            } else {
                third
            };
            (
                None,
                pid,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            )
        }
    };

    let pid = pid.parse::<u64>().ok()?;
    let tid = tid.parse::<u64>().ok()?;
    let level = logcat_level(priority)?;
    let tag = tag.strip_suffix(':')?;
    let mut meta = BTreeMap::new();
    meta.insert("pid".to_string(), Value::from(pid));
    meta.insert("tid".to_string(), Value::from(tid));
    meta.insert("tag".to_string(), Value::from(tag));
    meta.insert("priority".to_string(), Value::from(priority));

    Some(ParsedLine {
        ts_ms,
        level: level.to_string(),
        text: raw.to_string(),
        meta,
    })
}

fn parse_logcat_epoch_ms(raw: &str) -> Option<i64> {
    let (seconds, fraction) = match raw.split_once('.') {
        Some(parts) => parts,
        None => (raw, ""),
    };
    if seconds.is_empty()
        || !seconds.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut millis = fraction.chars().take(3).collect::<String>();
    while millis.len() < 3 {
        millis.push('0');
    }
    seconds
        .parse::<i64>()
        .ok()?
        .checked_mul(1000)?
        .checked_add(millis.parse::<i64>().ok()?)
}

fn is_logcat_zone(raw: &str) -> bool {
    raw.len() == 5
        && matches!(raw.as_bytes()[0], b'+' | b'-')
        && raw.as_bytes()[1..].iter().all(u8::is_ascii_digit)
}

fn logcat_level(priority: &str) -> Option<&'static str> {
    match priority {
        "V" | "D" | "S" => Some("debug"),
        "I" => Some("info"),
        "W" => Some("warn"),
        "E" | "F" | "A" => Some("error"),
        _ => None,
    }
}

fn parse_journald(raw: &str) -> Option<ParsedLine> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let text = journal_message(&value)?;
    let ts_ms = value
        .get("__REALTIME_TIMESTAMP")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|micros| micros / 1000);
    let mut level = value
        .get("PRIORITY")
        .and_then(|v| v.as_str())
        .map(priority_level)
        .unwrap_or("info")
        .to_string();
    // Rust tracing writes everything as PRIORITY=6 and puts the real level in
    // the message, so only ever raise severity based on content.
    let inferred = infer_level("stdout", &text);
    if severity_rank(inferred) > severity_rank(&level) {
        level = inferred.to_string();
    }

    let mut meta = BTreeMap::new();
    for (key, target) in [
        ("_PID", "pid"),
        ("SYSLOG_IDENTIFIER", "identifier"),
        ("_SYSTEMD_UNIT", "systemd_unit"),
        ("_HOSTNAME", "hostname"),
    ] {
        if let Some(field) = value.get(key).and_then(|v| v.as_str()) {
            meta.insert(target.to_string(), Value::String(field.to_string()));
        }
    }

    Some(ParsedLine {
        ts_ms,
        level,
        text,
        meta,
    })
}

/// MESSAGE is a string normally, but journald stores non-UTF8 payloads as a
/// byte array, so accept both.
fn journal_message(value: &Value) -> Option<String> {
    match value.get("MESSAGE") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(bytes)) => {
            let raw: Vec<u8> = bytes
                .iter()
                .filter_map(|b| b.as_u64().map(|n| n as u8))
                .collect();
            if raw.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&raw).into_owned())
            }
        }
        _ => None,
    }
}

fn priority_level(priority: &str) -> &'static str {
    match priority {
        "0" | "1" | "2" | "3" => "error",
        "4" => "warn",
        "7" => "debug",
        _ => "info",
    }
}

#[derive(Debug, Clone)]
pub struct NginxLine {
    pub ts_ms: Option<i64>,
    pub remote_addr: String,
    pub request: String,
    pub status: u16,
    pub body_bytes: Option<u64>,
    pub request_time: Option<f64>,
    pub upstream_time: Option<f64>,
}

fn parse_nginx(raw: &str) -> Option<ParsedLine> {
    let line = parse_nginx_line(raw)?;
    let level = match line.status {
        500..=599 => "error",
        400..=499 => "warn",
        _ => "info",
    };
    let mut meta = BTreeMap::new();
    meta.insert(
        "remote_addr".to_string(),
        Value::String(line.remote_addr.clone()),
    );
    meta.insert("request".to_string(), Value::String(line.request.clone()));
    meta.insert("status".to_string(), Value::from(line.status));
    if let Some(bytes) = line.body_bytes {
        meta.insert("body_bytes".to_string(), Value::from(bytes));
    }
    if let Some(rt) = line.request_time {
        meta.insert("request_time".to_string(), Value::from(rt));
    }
    if let Some(ut) = line.upstream_time {
        meta.insert("upstream_time".to_string(), Value::from(ut));
    }
    Some(ParsedLine {
        ts_ms: line.ts_ms,
        level: level.to_string(),
        text: raw.to_string(),
        meta,
    })
}

pub fn parse_nginx_line(raw: &str) -> Option<NginxLine> {
    // 1) remote_addr
    let (remote_addr, rest) = raw.split_once(' ')?;
    // 2) [time_local]
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    let time_str = &rest[open + 1..close];
    let ts_ms = OffsetDateTime::parse(time_str, &NGINX_TIME)
        .ok()
        .map(|dt| dt.unix_timestamp() * 1000 + i64::from(dt.millisecond()));
    // 3) "request"
    let after = &rest[close + 1..];
    let q1 = after.find('"')?;
    let q2 = after[q1 + 1..].find('"')? + q1 + 1;
    let request = after[q1 + 1..q2].to_string();
    // 4) status, body_bytes_sent, then any bare float as request_time
    let tail = &after[q2 + 1..];
    let mut tokens = tail.split_whitespace().filter(|t| !t.starts_with('"'));
    let status: u16 = tokens.next()?.parse().ok()?;
    let body_bytes = tokens.next().and_then(|t| t.parse::<u64>().ok());
    // Anything numeric after body_bytes is $request_time / $upstream_response_time.
    let mut timings = Vec::new();
    for token in tokens {
        if let Ok(v) = token.parse::<f64>() {
            timings.push(v);
        }
    }
    let request_time = timings.first().copied();
    let upstream_time = timings.get(1).copied();
    Some(NginxLine {
        ts_ms,
        remote_addr: remote_addr.to_string(),
        request,
        status,
        body_bytes,
        request_time,
        upstream_time,
    })
}

/// Best-effort level inference for unstructured lines.
pub fn infer_level(stream: &str, text: &str) -> &'static str {
    if stream == "stderr" {
        return "error";
    }
    let upper = text.to_ascii_uppercase();
    let lower = text.to_ascii_lowercase();
    if upper.contains("ERROR")
        || upper.contains("FATAL")
        || upper.contains("PANIC")
        || lower.contains("traceback (most recent call last)")
        || lower.contains("panicked at")
    {
        return "error";
    }
    if upper.contains("WARN") {
        return "warn";
    }
    if upper.contains("DEBUG") || upper.contains("TRACE") {
        return "debug";
    }
    if let Some(code) = http_status(text) {
        return match code {
            500..=599 => "error",
            400..=499 => "warn",
            _ => "info",
        };
    }
    "info"
}

/// Extract a 3-digit HTTP status following a `HTTP/x.y"` token (nginx format).
fn http_status(text: &str) -> Option<u16> {
    let mut previous_was_http = false;
    for token in text.split_whitespace() {
        if previous_was_http {
            if token.len() == 3 && token.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(code) = token.parse::<u16>() {
                    if (100..=599).contains(&code) {
                        return Some(code);
                    }
                }
            }
            previous_was_http = false;
        }
        if token.contains("HTTP/") && token.ends_with('"') {
            previous_was_http = true;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_journald_json_with_microsecond_ts_and_priority() {
        let raw = r#"{"__REALTIME_TIMESTAMP":"1792416044138123","PRIORITY":"3","MESSAGE":"upstream 503","SYSLOG_IDENTIFIER":"picboo-api","_PID":"42","_SYSTEMD_UNIT":"picboo-api.service"}"#;
        let line = parse_line(Parser::Journald, "stdout", raw).unwrap();
        assert_eq!(line.ts_ms, Some(1792416044138));
        assert_eq!(line.level, "error");
        assert_eq!(line.text, "upstream 503");
        assert_eq!(
            line.meta.get("identifier").and_then(|v| v.as_str()),
            Some("picboo-api")
        );
    }

    #[test]
    fn journald_message_as_byte_array_is_decoded() {
        let raw = r#"{"__REALTIME_TIMESTAMP":"1000000","PRIORITY":"6","MESSAGE":[104,105]}"#;
        let line = parse_line(Parser::Journald, "stdout", raw).unwrap();
        assert_eq!(line.text, "hi");
        assert_eq!(line.level, "info");
    }

    #[test]
    fn journald_message_level_overrides_info_priority() {
        let raw = r#"{"__REALTIME_TIMESTAMP":"1792464644000000","PRIORITY":"6","MESSAGE":"\u001b[2m2026-10-20T02:10:44Z\u001b[0m \u001b[33m WARN\u001b[0m [picboo] slow query"}"#;
        let line = parse_line(Parser::Journald, "stdout", raw).unwrap();
        assert_eq!(line.level, "warn");
        assert!(!line.text.contains('\u{1b}'), "ansi must be stripped: {:?}", line.text);
        assert!(line.text.contains("WARN"));
    }

    #[test]
    fn journald_priority_error_wins() {
        let raw = r#"{"__REALTIME_TIMESTAMP":"1000","PRIORITY":"3","MESSAGE":"plain text"}"#;
        assert_eq!(parse_line(Parser::Journald, "stdout", raw).unwrap().level, "error");
    }

    #[test]
    fn strips_ansi_csi_sequences() {
        assert_eq!(strip_ansi("\u{1b}[2m2026-10-20\u{1b}[0m ok"), "2026-10-20 ok");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
    }

    #[test]
    fn non_json_journald_line_falls_back_to_raw() {
        let line = parse_line(Parser::Journald, "stdout", "Failed to open journal: denied").unwrap();
        assert_eq!(line.ts_ms, None);
        assert_eq!(line.level, "info");
    }

    #[test]
    fn parses_nginx_combined_with_timings() {
        let raw = r#"10.7.0.12 - - [19/Oct/2026:21:51:29 +0800] "POST /api/auth/wx_login HTTP/1.1" 503 412 "-" "wechat" 1.234 0.987"#;
        let line = parse_line(Parser::Nginx, "stdout", raw).unwrap();
        assert_eq!(line.level, "error");
        // 2026-10-19T21:51:29+08:00 == 2026-10-19T13:51:29Z
        assert_eq!(line.ts_ms, Some(1792417889000));
        let parsed = parse_nginx_line(raw).unwrap();
        assert_eq!(parsed.status, 503);
        assert_eq!(parsed.body_bytes, Some(412));
        assert_eq!(parsed.request_time, Some(1.234));
        assert_eq!(parsed.upstream_time, Some(0.987));
    }

    #[test]
    fn parses_nginx_without_timings() {
        let raw = r#"1.2.3.4 - - [19/Oct/2026:21:51:29 +0800] "GET /health HTTP/1.1" 200 12 "-" "curl""#;
        let line = parse_line(Parser::Nginx, "stdout", raw).unwrap();
        assert_eq!(line.level, "info");
        assert_eq!(parse_nginx_line(raw).unwrap().request_time, None);
    }

    #[test]
    fn parses_logcat_epoch_threadtime_with_device_timestamp() {
        let raw = "1792416044.138  1234  5678 E Godot: rust panicked at render";
        let line = parse_line(Parser::Logcat, "stdout", raw).unwrap();
        assert_eq!(line.ts_ms, Some(1792416044138));
        assert_eq!(line.level, "error");
        assert_eq!(line.text, raw);
        assert_eq!(line.meta.get("pid").and_then(|v| v.as_u64()), Some(1234));
        assert_eq!(line.meta.get("tid").and_then(|v| v.as_u64()), Some(5678));
        assert_eq!(line.meta.get("tag").and_then(|v| v.as_str()), Some("Godot"));
        assert_eq!(
            line.meta.get("priority").and_then(|v| v.as_str()),
            Some("E")
        );
    }

    #[test]
    fn logcat_priorities_map_to_view_levels() {
        for (priority, expected) in [
            ("V", "debug"),
            ("D", "debug"),
            ("I", "info"),
            ("W", "warn"),
            ("E", "error"),
            ("F", "error"),
            ("A", "error"),
        ] {
            let raw = format!("1792416044.138 1 2 {priority} Tag: message");
            assert_eq!(
                parse_line(Parser::Logcat, "stdout", &raw).unwrap().level,
                expected
            );
        }
    }

    #[test]
    fn parses_plain_logcat_threadtime_without_inventing_timezone() {
        for raw in [
            "10-20 14:29:31.123  1234  5678 W Godot: WARNING: slow frame",
            "10-20 14:29:31.123 +0000  1234  5678 W Godot: WARNING: slow frame",
        ] {
            let line = parse_line(Parser::Logcat, "stdout", raw).unwrap();
            assert_eq!(line.ts_ms, None);
            assert_eq!(line.level, "warn");
            assert_eq!(
                line.meta.get("tag").and_then(|v| v.as_str()),
                Some("Godot")
            );
        }
    }

    #[test]
    fn non_logcat_line_falls_back_to_raw() {
        let raw = "--------- beginning of main";
        let line = parse_line(Parser::Logcat, "stdout", raw).unwrap();
        assert_eq!(line.ts_ms, None);
        assert_eq!(line.level, "info");
        assert_eq!(line.text, raw);
        assert!(line.meta.is_empty());
    }

    #[test]
    fn logcat_epoch_supports_microseconds_and_rejects_dates() {
        assert_eq!(
            parse_logcat_epoch_ms("1792416044.138999"),
            Some(1792416044138)
        );
        assert_eq!(parse_logcat_epoch_ms("1792416044"), Some(1792416044000));
        assert_eq!(parse_logcat_epoch_ms("10-20"), None);
    }

    #[test]
    fn builds_logcat_command_with_serial_and_prewarm() {
        let spec = SourceSpec::Logcat {
            serial: Some("emulator-5554".to_string()),
            lines: 200,
        };
        let (argv, parser) = spec.build().unwrap();
        assert_eq!(parser, Parser::Logcat);
        assert_eq!(
            argv,
            vec![
                "adb",
                "-s",
                "emulator-5554",
                "logcat",
                "-v",
                "epoch",
                "-T",
                "200"
            ]
        );
    }

    #[test]
    fn builds_journald_remote_command_with_since() {
        let spec = SourceSpec::Journald {
            remote: Some("picboo-ali".to_string()),
            unit: "picboo-api".to_string(),
            since: Some("15m".to_string()),
            lines: 200,
            sudo: true,
        };
        let (argv, parser) = spec.build().unwrap();
        assert_eq!(parser, Parser::Journald);
        assert_eq!(argv[0], "ssh");
        assert_eq!(argv[1], "-o");
        assert_eq!(argv[2], "BatchMode=yes");
        assert_eq!(argv[3], "picboo-ali");
        assert!(argv[4].contains("journalctl -o json -f"));
        assert!(argv[4].contains("--since"));
        assert!(argv[4].contains("900 sec ago"));
        assert!(argv[4].starts_with("sudo -n "));
    }

    #[test]
    fn resumes_journald_from_last_timestamp() {
        let spec = SourceSpec::Journald {
            remote: Some("picboo-ali".to_string()),
            unit: "picboo-api".to_string(),
            since: Some("15m".to_string()),
            lines: 500,
            sudo: true,
        };
        let (argv, parser) = spec.build_resumed(Some(1_792_416_999_123)).unwrap();
        assert_eq!(parser, Parser::Journald);
        let script = &argv[4];
        assert!(script.contains("--since @1792416999.123001"), "{script}");
        assert!(!script.contains("15 min ago"), "{script}");
        // no resume point yet → normal prewarm
        let (argv, _) = spec.build_resumed(None).unwrap();
        assert!(argv[4].contains("900 sec ago"), "{}", argv[4]);
    }

    #[test]
    fn journal_since_is_one_microsecond_past_last_line() {
        assert_eq!(journal_since_at(1_000), "@1.000001");
        assert_eq!(journal_since_at(1_792_416_999_000), "@1792416999.000001");
        assert_eq!(journal_since_at(0), "@0.000001");
    }

    #[test]
    fn builds_local_nginx_command() {
        let spec = SourceSpec::Nginx {
            remote: None,
            path: "/var/log/nginx/access.log".to_string(),
            lines: 50,
            sudo: false,
            follow: true,
        };
        let (argv, parser) = spec.build().unwrap();
        assert_eq!(parser, Parser::Nginx);
        assert_eq!(argv, vec!["tail", "-F", "-n", "50", "/var/log/nginx/access.log"]);
    }
}
