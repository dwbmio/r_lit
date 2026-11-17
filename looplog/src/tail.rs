//! `looplog tail`: turn a streaming source into local SQLite lines.
//!
//! Two entry points share one loop:
//! - `tail_command` — CLI, blocks until Ctrl-C
//! - `spawn_runner` — background runner owned by the viewer, start/stoppable
//!
//! Design notes:
//! - One run is created per start; reconnects append to it.
//! - Lines are buffered and flushed every ~150ms (or 200 lines).
//! - The line's own timestamp is preserved when the source provides one;
//!   `ingest_ms` always records the receive time.

use crate::db::{format_ms, NewRun, SqliteStorage};
use crate::error::Result;
use crate::protocol::{AppendLine, FinishRunRequest};
use crate::source::{parse_line, Parser, SourceSpec};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FLUSH_LINES: usize = 200;
const FLUSH_INTERVAL: Duration = Duration::from_millis(150);

/// Set by SIGINT/SIGTERM so CLI `tail` can close its run instead of leaving it
/// stuck in `running` after Ctrl-C.
static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
fn install_signal_handlers() {
    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }
    unsafe {
        let handler = on_signal as *const () as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

pub struct TailOptions {
    pub tag: String,
    pub kind: Option<String>,
    pub source: Option<String>,
    pub meta: BTreeMap<String, Value>,
    pub reconnect: bool,
    pub reconnect_max_secs: u64,
    pub spec: SourceSpec,
}

/// A running background source. Dropping it detaches the thread; call `stop`
/// (and optionally `join`) to shut it down deterministically.
pub struct RunnerHandle {
    pub name: String,
    pub tag: String,
    pub label: String,
    pub run_id: String,
    pub started_ms: i64,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl RunnerHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn join(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Start a source in the background. Returns once the run row exists, so the
/// caller can surface configuration errors immediately.
pub fn spawn_runner(
    name: &str,
    db_path: PathBuf,
    opts: TailOptions,
    command: Vec<String>,
) -> Result<RunnerHandle> {
    let spec = match &opts.spec {
        SourceSpec::Cmd { .. } => SourceSpec::Cmd { command },
        other => other.clone(),
    };
    let (argv, parser) = spec.build()?;
    let label = spec.label();

    let mut storage = SqliteStorage::open(&db_path)?;
    let run_id = storage.start_run(NewRun {
        tag: Some(opts.tag.clone()),
        source: Some(
            opts.source
                .clone()
                .unwrap_or_else(|| "looplog-tail".to_string()),
        ),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string()),
        argv: argv.clone(),
        client_id: None,
        kind: opts.kind.clone(),
        meta: opts.meta.clone(),
    })?;

    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(true));
    let started_ms = crate::db::now_ms();

    // Values the handle keeps after the closure takes ownership.
    let handle_tag = opts.tag.clone();
    let handle_run_id = run_id.clone();

    let thread_stop = stop.clone();
    let thread_running = running.clone();
    let thread_name = name.to_string();
    let thread_tag = opts.tag.clone();
    let thread_spec = spec.clone();
    let first_build = (argv, parser);

    let join = thread::Builder::new()
        .name(format!("looplog-tail-{name}"))
        .spawn(move || {
            let result = run_loop(
                &mut storage,
                &run_id,
                &thread_spec,
                first_build,
                &opts,
                &thread_stop,
                &thread_name,
                &thread_tag,
            );
            if let Err(e) = result {
                eprintln!("[tail {}] stopped with error: {}", thread_tag, e);
            }
            thread_running.store(false, Ordering::SeqCst);
        })?;

    Ok(RunnerHandle {
        name: name.to_string(),
        tag: handle_tag,
        label,
        run_id: handle_run_id,
        started_ms,
        stop,
        running,
        join: Some(join),
    })
}

/// CLI entry point: start and block until the source exits or Ctrl-C.
pub fn tail_command(db_path: &Path, opts: TailOptions, command: &[String]) -> Result<()> {
    install_signal_handlers();
    let mut handle = spawn_runner(
        "tail",
        db_path.to_path_buf(),
        opts,
        command.to_vec(),
    )?;
    eprintln!(
        "looplog tail → run={} tag={} source={} db={}",
        handle.run_id,
        handle.tag,
        handle.label,
        db_path.display()
    );
    handle.join();
    eprintln!("looplog tail → run={} closed", handle.run_id);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    storage: &mut SqliteStorage,
    run_id: &str,
    spec: &SourceSpec,
    first_build: (Vec<String>, Parser),
    opts: &TailOptions,
    local_stop: &AtomicBool,
    name: &str,
    tag: &str,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<AppendLine>();
    // Highest source timestamp stored so far; drives journald resume.
    let latest_ts = Arc::new(AtomicI64::new(i64::MIN));
    let mut backoff = 1u64;
    let mut attempts = 0u64;

    loop {
        if should_stop(local_stop) {
            break;
        }
        attempts += 1;

        // First run uses the configured prewarm; reconnects resume from the last
        // stored timestamp so a dropped ssh session neither loses nor re-ingests.
        let (argv, parser) = if attempts == 1 {
            first_build.clone()
        } else {
            let resume = match latest_ts.load(Ordering::Relaxed) {
                i64::MIN => None,
                ts => Some(ts),
            };
            match spec.build_resumed(resume) {
                Ok(built) => built,
                Err(e) => {
                    eprintln!("[tail {tag}] could not build resume command: {e}");
                    break;
                }
            }
        };

        // journald/nginx 走 ssh 时，stderr 是传输层诊断（Disconnected / Permission denied），
        // 写进业务日志会污染时间线并误报 error。cmd/file 的 stderr 属于业务输出，保留。
        let stderr_is_transport = matches!(
            spec,
            SourceSpec::Journald { remote: Some(_), .. } | SourceSpec::Nginx { remote: Some(_), .. }
        );
        match spawn_source(&argv, parser, &tx, latest_ts.clone(), stderr_is_transport, tag) {
            Ok(mut child) => {
                backoff = 1;
                eprintln!("[tail {tag}] source started (attempt {attempts}, {name})");
                let status = pump(storage, run_id, &rx, &mut child, local_stop)?;
                if should_stop(local_stop) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                eprintln!(
                    "[tail {tag}] source exited ({})",
                    describe_status(status.as_ref())
                );
            }
            Err(e) => {
                eprintln!("[tail {tag}] spawn failed: {e}");
                if !opts.reconnect {
                    return Err(e.into());
                }
            }
        }

        if !opts.reconnect || should_stop(local_stop) {
            break;
        }
        eprintln!("[tail {tag}] reconnecting in {backoff}s");
        thread::sleep(Duration::from_secs(backoff));
        backoff = (backoff * 2).min(opts.reconnect_max_secs.max(1));
    }

    let interrupted = should_stop(local_stop);
    storage.finish_run(
        run_id,
        &FinishRunRequest {
            status: Some(if interrupted { "interrupted" } else { "ok" }.to_string()),
            exit_code: Some(if interrupted { 130 } else { 0 }),
        },
    )?;
    Ok(())
}

fn should_stop(local: &AtomicBool) -> bool {
    STOP.load(Ordering::SeqCst) || local.load(Ordering::SeqCst)
}

fn spawn_source(
    argv: &[String],
    parser: Parser,
    tx: &Sender<AppendLine>,
    latest_ts: Arc<AtomicI64>,
    stderr_is_transport: bool,
    tag: &str,
) -> std::io::Result<Child> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(out) = child.stdout.take() {
        spawn_lines(out, "stdout", parser, tx.clone(), latest_ts.clone());
    }
    if let Some(err) = child.stderr.take() {
        if stderr_is_transport {
            spawn_stderr_tee(err, tag.to_string());
        } else {
            spawn_lines(err, "stderr", Parser::Raw, tx.clone(), latest_ts);
        }
    }
    Ok(child)
}

/// Drain `rx` into storage until the child exits. Returns the child status.
fn pump(
    storage: &mut SqliteStorage,
    run_id: &str,
    rx: &Receiver<AppendLine>,
    child: &mut Child,
    local_stop: &AtomicBool,
) -> Result<Option<ExitStatus>> {
    let mut batch: Vec<AppendLine> = Vec::new();
    let mut last_flush = Instant::now();

    loop {
        match rx.recv_timeout(FLUSH_INTERVAL) {
            Ok(line) => {
                batch.push(line);
                if batch.len() >= FLUSH_LINES || last_flush.elapsed() >= FLUSH_INTERVAL {
                    flush(storage, run_id, &mut batch)?;
                    last_flush = Instant::now();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !batch.is_empty() {
                    flush(storage, run_id, &mut batch)?;
                    last_flush = Instant::now();
                }
                if should_stop(local_stop) {
                    return Ok(child.try_wait()?);
                }
                if let Some(status) = child.try_wait()? {
                    let deadline = Instant::now() + Duration::from_millis(500);
                    while Instant::now() < deadline {
                        match rx.recv_timeout(Duration::from_millis(50)) {
                            Ok(line) => batch.push(line),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    flush(storage, run_id, &mut batch)?;
                    return Ok(Some(status));
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush(storage, run_id, &mut batch)?;
                return Ok(child.try_wait()?);
            }
        }
    }
}

fn flush(storage: &mut SqliteStorage, run_id: &str, batch: &mut Vec<AppendLine>) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    storage.append_lines(run_id, batch)?;
    batch.clear();
    Ok(())
}

fn spawn_lines<R>(
    reader: R,
    stream: &'static str,
    parser: Parser,
    tx: Sender<AppendLine>,
    latest_ts: Arc<AtomicI64>,
) where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                        buf.pop();
                    }
                    if buf.is_empty() {
                        continue;
                    }
                    let raw = String::from_utf8_lossy(&buf);
                    let Some(parsed) = parse_line(parser, stream, &raw) else {
                        continue;
                    };
                    if let Some(ts) = parsed.ts_ms {
                        latest_ts.fetch_max(ts, Ordering::Relaxed);
                    }
                    let line = AppendLine {
                        ts: parsed.ts_ms.map(format_ms),
                        stream: Some(stream.to_string()),
                        level: Some(parsed.level),
                        text: parsed.text,
                        meta: parsed.meta,
                        ..Default::default()
                    };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

/// 传输层 stderr：打到 view 的终端，不写库。
fn spawn_stderr_tee<R>(reader: R, tag: String)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(std::result::Result::ok) {
            eprintln!("[tail {tag} stderr] {line}");
        }
    });
}

fn describe_status(status: Option<&ExitStatus>) -> String {
    match status {
        Some(s) => match s.code() {
            Some(code) => format!("exit {code}"),
            None => "killed by signal".to_string(),
        },
        None => "no status".to_string(),
    }
}
