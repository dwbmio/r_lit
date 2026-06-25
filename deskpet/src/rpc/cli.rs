//! CLI subcommand entry points. Two surfaces:
//!
//! - `deskpet send <args>` — legacy reminder CLI, kept as an alias for
//!   `deskpet call notification/show` so existing shell scripts / muscle
//!   memory keep working.
//! - `deskpet call <method> -p '<json>'` — generic dispatcher for any
//!   registered method. Returns the raw JSON-RPC response on stdout.

use std::io::{IsTerminal, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use super::server::listen_addr as ndjson_addr;

/// Per-process request id counter. Each CLI invocation gets a unique
/// starting point (Unix nanos at startup), then increments per call.
/// Combined with the nanos prefix, ids are globally unique within the
/// uptime of this CLI process — useful for log correlation when multiple
/// CLIs run concurrently against the same deskpet.
fn next_request_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // Init once with current Unix nanos; subsequent calls just increment.
    // (The first caller sees a high id; later callers get +1, +2, ...)
    let init = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let prev = COUNTER.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |_| {
        Some(init.saturating_add(1))
    });
    if prev.is_err() {
        // First call: was 0, now init+1. Return init+1 to match.
        init + 1
    } else {
        COUNTER.fetch_add(1, Ordering::SeqCst) + 1
    }
}

// ---- deskpet send ----------------------------------------------------------

const SEND_HELP: &str = "\
deskpet send — push a reminder to a running deskpet (alias for `call notification/show`)

USAGE:
    deskpet send [OPTIONS] [BODY...]

OPTIONS:
    -m, --body <TEXT>       Reminder body (or pass it as trailing args)
    -t, --title <TEXT>      Optional bold title line
    -l, --level <LEVEL>     info | success | warn | error   (default: info)
    -d, --duration <MS>     How long to show it (default: derived from level)
        --clear             Dismiss the current reminder instead of adding one
    -h, --help              Show this help

ENV:
    DESKPET_RPC_PORT        Loopback port (default 47800)

EXAMPLES:
    deskpet send \"build finished\"
    deskpet send -t Build -l error -m \"3 errors in main.rs\"
    cargo build 2>&1 | tail -1 | deskpet send -t Build -l warn
    deskpet send --clear

NOTE: prefer `deskpet call notification/show -p '...'` for new scripts.
";

pub fn send_cli(args: &[String]) -> i32 {
    let mut title: Option<String> = None;
    let mut level: Option<String> = None;
    let mut duration_ms: Option<u64> = None;
    let mut body_flag: Option<String> = None;
    let mut clear = false;
    let mut positional: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{SEND_HELP}");
                return 0;
            }
            "--clear" => clear = true,
            "-t" | "--title" => title = it.next().cloned(),
            "-l" | "--level" => level = it.next().cloned(),
            "-m" | "--body" | "--message" => body_flag = it.next().cloned(),
            "-d" | "--duration" => {
                duration_ms = it.next().and_then(|s| s.parse::<u64>().ok());
            }
            other => positional.push(other.to_string()),
        }
    }

    let addr = ndjson_addr();

    if clear {
        let req = json!({
            "id": next_request_id(),
            "method": "notification/clear",
            "params": {}
        });
        return write_and_print(&addr, &req.to_string(), false);
    }

    // Body precedence: -m flag, then trailing positional args, then stdin.
    let body = body_flag
        .or_else(|| {
            if positional.is_empty() {
                None
            } else {
                Some(positional.join(" "))
            }
        })
        .or_else(read_stdin_body)
        .unwrap_or_default();

    if body.trim().is_empty() {
        eprintln!("deskpet send: empty reminder body (see --help)");
        return 2;
    }

    let mut msg = json!({
        "id": next_request_id(),
        "method": "notification/show",
        "params": { "body": body }
    });
    if let Some(t) = title {
        msg["params"]["title"] = serde_json::Value::String(t);
    }
    if let Some(l) = level {
        msg["params"]["level"] = serde_json::Value::String(l);
    }
    if let Some(d) = duration_ms {
        msg["params"]["duration_ms"] = serde_json::Value::from(d);
    }
    write_and_print(&addr, &msg.to_string(), false)
}

// ---- deskpet call ----------------------------------------------------------

const CALL_HELP: &str = "\
deskpet call — invoke any RPC method on a running deskpet

USAGE:
    deskpet call <METHOD> [OPTIONS]
    deskpet call --batch <FILE>

ARGS:
    <METHOD>                Method name, e.g. notification/show, pet/state, help/methods

OPTIONS:
    -p, --params <JSON>     Method params as a JSON object (default: {})
        --id <N>            Override the request id (default: per-process unique counter)
        --batch <FILE>      Send a JSON-RPC 2.0 batch from FILE (JSON array)
        --json              Pretty-print the JSON response
    -q, --quiet             Suppress output; exit code only
    -h, --help              Show this help

ENV:
    DESKPET_RPC_PORT        Loopback NDJSON port (default 47800)
    DESKPET_HTTP_PORT       Loopback HTTP port (default 47801, used by --batch)

EXAMPLES:
    deskpet call help/methods --json
    deskpet call notification/show -p '{\"body\":\"hi\",\"level\":\"info\"}'
    deskpet call pet/state --json
    deskpet call pet/control -p '{\"action\":\"hop\"}'
    deskpet call --batch batch.json --json
";

pub fn call_cli(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("{CALL_HELP}");
        return 2;
    }
    let mut method: Option<String> = None;
    let mut params_str: Option<String> = None;
    let mut explicit_id: Option<u64> = None;
    let mut pretty = false;
    let mut quiet = false;
    let mut batch_file: Option<String> = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{CALL_HELP}");
                return 0;
            }
            "-p" | "--params" => params_str = it.next().cloned(),
            "--id" => {
                let v = match it.next().and_then(|s| s.parse::<u64>().ok()) {
                    Some(n) => n,
                    None => {
                        eprintln!("deskpet call: --id requires a non-negative integer");
                        return 2;
                    }
                };
                explicit_id = Some(v);
            }
            "--batch" => batch_file = it.next().cloned(),
            "--json" => pretty = true,
            "-q" | "--quiet" => quiet = true,
            other => {
                if method.is_some() {
                    eprintln!("deskpet call: unexpected positional argument '{other}'");
                    return 2;
                }
                method = Some(other.to_string());
            }
        }
    }

    // Batch mode: read JSON array from file, send as JSON-RPC 2.0 batch.
    if let Some(path) = batch_file {
        return call_batch(&path, pretty && !quiet);
    }

    let Some(method) = method else {
        eprintln!("deskpet call: missing <METHOD> argument (or use --batch <file>)");
        return 2;
    };

    let params: serde_json::Value = match params_str {
        None => json!({}),
        Some(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("deskpet call: --params is not valid JSON: {e}");
                return 2;
            }
        },
    };

    let id = explicit_id.unwrap_or_else(next_request_id);
    let req = json!({
        "id": id,
        "method": method,
        "params": params,
    });

    write_and_print(&ndjson_addr(), &req.to_string(), pretty && !quiet)
}

/// JSON-RPC 2.0 batch over HTTP. Reads a JSON array of `{id, method, params}`
/// from `path`, POSTs to `/batch`, prints the array of responses.
fn call_batch(path: &str, pretty: bool) -> i32 {
    let body = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("deskpet call --batch: read {path}: {e}");
            return 2;
        }
    };
    let array: serde_json::Value = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) if v.is_array() => v,
        Ok(_) => {
            eprintln!("deskpet call --batch: file must contain a JSON array of request objects");
            return 2;
        }
        Err(e) => {
            eprintln!("deskpet call --batch: invalid JSON: {e}");
            return 2;
        }
    };

    // Stamp auto-ids where missing so callers don't have to manage ids.
    let stamped = if let serde_json::Value::Array(items) = array {
        serde_json::Value::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(i, mut item)| {
                    if let serde_json::Value::Object(ref mut obj) = item {
                        if !obj.contains_key("id") {
                            obj.insert("id".into(), json!(next_request_id() + i as u64));
                        }
                    }
                    item
                })
                .collect(),
        )
    } else {
        unreachable!("checked is_array above")
    };

    let resp = match post_http("/batch", &stamped.to_string()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("deskpet call --batch: HTTP request failed: {e}");
            return 1;
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&resp) {
        Ok(v) => v,
        Err(_) => {
            println!("{resp}");
            return 1;
        }
    };

    let any_error = if let serde_json::Value::Array(items) = &parsed {
        items.iter().any(|i| i.get("error").is_some())
    } else {
        false
    };

    if pretty {
        println!(
            "{}",
            serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| resp.clone())
        );
    } else {
        println!("{resp}");
    }
    if any_error {
        1
    } else {
        0
    }
}

/// Minimal HTTP POST helper (no reqwest dep — just std::net + manual framing).
/// Used by the batch CLI; single-call goes through NDJSON.
fn post_http(path: &str, body: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    // Port: prefer the HTTP_RPC_PORT env var or default 47801.
    let port: u16 = std::env::var("DESKPET_HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(47801);
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("connect: {e}"))?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).map_err(|e| format!("write: {e}"))?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|e| format!("read: {e}"))?;
    // Strip HTTP headers — find blank line separator.
    let body_start = raw
        .find("\r\n\r\n")
        .ok_or_else(|| "no body separator".to_string())?
        + 4;
    Ok(raw[body_start..].to_string())
}

fn write_and_print(addr: &str, line: &str, pretty: bool) -> i32 {
    use std::io::BufRead;
    let mut stream = match TcpStream::connect(addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "deskpet: could not reach deskpet at {addr} ({e}).\n\
                 Is deskpet running? (start it, then retry; set DESKPET_RPC_PORT to match.)"
            );
            return 1;
        }
    };
    if let Err(e) = writeln!(stream, "{line}") {
        eprintln!("deskpet: write failed: {e}");
        return 1;
    }
    // Read ONE NDJSON line (the server keeps the connection open for more
    // requests, so `read_to_string` would block forever waiting for EOF).
    let mut reader = std::io::BufReader::new(stream);
    let mut buf = String::new();
    if let Err(e) = reader.read_line(&mut buf) {
        eprintln!("deskpet: read failed: {e}");
        return 1;
    }
    let line = buf.trim();

    if pretty {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(line.into())),
            Err(_) => println!("{line}"),
        }
    } else {
        println!("{line}");
    }

    // Exit 0 on `result`, exit 1 on `error`. Useful in shell `&&` chains.
    if line.contains("\"error\"") && !line.contains("\"result\"") {
        1
    } else {
        0
    }
}

fn read_stdin_body() -> Option<String> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// `ExitCode` is referenced via the function signature on some platforms but
// we currently return raw i32. Keep the import alive for future use.
#[allow(dead_code)]
fn _exitcode_marker(_: ExitCode) {}