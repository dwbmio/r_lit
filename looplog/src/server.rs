//! Local HTTP intake server.
//!
//! Two write paths share one SQLite database:
//! - native: `POST /v1/runs`, `POST /v1/runs/{id}/lines`, `PATCH /v1/runs/{id}`
//! - Loki compatible: `POST /loki/api/v1/push`
//!
//! The Loki path exists because the WeChat Mini Program frontend already ships a
//! `LokiClient`; pointing it at looplog gives structured client logs without a
//! second reporter implementation. Label `loglog_tag` (or `app_id`/`source`)
//! becomes the `runs.tag`, so `looplog view --column client:picboo-client`
//! lines up with server streams.
//!
//! Exposure is explicit: loopback only by default; `--allow-lan` requires a
//! token and peers must fall inside the CIDR allowlist.

use crate::db::{format_ms, NewRun, SqliteStorage};
use crate::error::{Error, Result};
use crate::protocol::{
    AppendLine, ErrorResponse, FinishRunRequest, HealthResponse, StartRunRequest, StartRunResponse,
};
use crate::source::infer_level;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const DEFAULT_CIDRS: &[&str] = &[
    "127.0.0.0/8",
    "::1/128",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
];

pub struct ServeOptions {
    pub addr: String,
    pub keep_hours: u64,
    pub allow_lan: bool,
    pub token: Option<String>,
    pub allow_cidrs: Vec<String>,
}

#[derive(Clone)]
struct AuthGuard {
    token: Option<String>,
    cidrs: Vec<Cidr>,
    allow_lan: bool,
}

impl AuthGuard {
    fn new(opts: &ServeOptions) -> Result<Self> {
        let token = opts.token.clone().filter(|t| !t.is_empty());
        if opts.allow_lan {
            match &token {
                Some(t) if t.len() >= 16 => {}
                Some(_) => {
                    return Err(Error::Message(
                        "--token must be at least 16 characters when using --allow-lan".into(),
                    ))
                }
                None => {
                    return Err(Error::Message(
                        "--allow-lan requires --token (otherwise anyone on the LAN can write)"
                            .into(),
                    ))
                }
            }
        }
        let raw = if opts.allow_cidrs.is_empty() {
            DEFAULT_CIDRS.iter().map(|s| s.to_string()).collect()
        } else {
            opts.allow_cidrs.clone()
        };
        let mut cidrs = Vec::new();
        for spec in raw {
            for part in spec.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                cidrs.push(Cidr::parse(part).ok_or_else(|| {
                    Error::Message(format!("invalid --allow-cidr `{part}`"))
                })?);
            }
        }
        Ok(Self {
            token,
            cidrs,
            allow_lan: opts.allow_lan,
        })
    }

    /// Loopback is always allowed. Anything else needs allow-lan + CIDR + token.
    fn authorize(&self, peer: IpAddr, request: &Request) -> std::result::Result<(), &'static str> {
        if peer.is_loopback() {
            return Ok(());
        }
        if !self.allow_lan {
            return Err("non-loopback peer rejected (start with --allow-lan to permit LAN)");
        }
        if !self.cidrs.iter().any(|c| c.contains(&peer)) {
            return Err("peer not in --allow-cidr allowlist");
        }
        let Some(expected) = &self.token else {
            return Err("server has no token configured");
        };
        let supplied = bearer_token(request).ok_or("missing token header")?;
        if &supplied != expected {
            return Err("invalid token");
        }
        Ok(())
    }
}

fn bearer_token(request: &Request) -> Option<String> {
    for header in request.headers() {
        let field = header.field.as_str().as_str().to_ascii_lowercase();
        let value = header.value.as_str().trim().to_string();
        if field == "authorization" {
            if let Some(token) = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")) {
                return Some(token.trim().to_string());
            }
        }
        if field == "x-looplog-token" {
            return Some(value);
        }
    }
    None
}

#[derive(Clone, Copy)]
struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    fn parse(spec: &str) -> Option<Self> {
        let (addr, prefix) = spec.split_once('/')?;
        let network: IpAddr = addr.trim().parse().ok()?;
        let prefix: u8 = prefix.trim().parse().ok()?;
        let max = if network.is_ipv4() { 32 } else { 128 };
        if prefix > max {
            return None;
        }
        Some(Self { network, prefix })
    }

    fn contains(&self, ip: &IpAddr) -> bool {
        match (self.network, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                if self.prefix == 0 {
                    return true;
                }
                let mask = u32::MAX << (32 - self.prefix as u32);
                (u32::from(net) & mask) == (u32::from(*ip) & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                if self.prefix == 0 {
                    return true;
                }
                let mask = u128::MAX << (128 - self.prefix as u32);
                (u128::from(net) & mask) == (u128::from(*ip) & mask)
            }
            _ => false,
        }
    }
}

type RunMap = Arc<Mutex<HashMap<String, String>>>;

pub fn serve(db_path: PathBuf, opts: ServeOptions) -> Result<()> {
    let guard = AuthGuard::new(&opts)?;
    let server = Server::http(&opts.addr)?;
    let runs: RunMap = Arc::new(Mutex::new(HashMap::new()));
    eprintln!(
        "looplog serve listening on http://{}{}",
        opts.addr,
        if opts.allow_lan {
            " (LAN enabled, token required for non-loopback peers)"
        } else {
            ""
        }
    );
    eprintln!("  POST /v1/runs            native run create");
    eprintln!("  POST /v1/runs/{{id}}/lines  native ndjson append");
    eprintln!("  POST /loki/api/v1/push    Loki compatible (mini program client)");

    for request in server.incoming_requests() {
        let db_path = db_path.clone();
        let guard = guard.clone();
        let runs = runs.clone();
        let keep_hours = opts.keep_hours;
        thread::spawn(move || {
            if let Err(e) = handle_request(request, &db_path, keep_hours, &guard, &runs) {
                eprintln!("[looplog serve] request failed: {}", e);
            }
        });
    }
    Ok(())
}

fn handle_request(
    mut request: Request,
    db_path: &Path,
    keep_hours: u64,
    guard: &AuthGuard,
    runs: &RunMap,
) -> Result<()> {
    let method = request.method().clone();
    let path = request.url().split('?').next().unwrap_or("/").to_string();

    let peer = request
        .remote_addr()
        .map(|a| a.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));

    if method == Method::Options {
        return respond_cors_preflight(request);
    }
    if path == "/healthz" {
        return respond_json(
            request,
            StatusCode(200),
            &HealthResponse {
                status: "ok",
                service: "looplog",
            },
        );
    }
    if let Err(reason) = guard.authorize(peer, &request) {
        let status = if reason.contains("token") { 401 } else { 403 };
        return respond_json(
            request,
            StatusCode(status),
            &ErrorResponse {
                status: "error",
                error: reason.to_string(),
            },
        );
    }

    let mut storage = SqliteStorage::open(db_path)?;
    storage.clean(keep_hours, false)?;

    match (method.clone(), path.as_str()) {
        (Method::Post, "/loki/api/v1/push") => {
            let body = read_body(&mut request)?;
            let count = ingest_loki_push(&mut storage, runs, &body)?;
            eprintln!("[looplog serve] loki push → {} line(s)", count);
            respond_json(
                request,
                StatusCode(200),
                &serde_json::json!({"status": "ok", "lines": count}),
            )
        }
        (Method::Post, "/v1/runs") => {
            let req: StartRunRequest = read_json(&mut request)?;
            let run_id = storage.start_run(NewRun {
                tag: req.tag,
                source: req.source,
                cwd: req.cwd,
                argv: req.argv,
                client_id: req.client_id,
                kind: req.kind,
                meta: req.meta,
            })?;
            respond_json(request, StatusCode(200), &StartRunResponse { run_id })
        }
        _ => {
            if method == Method::Post && path.starts_with("/v1/runs/") && path.ends_with("/lines") {
                let run_id = path
                    .trim_start_matches("/v1/runs/")
                    .trim_end_matches("/lines")
                    .trim_end_matches('/');
                let body = read_body(&mut request)?;
                let lines = parse_ndjson_lines(&body)?;
                let count = storage.append_lines(run_id, &lines)?;
                respond_json(
                    request,
                    StatusCode(200),
                    &serde_json::json!({"status": "ok", "lines": count}),
                )
            } else if method == Method::Patch && path.starts_with("/v1/runs/") {
                let run_id = path.trim_start_matches("/v1/runs/").trim_end_matches('/');
                let req: FinishRunRequest = read_json(&mut request)?;
                storage.finish_run(run_id, &req)?;
                respond_json(
                    request,
                    StatusCode(200),
                    &serde_json::json!({"status": "ok"}),
                )
            } else {
                respond_json(
                    request,
                    StatusCode(404),
                    &ErrorResponse {
                        status: "error",
                        error: "not found".to_string(),
                    },
                )
            }
        }
    }
}

// ---------------------------------------------------------------- Loki intake

#[derive(Debug, Deserialize)]
struct LokiPush {
    #[serde(default)]
    streams: Vec<LokiStream>,
}

#[derive(Debug, Deserialize)]
struct LokiStream {
    #[serde(default)]
    stream: BTreeMap<String, String>,
    #[serde(default)]
    values: Vec<(String, String)>,
}

fn ingest_loki_push(
    storage: &mut SqliteStorage,
    runs: &RunMap,
    body: &str,
) -> Result<usize> {
    let push: LokiPush = serde_json::from_str(body)?;
    let mut total = 0usize;

    for stream in push.streams {
        if stream.values.is_empty() {
            continue;
        }
        let tag = tag_from_labels(&stream.stream);
        let run_id = ensure_run(storage, runs, &tag, &stream.stream)?;

        let mut lines = Vec::with_capacity(stream.values.len());
        for (ts_ns, raw) in &stream.values {
            let ts_ms = ts_ns.parse::<i128>().ok().map(|ns| (ns / 1_000_000) as i64);
            let (text, mut meta) = parse_loki_line(raw);
            for (key, value) in &stream.stream {
                meta.entry(key.clone())
                    .or_insert_with(|| Value::String(value.clone()));
            }
            let level = stream
                .stream
                .get("level")
                .map(|l| normalize_level(l))
                .unwrap_or_else(|| infer_level("stdout", &text).to_string());
            lines.push(AppendLine {
                ts: ts_ms.map(format_ms),
                stream: Some("console".to_string()),
                level: Some(level),
                text,
                meta,
                ..Default::default()
            });
        }
        total += storage.append_lines(&run_id, &lines)?;
    }
    Ok(total)
}

/// Label priority: explicit `looplog_tag`, then `app_id`, then `source`.
fn tag_from_labels(labels: &BTreeMap<String, String>) -> String {
    for key in ["looplog_tag", "app_id", "source"] {
        if let Some(value) = labels.get(key).map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return value.to_string();
        }
    }
    "loki".to_string()
}

/// Reuse one run per label set for the lifetime of this process, so a client
/// session keeps appending instead of creating a run per push.
fn ensure_run(
    storage: &mut SqliteStorage,
    runs: &RunMap,
    tag: &str,
    labels: &BTreeMap<String, String>,
) -> Result<String> {
    let key = serde_json::to_string(labels)?;
    if let Some(run_id) = runs.lock().map(|m| m.get(&key).cloned()).ok().flatten() {
        return Ok(run_id);
    }

    let meta: BTreeMap<String, Value> = labels
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    let run_id = storage.start_run(NewRun {
        tag: Some(tag.to_string()),
        source: labels.get("source").cloned().or_else(|| Some("loki-push".into())),
        cwd: None,
        argv: Vec::new(),
        client_id: None,
        kind: Some("wechat_miniprogram".to_string()),
        meta,
    })?;
    if let Ok(mut map) = runs.lock() {
        map.insert(key, run_id.clone());
    }
    Ok(run_id)
}

/// Loki values carry either the raw message or the JSON line produced by
/// `LokiClient.formatLogLine`.
fn parse_loki_line(raw: &str) -> (String, BTreeMap<String, Value>) {
    let mut meta = BTreeMap::new();
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(raw) {
        for (key, value) in &obj {
            if key != "message" {
                meta.insert(key.clone(), value.clone());
            }
        }
        if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
            return (message.to_string(), meta);
        }
    }
    (raw.to_string(), meta)
}

fn normalize_level(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "debug" | "trace" => "debug".to_string(),
        "warn" | "warning" => "warn".to_string(),
        "error" | "fatal" | "panic" => "error".to_string(),
        _ => "info".to_string(),
    }
}

// ---------------------------------------------------------------- HTTP helpers

fn parse_ndjson_lines(body: &str) -> Result<Vec<AppendLine>> {
    let mut lines = Vec::new();
    for raw in body.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        lines.push(serde_json::from_str(raw)?);
    }
    Ok(lines)
}

fn read_json<T: serde::de::DeserializeOwned>(request: &mut Request) -> Result<T> {
    let body = read_body(request)?;
    Ok(serde_json::from_str(&body)?)
}

fn read_body(request: &mut Request) -> Result<String> {
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

fn respond_cors_preflight(request: Request) -> Result<()> {
    let mut response = Response::empty(StatusCode(204));
    for (name, value) in [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Methods", "GET, POST, PATCH, OPTIONS"),
        (
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-Looplog-Token",
        ),
        ("Access-Control-Max-Age", "600"),
    ] {
        if let Ok(header) = Header::from_bytes(name, value) {
            response.add_header(header);
        }
    }
    request
        .respond(response)
        .map_err(|e| Error::Http(e.to_string()))
}

fn respond_json<T: Serialize>(request: Request, status: StatusCode, value: &T) -> Result<()> {
    respond(
        request,
        status,
        "application/json; charset=utf-8",
        serde_json::to_string(value)?,
    )
}

fn respond(request: Request, status: StatusCode, content_type: &str, body: String) -> Result<()> {
    let mut response = Response::from_string(body).with_status_code(status);
    for (name, value) in [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("Access-Control-Allow-Origin", "*"),
    ] {
        if let Ok(header) = Header::from_bytes(name, value) {
            response.add_header(header);
        }
    }
    request
        .respond(response)
        .map_err(|e| Error::Http(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ndjson_batch() {
        let lines = parse_ndjson_lines(
            r#"{"level":"error","stream":"console","text":"TypeError"}
{"level":"info","text":"ok"}"#,
        )
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "TypeError");
    }

    #[test]
    fn cidr_matches_private_ranges() {
        let cidr = Cidr::parse("192.168.0.0/16").unwrap();
        assert!(cidr.contains(&"192.168.1.20".parse().unwrap()));
        assert!(!cidr.contains(&"10.0.0.1".parse().unwrap()));
        let all = Cidr::parse("0.0.0.0/0").unwrap();
        assert!(all.contains(&"8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn maps_loki_labels_to_tag() {
        let mut labels = BTreeMap::new();
        labels.insert("app_id".to_string(), "picboo".to_string());
        labels.insert("source".to_string(), "typescript".to_string());
        assert_eq!(tag_from_labels(&labels), "picboo");
        labels.insert("looplog_tag".to_string(), "picboo-client".to_string());
        assert_eq!(tag_from_labels(&labels), "picboo-client");
    }

    #[test]
    fn parses_loki_json_line_into_message_and_meta() {
        let (text, meta) = parse_loki_line(
            r#"{"timestamp":"2026-10-20T02:10:44Z","level":"warn","source":"typescript","message":"slow query","metadata":{"ms":12}}"#,
        );
        assert_eq!(text, "slow query");
        assert_eq!(meta.get("level").and_then(|v| v.as_str()), Some("warn"));
        assert!(meta.contains_key("metadata"));
    }

    #[test]
    fn raw_loki_line_is_kept_as_text() {
        let (text, meta) = parse_loki_line("plain line");
        assert_eq!(text, "plain line");
        assert!(meta.is_empty());
    }

    #[test]
    fn allow_lan_requires_strong_token() {
        let base = ServeOptions {
            addr: "127.0.0.1:3768".into(),
            keep_hours: 24,
            allow_lan: true,
            token: None,
            allow_cidrs: vec![],
        };
        assert!(AuthGuard::new(&base).is_err());
        let weak = ServeOptions {
            token: Some("short".into()),
            ..base
        };
        assert!(AuthGuard::new(&weak).is_err());
        let ok = ServeOptions {
            token: Some("0123456789abcdef".into()),
            ..weak
        };
        assert!(AuthGuard::new(&ok).is_ok());
    }
}
