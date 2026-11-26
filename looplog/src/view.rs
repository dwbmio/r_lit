//! `looplog view`: a local, read-only log viewer plus a small control plane.
//!
//! The viewer owns the configuration (`~/.config/looplog/config.toml`): it can
//! list sources, start/stop them, and write the file back. That makes the page
//! the single place to see "what am I following and why".
//!
//! Read-only against the database: only the tail runners it spawns write.

use crate::config::{self, Config};
use crate::db::now_ms;
use crate::error::{Error, Result};
use crate::tail::{spawn_runner, RunnerHandle, TailOptions};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX_HTML: &str = include_str!("view/index.html");
const DEFAULT_LIMIT: usize = 3000;
const MAX_LIMIT: usize = 8000;
const BUSY_TIMEOUT_MS: u32 = 800;

pub struct ViewOptions {
    pub addr: String,
    pub config: Config,
    pub config_path: PathBuf,
    pub open: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LineOut {
    run: String,
    seq: i64,
    ts_ms: i64,
    ingest_ms: i64,
    stream: String,
    level: String,
    text: String,
    tag: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TagOut {
    tag: Option<String>,
    lines: i64,
    last_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
struct SourceStatus {
    name: String,
    tag: String,
    from: String,
    target: String,
    enabled: bool,
    running: bool,
    run_id: Option<String>,
    started_ms: Option<i64>,
    label: Option<String>,
}

struct ViewState {
    db_path: PathBuf,
    config_path: PathBuf,
    config: Mutex<Config>,
    runners: Mutex<HashMap<String, RunnerHandle>>,
}

type SharedState = Arc<ViewState>;

pub fn serve(db_path: PathBuf, opts: ViewOptions) -> Result<()> {
    let state = Arc::new(ViewState {
        db_path,
        config_path: opts.config_path.clone(),
        config: Mutex::new(opts.config.clone()),
        runners: Mutex::new(HashMap::new()),
    });

    if opts.config.view.autostart {
        let enabled: Vec<String> = opts
            .config
            .enabled_sources()
            .map(|s| s.name.clone())
            .collect();
        for name in enabled {
            if let Err(e) = start_source(&state, &name) {
                eprintln!("[view] could not autostart `{name}`: {e}");
            }
        }
    }

    let server = Server::http(&opts.addr)?;
    let url = format!("http://{}", opts.addr);
    eprintln!(
        "looplog view → {url}  (config={}, db={})",
        opts.config_path.display(),
        state.db_path.display()
    );
    if !state.db_path.exists() {
        eprintln!("[view] warning: database does not exist yet; start a source first");
    }
    if opts.open {
        open_browser(&url);
    }

    let boot = serde_json::json!({
        "sinceMs": crate::db::parse_since(&opts.config.view.since).unwrap_or_else(|_| now_ms() - 900_000),
        "columns": opts.config.view.columns.iter().map(|c| {
            serde_json::json!({"label": c.label, "tag": c.tag})
        }).collect::<Vec<_>>(),
        "colors": opts.config.view.colors,
        "db": state.db_path.display().to_string(),
        "addr": opts.addr,
        "configPath": opts.config_path.display().to_string(),
    });
    let boot_js = format!(
        "window.LOOPLOG_BOOT = {};",
        serde_json::to_string(&boot)?
    );

    for request in server.incoming_requests() {
        let state = state.clone();
        let boot_js = boot_js.clone();
        thread::spawn(move || {
            if let Err(e) = handle(request, &state, &boot_js) {
                eprintln!("[view] request failed: {}", e);
            }
        });
    }
    Ok(())
}

// ------------------------------------------------------------------ runners

fn start_source(state: &SharedState, name: &str) -> Result<()> {
    let source = {
        let cfg = state
            .config
            .lock()
            .map_err(|_| Error::Message("config lock poisoned".into()))?;
        cfg.source
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or_else(|| Error::Message(format!("unknown source `{name}`")))?
    };

    {
        let mut runners = state
            .runners
            .lock()
            .map_err(|_| Error::Message("runner lock poisoned".into()))?;
        if runners.get(name).map(|h| h.is_running()).unwrap_or(false) {
            return Ok(());
        }
        runners.remove(name);
    }

    let spec = source.to_spec()?;
    let opts = TailOptions {
        tag: source.tag.clone(),
        kind: source.kind.clone(),
        source: Some("looplog-view".to_string()),
        meta: BTreeMap::new(),
        reconnect: source.reconnect,
        reconnect_max_secs: 30,
        spec,
    };
    let handle = spawn_runner(name, state.db_path.clone(), opts, source.command.clone())?;
    eprintln!(
        "[view] started `{}` tag={} ({})",
        name, handle.tag, handle.label
    );
    let mut runners = state
        .runners
        .lock()
        .map_err(|_| Error::Message("runner lock poisoned".into()))?;
    runners.insert(name.to_string(), handle);
    Ok(())
}

fn stop_source(state: &SharedState, name: &str) -> Result<()> {
    let mut runners = state
        .runners
        .lock()
        .map_err(|_| Error::Message("runner lock poisoned".into()))?;
    if let Some(mut handle) = runners.remove(name) {
        handle.stop();
        // Give the reader a moment to exit so the UI does not show a stale
        // "running" dot right after the click.
        for _ in 0..40 {
            if !handle.is_running() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        handle.join();
        eprintln!("[view] stopped `{name}`");
    }
    Ok(())
}

fn source_statuses(state: &SharedState) -> Result<Vec<SourceStatus>> {
    let cfg = state
        .config
        .lock()
        .map_err(|_| Error::Message("config lock poisoned".into()))?
        .clone();
    let runners = state
        .runners
        .lock()
        .map_err(|_| Error::Message("runner lock poisoned".into()))?;

    Ok(cfg
        .source
        .iter()
        .map(|s| {
            let handle = runners.get(&s.name).filter(|h| h.is_running());
            SourceStatus {
                name: handle
                    .map(|h| h.name.clone())
                    .unwrap_or_else(|| s.name.clone()),
                tag: s.tag.clone(),
                from: s.from.clone(),
                target: s.target(),
                enabled: s.enabled,
                running: handle.is_some(),
                run_id: handle.map(|h| h.run_id.clone()),
                started_ms: handle.map(|h| h.started_ms),
                label: handle.map(|h| h.label.clone()),
            }
        })
        .collect())
}

fn apply_config(state: &SharedState, next: Config) -> Result<()> {
    next.validate()?;
    config::save(&state.config_path, &next)?;
    {
        let mut cfg = state
            .config
            .lock()
            .map_err(|_| Error::Message("config lock poisoned".into()))?;
        *cfg = next.clone();
    }

    // Restart everything so the page reflects the file immediately: stop all,
    // then start whatever is still enabled.
    let running: Vec<String> = {
        let runners = state
            .runners
            .lock()
            .map_err(|_| Error::Message("runner lock poisoned".into()))?;
        runners.keys().cloned().collect()
    };
    for name in running {
        let _ = stop_source(state, &name);
    }
    if next.view.autostart {
        for source in next.enabled_sources() {
            if let Err(e) = start_source(state, &source.name) {
                eprintln!("[view] could not start `{}`: {e}", source.name);
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ requests

fn handle(mut request: Request, state: &SharedState, boot_js: &str) -> Result<()> {
    let url = request.url().to_string();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (url.clone(), BTreeMap::new()),
    };
    let method = request.method().clone();

    match (method.clone(), path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            let html = INDEX_HTML.replace("/*__BOOTSTRAP__*/", boot_js);
            respond(request, StatusCode(200), "text/html; charset=utf-8", html)
        }
        (Method::Get, "/api/lines") => {
            let after_ms = query
                .get("after_ms")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let limit = query
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(DEFAULT_LIMIT)
                .min(MAX_LIMIT);
            let tags = split_tags(query.get("tags"));
            let tail = query.get("tail").map(|v| v == "1").unwrap_or(false);
            match query_lines(&state.db_path, &tags, after_ms, limit, tail) {
                Ok(lines) => respond_json(
                    request,
                    StatusCode(200),
                    &serde_json::json!({"status": "ok", "lines": lines, "now_ms": now_ms()}),
                ),
                Err(e) => respond_json(
                    request,
                    StatusCode(500),
                    &serde_json::json!({"status": "error", "error": e.to_string()}),
                ),
            }
        }
        (Method::Get, "/api/tags") => {
            let since_ms = query
                .get("since_ms")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            match query_tags(&state.db_path, since_ms) {
                Ok(tags) => respond_json(
                    request,
                    StatusCode(200),
                    &serde_json::json!({"status": "ok", "tags": tags}),
                ),
                Err(e) => respond_json(
                    request,
                    StatusCode(500),
                    &serde_json::json!({"status": "error", "error": e.to_string()}),
                ),
            }
        }
        (Method::Get, "/api/config") => {
            let cfg = state
                .config
                .lock()
                .map(|c| c.clone())
                .map_err(|_| Error::Message("config lock poisoned".into()))?;
            respond_json(
                request,
                StatusCode(200),
                &serde_json::json!({
                    "status": "ok",
                    "path": state.config_path.display().to_string(),
                    "config": cfg,
                }),
            )
        }
        (Method::Put, "/api/config") => {
            let body = read_body(&mut request)?;
            match serde_json::from_str::<Config>(&body) {
                Ok(next) => match apply_config(state, next.clone()) {
                    Ok(()) => respond_json(
                        request,
                        StatusCode(200),
                        &serde_json::json!({
                            "status": "ok",
                            "path": state.config_path.display().to_string(),
                            "config": next,
                        }),
                    ),
                    Err(e) => respond_json(
                        request,
                        StatusCode(400),
                        &serde_json::json!({"status": "error", "error": e.to_string()}),
                    ),
                },
                Err(e) => respond_json(
                    request,
                    StatusCode(400),
                    &serde_json::json!({"status": "error", "error": format!("invalid JSON: {e}")}),
                ),
            }
        }
        (Method::Get, "/api/sources") => match source_statuses(state) {
            Ok(sources) => respond_json(
                request,
                StatusCode(200),
                &serde_json::json!({"status": "ok", "sources": sources}),
            ),
            Err(e) => respond_json(
                request,
                StatusCode(500),
                &serde_json::json!({"status": "error", "error": e.to_string()}),
            ),
        },
        (Method::Post, p) if p.starts_with("/api/sources/") => {
            let rest = p.trim_start_matches("/api/sources/");
            let (name, action) = match rest.rsplit_once('/') {
                Some((name, action)) => (name.to_string(), action.to_string()),
                None => (rest.to_string(), String::new()),
            };
            let name = percent_decode(&name);
            let result = match action.as_str() {
                "start" => start_source(state, &name),
                "stop" => stop_source(state, &name),
                "restart" => stop_source(state, &name).and_then(|_| start_source(state, &name)),
                other => Err(Error::Message(format!("unknown action `{other}`"))),
            };
            match result {
                Ok(()) => match source_statuses(state) {
                    Ok(sources) => respond_json(
                        request,
                        StatusCode(200),
                        &serde_json::json!({"status": "ok", "sources": sources}),
                    ),
                    Err(e) => respond_json(
                        request,
                        StatusCode(500),
                        &serde_json::json!({"status": "error", "error": e.to_string()}),
                    ),
                },
                Err(e) => respond_json(
                    request,
                    StatusCode(400),
                    &serde_json::json!({"status": "error", "error": e.to_string()}),
                ),
            }
        }
        (Method::Get, "/healthz") => respond_json(
            request,
            StatusCode(200),
            &serde_json::json!({"status": "ok", "service": "looplog-view"}),
        ),
        _ => respond(
            request,
            StatusCode(404),
            "text/plain; charset=utf-8",
            "not found".to_string(),
        ),
    }
}

// ------------------------------------------------------------------ storage

fn open_readonly(db: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS as u64))?;
    Ok(conn)
}

fn query_lines(
    db: &Path,
    tags: &[String],
    after_ms: i64,
    limit: usize,
    tail: bool,
) -> Result<Vec<LineOut>> {
    if tags.is_empty() || !db.exists() {
        return Ok(Vec::new());
    }
    let conn = open_readonly(db)?;
    if !table_exists(&conn, "log_lines")? {
        return Ok(Vec::new());
    }

    let placeholders = vec!["?"; tags.len()].join(",");
    // 1M rows in the window: without the hint SQLite drives from `runs(tag)` and
    // scans every line of every matching run (~70ms per poll). Driving from
    // `log_lines(ts_ms, seq)` reads only the cursor range (~2s of traffic).
    // Old databases may predate the index, so only hint when it exists.
    let hint = if table_exists(&conn, "idx_lines_ts_seq")? {
        " INDEXED BY idx_lines_ts_seq"
    } else {
        ""
    };
    let select = format!(
        "SELECT l.run_id, l.seq, l.ts_ms, COALESCE(l.ingest_ms, l.ts_ms), l.stream, l.level, l.text, r.tag \
         FROM log_lines l{hint} JOIN runs r ON r.id = l.run_id \
         WHERE l.ts_ms >= ? AND r.tag IN ({placeholders})"
    );
    let sql = if tail {
        format!(
            "SELECT * FROM ({select} ORDER BY l.ts_ms DESC, l.seq DESC LIMIT ?) \
             ORDER BY ts_ms ASC, seq ASC"
        )
    } else {
        format!("{select} ORDER BY l.ts_ms ASC, l.seq ASC LIMIT ?")
    };
    let mut stmt = conn.prepare(&sql)?;
    let mut values: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::Integer(after_ms)];
    values.extend(
        tags.iter()
            .map(|t| rusqlite::types::Value::Text(t.clone())),
    );
    values.push(rusqlite::types::Value::Integer(limit as i64));

    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |row| {
        Ok(LineOut {
            run: row.get(0)?,
            seq: row.get(1)?,
            ts_ms: row.get(2)?,
            ingest_ms: row.get(3)?,
            stream: row.get(4)?,
            level: row.get(5)?,
            text: row.get(6)?,
            tag: row.get(7)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn query_tags(db: &Path, since_ms: i64) -> Result<Vec<TagOut>> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    let conn = open_readonly(db)?;
    if !table_exists(&conn, "runs")? {
        return Ok(Vec::new());
    }
    // Aggregate from `runs` (O(runs)) instead of scanning `log_lines` (O(lines)):
    // with a 1M-row backlog the line scan took ~300ms and only feeds the column
    // picker / tag discovery.
    let mut stmt = conn.prepare(
        "SELECT tag, SUM(line_count), MAX(COALESCE(ended_at_ms, started_at_ms)) \
         FROM runs WHERE tag IS NOT NULL AND COALESCE(ended_at_ms, started_at_ms) >= ? \
         GROUP BY tag ORDER BY MAX(COALESCE(ended_at_ms, started_at_ms)) DESC LIMIT 40",
    )?;
    let rows = stmt.query_map([since_ms], |row| {
        Ok(TagOut {
            tag: row.get(0)?,
            lines: row.get(1)?,
            last_ms: row.get(2)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name=?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

// ------------------------------------------------------------------ helpers

fn parse_query(query: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        out.insert(key.to_string(), percent_decode(value));
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn split_tags(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn read_body(request: &mut Request) -> Result<String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(Error::Io)?;
    Ok(body)
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
    let mut response = Response::from_string(body)
        .with_status_code(status)
        .with_header(
            Header::from_bytes("Content-Type", content_type)
                .map_err(|_| Error::Message("bad content-type header".to_string()))?,
        )
        .with_header(
            Header::from_bytes("Cache-Control", "no-store")
                .map_err(|_| Error::Message("bad cache header".to_string()))?,
        );
    response.add_header(
        Header::from_bytes("Access-Control-Allow-Origin", "*")
            .map_err(|_| Error::Message("bad cors header".to_string()))?,
    );
    request
        .respond(response)
        .map_err(|e| Error::Http(e.to_string()))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let chrome = Path::new("/Applications/Google Chrome.app");
        let spawned = if chrome.exists() {
            Command::new("open")
                .args(["-na", "Google Chrome", "--args", &format!("--app={url}")])
                .spawn()
        } else {
            Command::new("open").arg(url).spawn()
        };
        if spawned.is_err() {
            eprintln!("[view] could not open a browser; open {url} manually");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = Command::new("xdg-open").arg(url).spawn() {
            eprintln!("[view] could not open a browser ({e}); open {url} manually");
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query_values() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        let q = parse_query("after_ms=1&tags=a,b");
        assert_eq!(q.get("after_ms").unwrap(), "1");
        assert_eq!(split_tags(q.get("tags")), vec!["a", "b"]);
    }
}
