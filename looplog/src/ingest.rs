use crate::db::{NewRun, SqliteStorage};
use crate::error::{Error, Result};
use crate::protocol::{AppendLine, FinishRunRequest};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

pub struct IngestOptions {
    pub tag: Option<String>,
    pub source: Option<String>,
    pub kind: Option<String>,
    pub meta: BTreeMap<String, Value>,
}

pub fn run_command(db_path: &Path, opts: IngestOptions, command: &[String]) -> Result<i32> {
    if command.is_empty() {
        return Err(Error::MissingCommand);
    }

    let cwd = std::env::current_dir()?.display().to_string();
    let mut storage = SqliteStorage::open(db_path)?;
    let run_id = storage.start_run(NewRun {
        tag: opts.tag,
        source: opts.source.or_else(|| Some("looplog-run".to_string())),
        cwd: Some(cwd),
        argv: command.to_vec(),
        client_id: None,
        kind: opts.kind,
        meta: opts.meta,
    })?;

    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Message("could not capture stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Message("could not capture stderr".to_string()))?;

    let (tx, rx) = mpsc::channel::<AppendLine>();
    spawn_reader(stdout, "stdout", tx.clone());
    spawn_reader(stderr, "stderr", tx);

    for line in rx {
        storage.append_lines(&run_id, &[line])?;
    }

    let status = child.wait()?;
    let exit_code = status.code().unwrap_or(1);
    storage.finish_run(
        &run_id,
        &FinishRunRequest {
            status: Some(if exit_code == 0 { "ok" } else { "failed" }.to_string()),
            exit_code: Some(exit_code),
        },
    )?;
    Ok(exit_code)
}

pub fn push_stdin(db_path: &Path, opts: IngestOptions, level: Option<String>) -> Result<String> {
    let cwd = std::env::current_dir()?.display().to_string();
    let mut storage = SqliteStorage::open(db_path)?;
    let run_id = storage.start_run(NewRun {
        tag: opts.tag,
        source: opts.source.or_else(|| Some("looplog-push".to_string())),
        cwd: Some(cwd),
        argv: Vec::new(),
        client_id: None,
        kind: opts.kind,
        meta: opts.meta,
    })?;

    let stdin = io::stdin();
    let mut batch = Vec::new();
    for line in stdin.lock().lines() {
        batch.push(AppendLine {
            stream: Some("stdin".to_string()),
            level: level.clone().or_else(|| Some("info".to_string())),
            text: line?,
            ..Default::default()
        });
        if batch.len() >= 100 {
            storage.append_lines(&run_id, &batch)?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        storage.append_lines(&run_id, &batch)?;
    }
    storage.finish_run(
        &run_id,
        &FinishRunRequest {
            status: Some("ok".to_string()),
            exit_code: Some(0),
        },
    )?;
    Ok(run_id)
}

fn spawn_reader<R>(reader: R, stream: &'static str, tx: mpsc::Sender<AppendLine>)
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let mut out = if stream == "stderr" {
            Box::new(io::stderr()) as Box<dyn Write + Send>
        } else {
            Box::new(io::stdout()) as Box<dyn Write + Send>
        };
        for line in BufReader::new(reader).lines() {
            let Ok(text) = line else {
                break;
            };
            let _ = writeln!(out, "{}", text);
            let _ = tx.send(AppendLine {
                stream: Some(stream.to_string()),
                level: Some(if stream == "stderr" { "error" } else { "info" }.to_string()),
                text,
                ..Default::default()
            });
        }
    });
}
