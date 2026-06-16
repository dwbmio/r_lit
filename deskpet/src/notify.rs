//! Reminder protocol + transport.
//!
//! deskpet doubles as a *protocol-driven reminder surface*: any process can
//! pop a reminder above the mascot by sending it a one-line JSON message over a
//! loopback TCP socket — the same "send a message and it shows up" shape as an
//! LSP client talking to a server, but newline-delimited JSON instead of
//! Content-Length framing.
//!
//! Transport: a background thread owns a `TcpListener` on `127.0.0.1:<port>`
//! (loopback only — never reachable off-host) and reads **NDJSON** (one JSON
//! object per line) from each connection, so a single connection can stream
//! many reminders and many senders can connect at once. Parsed messages land in
//! a thread-safe `NotifyInbox` queue that a Bevy system drains each frame.
//!
//! The wire format is intentionally tiny and forward-compatible (unknown fields
//! are ignored, every field but `body` is optional):
//!
//! ```json
//! {"type":"notify","title":"Build","body":"3 errors","level":"error","duration_ms":8000}
//! {"type":"clear"}
//! ```

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use serde::Deserialize;

/// Default loopback port deskpet listens on for reminder messages. Overridable
/// via the `DESKPET_PORT` environment variable (shared by the running app and
/// the `deskpet send` CLI so they agree without configuration).
pub const DEFAULT_PORT: u16 = 47800;

/// Loopback address deskpet listens on / the CLI connects to.
pub fn listen_addr() -> String {
    let port = std::env::var("DESKPET_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    format!("127.0.0.1:{port}")
}

/// Reminder severity. Drives the bubble's accent color; unknown values from the
/// wire fall back to `Info` so a typo never drops a reminder.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Info,
    Success,
    Warn,
    Error,
}

impl Level {
    /// Bubble accent (border / title) color for this level.
    pub fn accent(self) -> [u8; 3] {
        match self {
            Level::Info => [86, 156, 255],
            Level::Success => [102, 187, 106],
            Level::Warn => [255, 183, 77],
            Level::Error => [239, 83, 80],
        }
    }
}

/// Custom deserializer for `level`: tolerate unknown strings (-> Info) instead
/// of failing the whole message.
fn de_level<'de, D>(d: D) -> Result<Level, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = Option::<String>::deserialize(d)?;
    Ok(match s.as_deref() {
        Some("success") => Level::Success,
        Some("warn") | Some("warning") => Level::Warn,
        Some("error") | Some("err") => Level::Error,
        _ => Level::Info,
    })
}

/// A decoded protocol message. `#[serde(tag = "type")]` makes `type` the
/// discriminator, matching the wire format above.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Incoming {
    /// Show a reminder above the mascot.
    Notify {
        #[serde(default)]
        title: Option<String>,
        body: String,
        #[serde(default, deserialize_with = "de_level")]
        level: Level,
        /// How long to keep it up; default derived from level + length.
        #[serde(default)]
        duration_ms: Option<u64>,
    },
    /// Dismiss the current reminder and flush the queue.
    Clear,
}

/// A reminder resolved for display (TTL computed, ready to render).
#[derive(Debug, Clone)]
pub struct Notice {
    pub title: Option<String>,
    pub body: String,
    pub level: Level,
    pub ttl: Duration,
}

impl Notice {
    fn from_parts(
        title: Option<String>,
        body: String,
        level: Level,
        duration_ms: Option<u64>,
    ) -> Self {
        // No explicit duration: scale with severity and reading length, clamped
        // to a sane window so a long error doesn't vanish before it's read.
        let ttl = match duration_ms {
            Some(ms) => Duration::from_millis(ms.clamp(800, 120_000)),
            None => {
                let base = match level {
                    Level::Info => 4_500,
                    Level::Success => 4_500,
                    Level::Warn => 6_500,
                    Level::Error => 9_000,
                };
                let per_char = 28 * body.chars().count() as u64;
                Duration::from_millis((base + per_char).min(20_000))
            }
        };
        Self {
            title,
            body,
            level,
            ttl,
        }
    }
}

/// Thread-safe hand-off from the listener thread to the Bevy world. The
/// listener pushes decoded messages; `drain_inbox` pops them each frame.
#[derive(Resource, Clone)]
pub struct NotifyInbox(Arc<Mutex<VecDeque<Incoming>>>);

impl NotifyInbox {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(VecDeque::new())))
    }

    fn push(&self, msg: Incoming) {
        if let Ok(mut q) = self.0.lock() {
            // Bound the buffer so a runaway sender can't grow memory without
            // bound; drop the oldest pending messages past the cap.
            const CAP: usize = 256;
            while q.len() >= CAP {
                q.pop_front();
            }
            q.push_back(msg);
        }
    }

    /// Move all pending messages out of the shared buffer.
    pub fn drain(&self) -> Vec<Incoming> {
        match self.0.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Live reminder state owned by the Bevy world: the one on screen, its
/// countdown, and the backlog waiting behind it.
#[derive(Resource, Default)]
pub struct NotifyState {
    pub current: Option<Notice>,
    pub timer: Option<Timer>,
    pub queue: VecDeque<Notice>,
}

impl NotifyState {
    /// True while a reminder is on screen (used to grow the hit-test region and
    /// keep the frame rate up so the bubble animates smoothly).
    pub fn showing(&self) -> bool {
        self.current.is_some()
    }

    /// Dismiss the current reminder; the next queued one shows next frame.
    pub fn dismiss(&mut self) {
        self.current = None;
        self.timer = None;
    }
}

/// Create the inbox resource and start the loopback listener thread. Returns
/// the inbox so the caller can register it with the app. Listener failures are
/// logged and non-fatal — the mascot still runs, just without remote reminders.
pub fn spawn_listener() -> NotifyInbox {
    let inbox = NotifyInbox::new();
    let thread_inbox = inbox.clone();
    let addr = listen_addr();

    std::thread::Builder::new()
        .name("deskpet-notify".into())
        .spawn(move || {
            let listener = match TcpListener::bind(&addr) {
                Ok(l) => l,
                Err(e) => {
                    warn!("deskpet: reminder listener disabled ({addr}: {e})");
                    return;
                }
            };
            info!("deskpet: reminder listener on {addr} (NDJSON)");
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let inbox = thread_inbox.clone();
                // One short-lived thread per connection: a connection can stream
                // many reminders, and a slow client can't block others.
                let _ = std::thread::Builder::new()
                    .name("deskpet-notify-conn".into())
                    .spawn(move || handle_conn(stream, inbox));
            }
        })
        .expect("spawn deskpet-notify thread");

    inbox
}

/// Build a display `Notice` from a `Notify` message's parts.
pub fn make_notice(
    title: Option<String>,
    body: String,
    level: Level,
    duration_ms: Option<u64>,
) -> Notice {
    Notice::from_parts(title, body, level, duration_ms)
}

/// Read NDJSON lines off one connection until EOF, pushing each decoded message
/// into the inbox. Malformed lines are skipped (logged) so one bad line can't
/// poison the stream.
fn handle_conn(stream: std::net::TcpStream, inbox: NotifyInbox) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Incoming>(line) {
            Ok(msg) => inbox.push(msg),
            Err(e) => warn!("deskpet: ignoring malformed reminder ({e}): {line}"),
        }
    }
}

// ---- CLI sender ------------------------------------------------------------

const SEND_HELP: &str = "\
deskpet send — push a reminder to a running deskpet

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
    DESKPET_PORT            Loopback port (default 47800)

EXAMPLES:
    deskpet send \"build finished\"
    deskpet send -t Build -l error -m \"3 errors in main.rs\"
    cargo build 2>&1 | tail -1 | deskpet send -t Build -l warn   # body from stdin
    deskpet send --clear";

/// Handle the `deskpet send` subcommand: build one protocol message from the
/// CLI args (or stdin) and write it as a single NDJSON line to the running
/// instance. Returns a process exit code.
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

    let addr = listen_addr();

    if clear {
        return write_line(&addr, &serde_json::json!({ "type": "clear" }).to_string());
    }

    // Body precedence: -m flag, then trailing positional args, then stdin (so
    // it composes in a pipe: `... | deskpet send -t Build`).
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

    let mut msg = serde_json::json!({ "type": "notify", "body": body });
    if let Some(t) = title {
        msg["title"] = serde_json::Value::String(t);
    }
    if let Some(l) = level {
        msg["level"] = serde_json::Value::String(l);
    }
    if let Some(d) = duration_ms {
        msg["duration_ms"] = serde_json::Value::from(d);
    }
    write_line(&addr, &msg.to_string())
}

fn read_stdin_body() -> Option<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return None; // interactive: don't block waiting on input
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

/// Connect to the running deskpet and write one NDJSON line. Returns an exit
/// code (0 ok; 1 connection/write failure with a friendly hint).
fn write_line(addr: &str, json: &str) -> i32 {
    match TcpStream::connect(addr) {
        Ok(mut stream) => {
            if let Err(e) = writeln!(stream, "{json}") {
                eprintln!("deskpet send: write failed: {e}");
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!(
                "deskpet send: could not reach deskpet at {addr} ({e}).\n\
                 Is deskpet running? (start it, then retry; set DESKPET_PORT to match.)"
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Incoming {
        serde_json::from_str(s).expect("valid protocol message")
    }

    #[test]
    fn notify_full() {
        let msg = parse(
            r#"{"type":"notify","title":"Build","body":"3 errors","level":"error","duration_ms":8000}"#,
        );
        match msg {
            Incoming::Notify {
                title,
                body,
                level,
                duration_ms,
            } => {
                assert_eq!(title.as_deref(), Some("Build"));
                assert_eq!(body, "3 errors");
                assert_eq!(level, Level::Error);
                assert_eq!(duration_ms, Some(8000));
            }
            _ => panic!("expected notify"),
        }
    }

    #[test]
    fn notify_minimal_defaults() {
        let msg = parse(r#"{"type":"notify","body":"hi"}"#);
        match msg {
            Incoming::Notify {
                title,
                level,
                duration_ms,
                ..
            } => {
                assert!(title.is_none());
                assert_eq!(level, Level::Info);
                assert!(duration_ms.is_none());
            }
            _ => panic!("expected notify"),
        }
    }

    #[test]
    fn unknown_level_falls_back_to_info() {
        let msg = parse(r#"{"type":"notify","body":"x","level":"bogus"}"#);
        match msg {
            Incoming::Notify { level, .. } => assert_eq!(level, Level::Info),
            _ => panic!("expected notify"),
        }
        // synonyms map sensibly
        let msg = parse(r#"{"type":"notify","body":"x","level":"warning"}"#);
        match msg {
            Incoming::Notify { level, .. } => assert_eq!(level, Level::Warn),
            _ => panic!("expected notify"),
        }
    }

    #[test]
    fn unknown_fields_ignored() {
        let msg = parse(r#"{"type":"notify","body":"x","sound":true,"icon":"y"}"#);
        assert!(matches!(msg, Incoming::Notify { .. }));
    }

    #[test]
    fn clear_message() {
        assert!(matches!(parse(r#"{"type":"clear"}"#), Incoming::Clear));
    }

    #[test]
    fn ttl_clamped_and_derived() {
        // explicit duration is clamped to the sane window
        let n = make_notice(None, "x".into(), Level::Info, Some(10));
        assert_eq!(n.ttl, Duration::from_millis(800));
        // derived TTL grows with severity
        let info = make_notice(None, "short".into(), Level::Info, None);
        let err = make_notice(None, "short".into(), Level::Error, None);
        assert!(err.ttl > info.ttl);
    }
}
