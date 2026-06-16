//! Per-run log capture, persisted as a sidecar next to the tool's output.
//!
//! Every CLI invocation writes a fresh `<output>.log` file alongside the
//! artifact it produced, recording the full INFO/WARN/ERROR stream plus DEBUG
//! details that don't normally appear on stdout. The file overwrites on each
//! run — its purpose is "show me what just happened so I can review or report
//! it", not long-term archival.
//!
//! Why a custom dual-sink logger instead of `fern + chain(File)`: we don't
//! always know the output path up front, and buffering everything in memory
//! then flushing once at the end is simpler than juggling deferred chains. It
//! also lets stdout stay at INFO+ while the sidecar keeps the full DEBUG trail.
//!
//! This file is intentionally identical across the repo's short-running CLI
//! tools (see `mj_atlas/src/runlog.rs`); the only crate-specific bits — tool
//! name and version — come from `CARGO_PKG_*` so it can be copied verbatim.
//!
//! Flow per invocation:
//!   1. `runlog::init()` at process start — installs the global logger.
//!   2. Subcommand runs; `log::info!`/`debug!`/`warn!`/`error!` fan out to
//!      stdout (INFO+ in release, DEBUG+ in dev) AND an in-memory buffer.
//!   3. The caller computes the log path and calls `runlog::flush(path, hdr)`.
//!      The buffer is drained so a follow-up run in the same process is clean.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Global handle to the installed logger. Only set once `init()` has run.
static LOGGER: OnceLock<&'static DualLogger> = OnceLock::new();

struct DualLogger {
    buffer: Mutex<Vec<String>>,
    /// Levels at and above this go to stdout.
    stdout_level: Level,
}

impl Log for DualLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // Capture everything; per-message filtering happens in `log()`.
        true
    }

    fn log(&self, record: &Record) {
        let line = format!(
            "[{} {}] {}",
            humantime::format_rfc3339_seconds(SystemTime::now()),
            record.level(),
            record.args()
        );

        // The buffer captures every level so the sidecar has full detail.
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(line.clone());
        }

        // Stdout shows INFO+ in release, DEBUG+ in dev.
        if record.level() <= self.stdout_level {
            println!("{}", line);
        }
    }

    fn flush(&self) {}
}

/// Install the dual logger. Idempotent — extra calls are no-ops.
pub fn init() {
    if LOGGER.get().is_some() {
        return;
    }

    let stdout_level = if cfg!(debug_assertions) {
        Level::Debug
    } else {
        Level::Info
    };

    // `Box::leak` gives the `'static` reference the `log` crate requires; we
    // only ever leak one logger per process, so the cost is bounded.
    let logger: &'static DualLogger = Box::leak(Box::new(DualLogger {
        buffer: Mutex::new(Vec::new()),
        stdout_level,
    }));

    let _ = LOGGER.set(logger);
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Debug);
}

/// Flush the buffered log lines to `path`, prepending the header (one `# `
/// comment line per element). Resets the buffer afterwards so follow-up runs
/// in the same process start clean.
///
/// Best-effort: any I/O error falls back to stderr rather than aborting.
pub fn flush(path: &Path, header: &[String]) {
    let logger = match LOGGER.get() {
        Some(l) => l,
        None => return, // init() never ran; nothing to flush.
    };

    let lines = match logger.buffer.lock() {
        Ok(mut buf) => std::mem::take(&mut *buf),
        Err(_) => return,
    };

    let mut content = String::new();
    for hl in header {
        content.push_str("# ");
        content.push_str(hl);
        content.push('\n');
    }
    if !header.is_empty() {
        content.push('\n');
    }
    for line in &lines {
        content.push_str(line);
        content.push('\n');
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Err(e) = std::fs::write(path, content) {
        eprintln!("[runlog] could not write log to {}: {}", path.display(), e);
    }
}

/// Build the standard header lines: tool + version, timestamp, command line.
pub fn standard_header() -> Vec<String> {
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let argv: Vec<String> = std::env::args().collect();
    vec![
        format!(
            "{} {} — run log",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ),
        format!("started: {}", now),
        format!("argv:    {}", shell_quote(&argv)),
    ]
}

fn shell_quote(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            // Quote anything with whitespace or shell metas so the header
            // stays pasteable into a terminal for repro.
            if a.is_empty()
                || a.chars()
                    .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | '$' | '`'))
            {
                let escaped = a.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{}\"", escaped)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{debug, info};

    #[test]
    fn buffered_lines_round_trip_through_flush() {
        init();
        info!("test-info-{}", 1);
        debug!("test-debug-{}", 2);

        let tmp = std::env::temp_dir().join(format!("runlog_test_{}.log", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        flush(&tmp, &["unit test header".to_string()]);

        let written = std::fs::read_to_string(&tmp).unwrap();
        assert!(written.contains("# unit test header"), "header missing: {}", written);
        assert!(written.contains("test-info-1"), "info line missing: {}", written);
        assert!(
            written.contains("test-debug-2"),
            "debug line missing (buffer should keep DEBUG): {}",
            written
        );

        // After flush the buffer must be empty so the next run is clean.
        flush(&tmp, &[]);
        let second = std::fs::read_to_string(&tmp).unwrap();
        assert!(
            !second.contains("test-info-1"),
            "buffer should have been drained after first flush"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn shell_quote_handles_spaces_and_quotes() {
        let args = vec![
            "tool".to_string(),
            "/tmp/has spaces".to_string(),
            "weird\"name".to_string(),
        ];
        let out = shell_quote(&args);
        assert!(out.contains("\"/tmp/has spaces\""));
        assert!(out.contains("\"weird\\\"name\""));
    }
}
