use log::{Level, LevelFilter, Log, Metadata, Record};
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

struct RunLog(Mutex<Vec<String>>);
static LOGGER: OnceLock<RunLog> = OnceLock::new();

impl Log for RunLog {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }
    fn log(&self, record: &Record) {
        let line = format!(
            "[{} {}] {}",
            humantime::format_rfc3339_seconds(SystemTime::now()),
            record.level(),
            record.args()
        );
        if let Ok(mut lines) = self.0.lock() {
            lines.push(line.clone());
        }
        // Keep stdout exclusively for the JSON envelope / command result.
        if record.level() <= Level::Info {
            eprintln!("{line}");
        }
    }
    fn flush(&self) {}
}

pub fn init() {
    let logger = LOGGER.get_or_init(|| RunLog(Mutex::new(Vec::new())));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

pub fn flush(path: &Path, command: &str) -> std::io::Result<()> {
    let lines = LOGGER
        .get()
        .and_then(|l| l.0.lock().ok().map(|mut b| std::mem::take(&mut *b)))
        .unwrap_or_default();
    let content = format!(
        "# mini_reg {}\n# command: {}\n{}\n",
        env!("CARGO_PKG_VERSION"),
        command,
        lines.join("\n")
    );
    std::fs::write(path, content)
}
