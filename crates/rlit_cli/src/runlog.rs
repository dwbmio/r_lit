//! Per-run log capture, persisted as a sidecar next to a tool's output.
//!
//! Every CLI invocation writes a fresh `<output>.log` file alongside the
//! artifact it produced, recording INFO/WARN/ERROR plus DEBUG details that
//! don't normally appear on stdout. The file overwrites on each run — its
//! purpose is "show me what just happened so I can review or report it", not
//! long-term archival.
//!
//! Flow per invocation:
//!   1. `rlit_cli::runlog::init()` at process start — installs the global logger.
//!   2. Subcommand runs; `log::info!`/`debug!`/`warn!`/`error!` fan out to
//!      console (`INFO+` in release, `DEBUG+` in dev; stdout normally, stderr
//!      when `--json` is active) AND an in-memory buffer.
//!   3. Caller computes the log path and calls `runlog::flush(path, header)`.
//!      The buffer is drained so a follow-up run in the same process starts clean.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

static LOGGER: OnceLock<&'static DualLogger> = OnceLock::new();

struct DualLogger {
    buffer: Mutex<Vec<String>>,
    console_level: Level,
}

impl Log for DualLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let line = format!(
            "[{} {}] {}",
            humantime::format_rfc3339_seconds(SystemTime::now()),
            record.level(),
            record.args()
        );

        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(line.clone());
        }

        if record.level() <= self.console_level {
            if crate::json_mode() {
                eprintln!("{}", line);
            } else {
                println!("{}", line);
            }
        }
    }

    fn flush(&self) {}
}

/// Install the dual logger. Idempotent — extra calls are no-ops.
pub fn init() {
    if LOGGER.get().is_some() {
        return;
    }

    let console_level = if cfg!(debug_assertions) {
        Level::Debug
    } else {
        Level::Info
    };

    let logger: &'static DualLogger = Box::leak(Box::new(DualLogger {
        buffer: Mutex::new(Vec::new()),
        console_level,
    }));

    let _ = LOGGER.set(logger);
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Debug);
}

/// Flush buffered log lines to `path`, prepending `# <line>` headers, and drain
/// the buffer so subsequent runs in the same process start fresh.
///
/// Best-effort: I/O failures print to stderr rather than aborting the caller.
pub fn flush(path: &Path, header: &[String]) {
    let Some(logger) = LOGGER.get() else {
        return;
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

/// Build the standard header lines using the active [`crate::Contract`] metadata
/// (falling back to the executable name if `CONTRACT.parse()` has not run).
pub fn standard_header() -> Vec<String> {
    if let Some(c) = crate::CONTRACT.get() {
        return standard_header_for(c.tool, c.version);
    }
    let bin = std::env::args()
        .next()
        .and_then(|a| {
            Path::new(&a)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "tool".to_string());
    standard_header_for(&bin, "0.0.0")
}

/// Build the standard header lines for an explicit `tool` and `version`.
pub fn standard_header_for(tool: &str, version: &str) -> Vec<String> {
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let argv: Vec<String> = std::env::args().collect();
    vec![
        format!("{} {} — run log", tool, version),
        format!("started: {}", now),
        format!("argv:    {}", shell_quote(&argv)),
    ]
}

fn shell_quote(args: &[String]) -> String {
    args.iter()
        .map(|a| {
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

        let tmp = std::env::temp_dir().join(format!("rlit_cli_runlog_{}.log", std::process::id()));
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
