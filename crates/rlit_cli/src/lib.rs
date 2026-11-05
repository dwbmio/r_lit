//! Shared machine-call contract for r_lit CLIs.
//!
//! Every short-running r_lit tool links this crate so AI agents and the
//! titan-wall executor can call any of them the same way:
//!
//! * `--json` is global. With it, stdout carries **exactly one** JSON document
//!   (the [envelope](finish)) — on success *and* on failure. Human logs go to
//!   stderr.
//! * Exit codes are fixed: see [`ErrorKind`].
//! * `<tool> __schema` prints a machine-readable description of every leaf
//!   subcommand (JSON Schema for its arguments, plus how to map them back to
//!   argv, which paths are inputs/outputs, side effects and required env).
//!
//! The full written contract lives in `docs/cli-contract.md` at the repo root.
//!
//! Typical `main`:
//!
//! ```ignore
//! static CONTRACT: rlit_cli::Contract = rlit_cli::Contract {
//!     tool: env!("CARGO_PKG_NAME"),
//!     version: env!("CARGO_PKG_VERSION"),
//!     commands: &[ /* CommandSpec per leaf subcommand */ ],
//! };
//!
//! fn main() {
//!     let cli: Cli = CONTRACT.parse();
//!     let result = run(&cli);            // calls rlit_cli::set_data(..) etc.
//!     rlit_cli::finish(result, |e| e.kind());
//! }
//! ```

mod schema;

use serde::Serialize;
use serde_json::{json, Value};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

pub use schema::{check_contract, schema_for};

/// Version of the envelope / schema contract. Bump on breaking changes.
pub const CONTRACT_VERSION: u32 = 1;

/// Name of the hidden schema-export subcommand.
pub const SCHEMA_COMMAND: &str = "__schema";

// ---------------------------------------------------------------------------
// Static per-tool metadata
// ---------------------------------------------------------------------------

/// How a path argument is used. Lets an executor stage inputs into a sandbox
/// and rewrite outputs into its work dir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Io {
    InputFile,
    InputDir,
    /// File or directory, decided at runtime.
    InputPath,
    OutputFile,
    OutputDir,
    /// Output base name without extension; the tool derives several files
    /// from it (e.g. `atlas` → `atlas.png` + `atlas.json`).
    OutputStem,
    /// A file that is read and rewritten in place.
    InOutFile,
}

impl Io {
    pub fn as_str(self) -> &'static str {
        match self {
            Io::InputFile => "input-file",
            Io::InputDir => "input-dir",
            Io::InputPath => "input-path",
            Io::OutputFile => "output-file",
            Io::OutputDir => "output-dir",
            Io::OutputStem => "output-stem",
            Io::InOutFile => "inout-file",
        }
    }
}

/// Side-effect class of a leaf command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effects {
    /// Reads inputs, writes nothing. Safe to run any time.
    ReadOnly,
    /// Deterministic: same inputs + args → same outputs; only writes the
    /// declared output paths. Cacheable.
    Pure,
    /// Writes local files beyond (or instead of) a pure output, or depends on
    /// local machine state (installed fonts, user config, …).
    LocalWrite,
    /// Talks to a remote service (network, object storage, paid API).
    External,
}

impl Effects {
    pub fn as_str(self) -> &'static str {
        match self {
            Effects::ReadOnly => "read-only",
            Effects::Pure => "pure",
            Effects::LocalWrite => "local-write",
            Effects::External => "external",
        }
    }
}

/// Extra facts about one leaf subcommand that clap cannot express.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// Space-separated subcommand path, e.g. `"pack"` or `"cache clean"`.
    /// Use `""` for a tool without subcommands.
    pub path: &'static str,
    pub effects: Effects,
    /// `(arg id, io)` — arg id is the clap id (the Rust field name).
    pub io: &'static [(&'static str, Io)],
    /// Environment variables the command reads (credentials, endpoints…).
    pub env: &'static [&'static str],
    /// `Some(description)` when the command reads its main input from stdin.
    pub stdin: Option<&'static str>,
    /// One-line note for agents (preconditions, gotchas). Empty for none.
    pub note: &'static str,
    /// The command only exists under some cargo feature (e.g. `gui`);
    /// [`check_contract`] won't complain when it is absent.
    pub feature_gated: bool,
}

impl CommandSpec {
    /// Convenience constructor for the common case.
    pub const fn new(path: &'static str, effects: Effects) -> Self {
        CommandSpec {
            path,
            effects,
            io: &[],
            env: &[],
            stdin: None,
            note: "",
            feature_gated: false,
        }
    }
    pub const fn feature_gated(mut self) -> Self {
        self.feature_gated = true;
        self
    }
    pub const fn io(mut self, io: &'static [(&'static str, Io)]) -> Self {
        self.io = io;
        self
    }
    pub const fn env(mut self, env: &'static [&'static str]) -> Self {
        self.env = env;
        self
    }
    pub const fn stdin(mut self, what: &'static str) -> Self {
        self.stdin = Some(what);
        self
    }
    pub const fn note(mut self, note: &'static str) -> Self {
        self.note = note;
        self
    }
}

/// Per-tool contract declaration. Usually a `static`.
#[derive(Debug)]
pub struct Contract {
    pub tool: &'static str,
    pub version: &'static str,
    pub commands: &'static [CommandSpec],
}

impl Contract {
    pub(crate) fn spec(&self, path: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|c| c.path == path)
    }

    /// Parse argv into `C`, honouring the contract:
    ///
    /// * `<tool> __schema` prints the schema and exits 0.
    /// * Usage errors under `--json` print an error envelope (code `usage`,
    ///   exit 2) instead of clap's human text.
    /// * Records json mode + the resolved subcommand path for [`finish`].
    pub fn parse<C: clap::Parser>(&'static self) -> C {
        self.parse_from(std::env::args_os())
    }

    pub fn parse_from<C, I, T>(&'static self, args: I) -> C
    where
        C: clap::Parser,
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let args: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
        let _ = CONTRACT.set(self);

        if args.get(1).map(|a| a == SCHEMA_COMMAND).unwrap_or(false) {
            let schema = schema_for(self, &C::command());
            println!("{}", pretty(&schema));
            std::process::exit(0);
        }

        // Detect --json before clap runs so usage errors can be enveloped.
        let wants_json = args
            .iter()
            .skip(1)
            .take_while(|a| *a != "--")
            .any(|a| a == "--json");
        JSON.store(wants_json, Ordering::SeqCst);

        let mut cmd = C::command();
        // Best-effort command path so even usage errors say which command.
        set_command(guess_path(&cmd, &args));
        let matches = match cmd.try_get_matches_from_mut(&args) {
            Ok(m) => m,
            Err(e) => usage_error(e),
        };
        set_command(subcommand_path(&matches));
        if let Some(true) = matches.try_get_one::<bool>("json").ok().flatten().copied() {
            JSON.store(true, Ordering::SeqCst);
        }
        match C::from_arg_matches(&matches) {
            Ok(c) => c,
            Err(e) => usage_error(e.with_cmd(&cmd)),
        }
    }
}

fn usage_error(e: clap::Error) -> ! {
    use clap::error::ErrorKind as K;
    let informational = matches!(
        e.kind(),
        K::DisplayHelp | K::DisplayVersion | K::DisplayHelpOnMissingArgumentOrSubcommand
    );
    if informational || !json_mode() {
        e.exit();
    }
    let message = e.render().to_string();
    let message = message.trim().trim_start_matches("error: ").to_string();
    set_failure(ErrorKind::Usage, message);
    exit_now()
}

fn guess_path(cmd: &clap::Command, args: &[std::ffi::OsString]) -> String {
    let mut parts = Vec::new();
    let mut cur = cmd;
    for a in args.iter().skip(1) {
        let Some(a) = a.to_str() else { break };
        if a.starts_with('-') {
            continue;
        }
        match cur.find_subcommand(a) {
            Some(sub) => {
                parts.push(sub.get_name().to_string());
                cur = sub;
            }
            None => break,
        }
    }
    parts.join(" ")
}

fn subcommand_path(m: &clap::ArgMatches) -> String {
    let mut parts = Vec::new();
    let mut cur = m;
    while let Some((name, sub)) = cur.subcommand() {
        parts.push(name.to_string());
        cur = sub;
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Error kinds / exit codes
// ---------------------------------------------------------------------------

/// Error classes with fixed exit codes. `error.code` in the envelope is the
/// kind's [`code`](ErrorKind::code) unless a tool-specific code is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Exit 1 — unexpected failure while running.
    Runtime,
    /// Exit 2 — bad arguments / flag combination.
    Usage,
    /// Exit 3 — an input is missing, unreadable or malformed.
    Input,
    /// Exit 4 — a remote service failed (network, auth, storage, API).
    External,
    /// Exit 5 — batch finished but some items failed; `data` is complete.
    Partial,
}

impl ErrorKind {
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Runtime => 1,
            ErrorKind::Usage => 2,
            ErrorKind::Input => 3,
            ErrorKind::External => 4,
            ErrorKind::Partial => 5,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Runtime => "runtime",
            ErrorKind::Usage => "usage",
            ErrorKind::Input => "input",
            ErrorKind::External => "external",
            ErrorKind::Partial => "partial",
        }
    }
    pub(crate) fn all() -> [ErrorKind; 5] {
        [
            ErrorKind::Runtime,
            ErrorKind::Usage,
            ErrorKind::Input,
            ErrorKind::External,
            ErrorKind::Partial,
        ]
    }
}

/// Classify an `std::io::Error`: not-found / permission / invalid data are
/// input problems, everything else is runtime.
pub fn io_error_kind(e: &std::io::Error) -> ErrorKind {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound | K::PermissionDenied | K::InvalidData | K::InvalidInput | K::UnexpectedEof => {
            ErrorKind::Input
        }
        _ => ErrorKind::Runtime,
    }
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

static CONTRACT: OnceLock<&'static Contract> = OnceLock::new();
static JSON: AtomicBool = AtomicBool::new(false);

struct Failure {
    kind: ErrorKind,
    code: String,
    message: String,
}

struct State {
    command: String,
    data: Option<Value>,
    artifacts: Vec<(PathBuf, String)>,
    warnings: Vec<String>,
    failure: Option<Failure>,
}

static STATE: Mutex<State> = Mutex::new(State {
    command: String::new(),
    data: None,
    artifacts: Vec::new(),
    warnings: Vec::new(),
    failure: None,
});

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard)
}

/// True when `--json` was passed. Human-oriented output (progress, log echo)
/// must go to stderr or be suppressed while this is set.
pub fn json_mode() -> bool {
    JSON.load(Ordering::SeqCst)
}

/// Override json mode (tests, or tools that parse argv themselves).
pub fn set_json_mode(on: bool) {
    JSON.store(on, Ordering::SeqCst);
}

fn set_command(path: String) {
    with_state(|s| s.command = path);
}

/// Set the envelope's `data` payload. Last call wins.
pub fn set_data<T: Serialize>(data: T) {
    let v = serde_json::to_value(data).unwrap_or_else(|e| json!({ "serialize_error": e.to_string() }));
    with_state(|s| s.data = Some(v));
}

/// Record a file this run produced. `role` is a short tag such as
/// `"atlas-image"`, `"metadata"`, `"log"`. Size and sha256 are computed at
/// [`finish`] time; paths that no longer exist are dropped.
pub fn add_artifact(path: impl AsRef<Path>, role: &str) {
    let p = path.as_ref().to_path_buf();
    with_state(|s| {
        if !s.artifacts.iter().any(|(q, _)| *q == p) {
            s.artifacts.push((p, role.to_string()));
        }
    });
}

/// Attach a non-fatal warning to the envelope.
pub fn warn(msg: impl Into<String>) {
    with_state(|s| s.warnings.push(msg.into()));
}

/// Mark the run as failed while keeping `data` (e.g. verify found issues,
/// batch had failures). [`finish`] will exit with the kind's exit code.
pub fn set_failure(kind: ErrorKind, message: impl Into<String>) {
    set_failure_code(kind, kind.code(), message)
}

/// Like [`set_failure`] with a tool-specific `error.code`.
pub fn set_failure_code(kind: ErrorKind, code: &str, message: impl Into<String>) {
    let message = message.into();
    with_state(|s| {
        s.failure = Some(Failure {
            kind,
            code: code.to_string(),
            message,
        })
    });
}

// ---------------------------------------------------------------------------
// Finishing
// ---------------------------------------------------------------------------

/// End the process according to the contract.
///
/// * `Err(e)` → failure with `classify(&e)`; message is `e.to_string()`.
/// * `Ok(())` with a prior [`set_failure`] → that failure.
/// * otherwise success.
///
/// In json mode prints the envelope to stdout. In human mode prints the error
/// (if any) to stderr as `<tool>: error: <message>`.
pub fn finish<E: Display>(result: Result<(), E>, classify: impl FnOnce(&E) -> ErrorKind) -> ! {
    if let Err(e) = &result {
        let kind = classify(e);
        set_failure(kind, e.to_string());
    }
    exit_now()
}

/// [`finish`] for an `Ok` result.
pub fn finish_ok() -> ! {
    exit_now()
}

fn exit_now() -> ! {
    let (env, code) = build_envelope();
    if json_mode() {
        println!("{}", pretty(&env));
    } else if let Some(err) = env.get("error").filter(|e| !e.is_null()) {
        let tool = env["tool"].as_str().unwrap_or("tool");
        let msg = err["message"].as_str().unwrap_or("");
        // Partial failures already printed their per-item report.
        if code != ErrorKind::Partial.exit_code() || !msg.is_empty() {
            eprintln!("{}: error: {}", tool, msg);
        }
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

/// Build the envelope from the current state. Public for tests.
pub fn build_envelope() -> (Value, i32) {
    let (tool, version) = CONTRACT
        .get()
        .map(|c| (c.tool, c.version))
        .unwrap_or(("unknown", "0.0.0"));
    with_state(|s| {
        let artifacts: Vec<Value> = s
            .artifacts
            .iter()
            .filter_map(|(p, role)| artifact_json(p, role))
            .collect();
        let (ok, error, code) = match &s.failure {
            None => (true, Value::Null, 0),
            Some(f) => (
                false,
                json!({ "code": f.code, "kind": f.kind.code(), "message": f.message }),
                f.kind.exit_code(),
            ),
        };
        let mut env = json!({
            "contract": CONTRACT_VERSION,
            "ok": ok,
            "tool": tool,
            "version": version,
            "command": s.command,
            "data": s.data.clone().unwrap_or(Value::Null),
            "artifacts": artifacts,
            "error": error,
        });
        if !s.warnings.is_empty() {
            env["warnings"] = json!(s.warnings);
        }
        (env, code)
    })
}

fn artifact_json(path: &Path, role: &str) -> Option<Value> {
    use sha2::{Digest, Sha256};
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Some(json!({
        "path": abs.display().to_string(),
        "role": role,
        "bytes": meta.len(),
        "sha256": hex,
    }))
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

#[cfg(test)]
mod tests;
