//! Per-run log capture, persisted as a sidecar next to the atlas.
//!
//! Every CLI invocation (and every GUI pack) writes a fresh `<atlas>.log`
//! file alongside the output it touched, recording the full INFO/WARN/ERROR
//! stream plus DEBUG details that don't normally appear on stdout. The file
//! overwrites on each run — purpose is "show me what just happened so I can
//! review or report it", not long-term archival.
//!
//! Why a custom logger instead of `fern + chain(File)`: we don't know the
//! output path until the pack pipeline has resolved options and (for
//! incremental) loaded the manifest. Buffering everything in memory and
//! flushing once at the end is simpler than juggling deferred chains.
//!
//! Flow per invocation:
//!   1. `runlog::init()` at process start — installs the global logger.
//!   2. Subcommand runs; `log::info!`/`debug!`/`warn!`/`error!` calls fan
//!      out to stdout (matching the prior fern behavior) AND a Vec buffer.
//!   3. Subcommand returns; the caller computes the log path and calls
//!      `runlog::flush(path, header_lines)`. Buffer is drained so a follow-up
//!      pack in the same process (the GUI does this) starts fresh.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Global handle to the installed logger. Only set when `init()` has run.
static LOGGER: OnceLock<&'static DualLogger> = OnceLock::new();

struct DualLogger {
    buffer: Mutex<Vec<String>>,
    /// Levels at and above this go to stdout (preserves prior fern behavior).
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

        // Buffer captures every level so the file always has full detail.
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(line.clone());
        }

        // Stdout matches the prior fern policy (info+ in release, debug+ in dev).
        if record.level() <= self.stdout_level {
            // Errors / warnings on stderr would be cleaner, but that breaks
            // existing CI consumers that grep stdout for log markers. Keep
            // the stdout-only behavior we shipped in v0.1.
            println!("{}", line);
        }
    }

    fn flush(&self) {}
}

/// Install the dual logger. Idempotent — extra calls are no-ops so the GUI
/// thread can safely call it without coordinating with `main`.
pub fn init() {
    if LOGGER.get().is_some() {
        return;
    }

    let stdout_level = if cfg!(debug_assertions) {
        Level::Debug
    } else {
        Level::Info
    };

    // Box::leak gives us a 'static reference, which the `log` crate requires.
    // We only ever leak one logger per process; total cost is bounded.
    let logger: &'static DualLogger = Box::leak(Box::new(DualLogger {
        buffer: Mutex::new(Vec::new()),
        stdout_level,
    }));

    let _ = LOGGER.set(logger);
    let _ = log::set_logger(logger);
    // Max level must accept everything we want the buffer to capture.
    log::set_max_level(LevelFilter::Debug);
}

/// Flush the buffered log lines to `path`, prepending the header (one line
/// per element, no trailing newline added). Resets the buffer afterwards so
/// follow-up runs in the same process start clean (used by the GUI).
///
/// Best-effort: any I/O error is reported as a warning on the same logger,
/// then swallowed — losing the log shouldn't abort the user's pack.
pub fn flush(path: &Path, header: &[String]) {
    let logger = match LOGGER.get() {
        Some(l) => l,
        None => return, // init() never ran; nothing to flush.
    };

    let lines = {
        match logger.buffer.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => return,
        }
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
        // Don't recurse through `log::warn!` because that would re-buffer
        // post-flush. Fall back to stderr.
        eprintln!(
            "[runlog] could not write log to {}: {}",
            path.display(),
            e
        );
    }
}

/// Build the standard header lines: timestamp, command line, tool version.
/// Subcommands can append extra lines (e.g. resolved PackOptions summary).
pub fn standard_header() -> Vec<String> {
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let argv: Vec<String> = std::env::args().collect();
    vec![
        format!("mj_atlas {} — run log", env!("CARGO_PKG_VERSION")),
        format!("started: {}", now),
        format!("argv:    {}", shell_quote(&argv)),
    ]
}

fn shell_quote(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            // Quote anything that contains whitespace or shell metas — keeps
            // the header pasteable into a terminal for repro.
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
        // Two extra info messages — they should all land in the buffer
        // (and on stdout, but the test process discards that anyway).
        info!("test-info-{}", 1);
        debug!("test-debug-{}", 2);

        let tmp = std::env::temp_dir().join(format!(
            "mj_atlas_runlog_test_{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        flush(&tmp, &["unit test header".to_string()]);

        let written = std::fs::read_to_string(&tmp).unwrap();
        assert!(
            written.contains("# unit test header"),
            "header missing: {}",
            written
        );
        assert!(
            written.contains("test-info-1"),
            "info line missing: {}",
            written
        );
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
            "mj_atlas".to_string(),
            "pack".to_string(),
            "/tmp/has spaces".to_string(),
            "--output".to_string(),
            "weird\"name".to_string(),
        ];
        let out = shell_quote(&args);
        assert!(out.contains("\"/tmp/has spaces\""));
        assert!(out.contains("\"weird\\\"name\""));
    }
}
