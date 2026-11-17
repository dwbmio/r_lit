mod config;
mod db;
mod error;
mod ingest;
mod meta;
mod output;
mod protocol;
mod server;
mod source;
mod storage;
mod tail;
mod view;

use clap::{Parser, Subcommand};
use db::{default_db_path, parse_since, QueryFilter, SqliteStorage, DEFAULT_KEEP_HOURS};
use error::{Error, Result};
use ingest::IngestOptions;
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io};
use std::path::PathBuf;

static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[
        CommandSpec::new("serve", Effects::External)
            .note("Long-running loopback HTTP intake server; blocks until interrupted"),
        CommandSpec::new("run", Effects::LocalWrite)
            .note("Executes child command after `--`, records stdout/stderr, exits with child status"),
        CommandSpec::new("push", Effects::LocalWrite)
            .stdin("Log lines read from stdin and stored as one run"),
        CommandSpec::new("list", Effects::ReadOnly),
        CommandSpec::new("show", Effects::ReadOnly),
        CommandSpec::new("grep", Effects::ReadOnly),
        CommandSpec::new("clean", Effects::LocalWrite),
        CommandSpec::new("tail", Effects::External)
            .io(&[("path", Io::InputFile)])
            .note("Long-running stream follower for cmd/journald/nginx/file/logcat"),
        CommandSpec::new("config", Effects::LocalWrite),
        CommandSpec::new("view", Effects::LocalWrite)
            .note("Long-running local multi-column web viewer; blocks until interrupted"),
    ],
};

#[derive(Parser)]
#[command(
    name = "looplog",
    version,
    author = "looplog contributors",
    about = "Local-first log intake and query tool for AI-assisted debugging",
    long_about = "looplog captures short-lived local debugging logs into SQLite and exposes \
        a loopback-only HTTP intake protocol for cross-language tools.\n\n\
        The MVP is tuned for WeChat Mini Program debugging: appid, page, base library, \
        device, session, and trace metadata can be recorded and queried from the CLI.",
    after_help = "Examples:\n  \
        looplog serve --addr 127.0.0.1:3768\n  \
        looplog run --tag miniprogram-build --meta appid=wx123 -- npm run build\n  \
        looplog list --kind wechat_miniprogram --appid wx123 --json\n  \
        looplog grep TypeError --appid wx123 --page pages/index/index --since 2h --json"
)]
struct Cli {
    /// Output machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,

    /// SQLite database path. Defaults to the platform state/data directory.
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Configuration file. Defaults to ~/.config/looplog/config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP intake server (loopback by default, Loki push compatible).
    Serve {
        /// Address to bind. Non-loopback requires --allow-lan.
        #[arg(long, default_value = "127.0.0.1:3768")]
        addr: String,
        /// Retention window in hours. Values above 24 are capped to 24.
        #[arg(long, default_value_t = DEFAULT_KEEP_HOURS)]
        keep_hours: u64,
        /// Accept non-loopback peers (real device on the same LAN).
        #[arg(long)]
        allow_lan: bool,
        /// Shared secret required from non-loopback peers. Min 16 chars.
        #[arg(long)]
        token: Option<String>,
        /// CIDR allowlist for non-loopback peers. Repeatable or comma separated.
        #[arg(long = "allow-cidr", value_name = "CIDR")]
        allow_cidrs: Vec<String>,
    },
    /// Run a command, capture stdout/stderr, and return the child exit code.
    Run {
        /// Tag assigned to the recorded run.
        #[arg(long)]
        tag: Option<String>,
        /// Free-form source label.
        #[arg(long)]
        source: Option<String>,
        /// Log kind (e.g. wechat_miniprogram).
        #[arg(long)]
        kind: Option<String>,
        /// Metadata as key=value. Useful keys: appid, project_path, page, session, trace_id.
        #[arg(long = "meta", value_name = "KEY=VALUE")]
        meta: Vec<String>,
        /// Command and arguments to execute.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Read stdin and store it as one run.
    Push {
        /// Tag assigned to the recorded run.
        #[arg(long)]
        tag: Option<String>,
        /// Free-form source label.
        #[arg(long)]
        source: Option<String>,
        /// Log kind (e.g. wechat_miniprogram).
        #[arg(long)]
        kind: Option<String>,
        /// Default log level for ingested lines.
        #[arg(long)]
        level: Option<String>,
        /// Metadata as key=value.
        #[arg(long = "meta", value_name = "KEY=VALUE")]
        meta: Vec<String>,
    },
    /// List recent runs.
    List {
        #[command(flatten)]
        filter: FilterArgs,
        /// Maximum number of runs to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show lines from a run.
    Show {
        /// Run ID (or unique prefix) to inspect.
        run_id: String,
        /// Show only the last N lines.
        #[arg(long)]
        tail: Option<usize>,
        /// Filter by stream (stdout/stderr).
        #[arg(long)]
        stream: Option<String>,
    },
    /// Search log lines.
    Grep {
        /// Substring pattern to search for.
        pattern: String,
        #[command(flatten)]
        filter: FilterArgs,
        /// Maximum number of hits to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Delete expired records. Defaults to 24 hours retention.
    Clean {
        /// Retention window in hours before records expire.
        #[arg(long, default_value_t = DEFAULT_KEEP_HOURS)]
        keep_hours: u64,
        /// Run SQLite VACUUM after deleting rows.
        #[arg(long)]
        vacuum: bool,
    },
    /// Follow a log source into the database (cmd / journald / nginx / file / logcat).
    Tail {
        /// Tag that groups this stream in `looplog view`.
        #[arg(long)]
        tag: String,
        /// Free-form source label stored on the run.
        #[arg(long)]
        source: Option<String>,
        /// Log kind label stored on the run.
        #[arg(long)]
        kind: Option<String>,
        /// Structured source: cmd (default), journald, nginx, file, logcat.
        #[arg(long = "from", default_value = "cmd", value_name = "KIND")]
        from: String,
        /// Run the source command on this ssh host.
        #[arg(long)]
        remote: Option<String>,
        /// systemd unit for --from journald.
        #[arg(long)]
        unit: Option<String>,
        /// Log file path for --from nginx / file.
        #[arg(long)]
        path: Option<String>,
        /// Android device/emulator serial for --from logcat (adb -s).
        #[arg(long)]
        serial: Option<String>,
        /// Journald backfill window (e.g. 15m / 2h) instead of --lines.
        #[arg(long)]
        since: Option<String>,
        /// Prefix the remote command with `sudo -n` (journald / root-owned logs).
        #[arg(long)]
        sudo: bool,
        /// Number of existing lines to read before following.
        #[arg(long)]
        lines: Option<usize>,
        /// Metadata as key=value.
        #[arg(long = "meta", value_name = "KEY=VALUE")]
        meta: Vec<String>,
        /// Restart the source when it exits, with exponential backoff.
        #[arg(long)]
        reconnect: bool,
        /// Maximum reconnect backoff in seconds.
        #[arg(long = "reconnect-max", default_value_t = 30)]
        reconnect_max: u64,
        /// Command and arguments when --from cmd.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Show or initialise the configuration file.
    Config {
        /// Write a starter config (refuses to overwrite unless --force).
        #[arg(long)]
        init: bool,
        /// Overwrite existing config file when used with --init.
        #[arg(long)]
        force: bool,
    },
    /// Multi-column live view of tagged streams in a local window.
    View {
        /// Loopback address for the viewer.
        #[arg(long, default_value = "127.0.0.1:3769")]
        addr: String,
        /// Time window override, e.g. 5m / 15m / 1h (default: from config).
        #[arg(long)]
        since: Option<String>,
        /// Column as label:tag. Repeatable; overrides config when given.
        #[arg(long = "column", value_name = "LABEL:TAG")]
        columns: Vec<String>,
        /// Open a browser window on start.
        #[arg(long)]
        open: bool,
    },
}

#[derive(clap::Args, Debug, Clone, Default)]
struct FilterArgs {
    /// Filter by run ID (or prefix).
    #[arg(long = "run")]
    run_id: Option<String>,
    /// Filter by tag.
    #[arg(long)]
    tag: Option<String>,
    /// Filter by kind.
    #[arg(long)]
    kind: Option<String>,
    /// Filter by mini-program appid.
    #[arg(long)]
    appid: Option<String>,
    /// Filter by project path.
    #[arg(long = "project")]
    project: Option<String>,
    /// Filter by page path.
    #[arg(long)]
    page: Option<String>,
    /// Filter by session id.
    #[arg(long)]
    session: Option<String>,
    /// Filter by trace id.
    #[arg(long = "trace")]
    trace: Option<String>,
    /// Relative duration like 2h/30m or RFC3339 timestamp.
    #[arg(long)]
    since: Option<String>,
}

fn main() {
    let cli: Cli = CONTRACT.parse();
    let result = run(cli);
    rlit_cli::finish(result, classify_error);
}

fn classify_error(err: &Error) -> ErrorKind {
    match err {
        Error::Io(io) => rlit_cli::io_error_kind(io),
        Error::Json(_) | Error::TimeParse(_) | Error::InvalidMeta(_) => ErrorKind::Input,
        Error::MissingCommand => ErrorKind::Usage,
        Error::Http(_) => ErrorKind::External,
        Error::Db(_) | Error::Message(_) => ErrorKind::Runtime,
    }
}

fn run(cli: Cli) -> Result<()> {
    let db_path = cli.db.unwrap_or_else(default_db_path);

    match cli.command {
        Commands::Serve {
            addr,
            keep_hours,
            allow_lan,
            token,
            allow_cidrs,
        } => {
            ensure_bind_allowed(&addr, allow_lan)?;
            server::serve(
                db_path,
                server::ServeOptions {
                    addr,
                    keep_hours: cap_keep_hours(keep_hours),
                    allow_lan,
                    token,
                    allow_cidrs,
                },
            )
        }
        Commands::Run {
            tag,
            source,
            kind,
            meta,
            command,
        } => {
            let exit_code = ingest::run_command(
                &db_path,
                IngestOptions {
                    tag,
                    source,
                    kind: normalize_kind(kind),
                    meta: meta::parse_cli_meta(&meta)?,
                },
                &command,
            )?;
            std::process::exit(exit_code);
        }
        Commands::Push {
            tag,
            source,
            kind,
            level,
            meta,
        } => {
            let run_id = ingest::push_stdin(
                &db_path,
                IngestOptions {
                    tag,
                    source,
                    kind: normalize_kind(kind),
                    meta: meta::parse_cli_meta(&meta)?,
                },
                level,
            )?;
            rlit_cli::set_data(serde_json::json!({"status": "ok", "run_id": run_id}));
            if !rlit_cli::json_mode() {
                println!("{}", run_id);
            }
            Ok(())
        }
        Commands::List { filter, limit } => {
            let mut storage = SqliteStorage::open(&db_path)?;
            let runs = storage.list_runs(&filter.try_into()?, limit)?;
            rlit_cli::set_data(serde_json::json!({"status": "ok", "runs": runs}));
            if !rlit_cli::json_mode() {
                output::print_runs(&runs);
            }
            Ok(())
        }
        Commands::Show {
            run_id,
            tail,
            stream,
        } => {
            let mut storage = SqliteStorage::open(&db_path)?;
            let lines = storage.show_lines(&run_id, tail, stream.as_deref())?;
            rlit_cli::set_data(serde_json::json!({
                "status": "ok",
                "run_id": run_id,
                "lines": lines
            }));
            if !rlit_cli::json_mode() {
                output::print_lines(&lines);
            }
            Ok(())
        }
        Commands::Grep {
            pattern,
            filter,
            limit,
        } => {
            let mut storage = SqliteStorage::open(&db_path)?;
            let hits = storage.search(&pattern, &filter.try_into()?, limit)?;
            rlit_cli::set_data(serde_json::json!({
                "status": "ok",
                "search_backend": "like",
                "hits": hits
            }));
            if !rlit_cli::json_mode() {
                output::print_hits(&hits);
            }
            Ok(())
        }
        Commands::Clean { keep_hours, vacuum } => {
            let mut storage = SqliteStorage::open(&db_path)?;
            let stats = storage.clean(cap_keep_hours(keep_hours), vacuum)?;
            rlit_cli::set_data(serde_json::json!({"status": "ok", "clean": stats}));
            if !rlit_cli::json_mode() {
                output::print_clean(&stats);
            }
            Ok(())
        }
        Commands::Tail {
            tag,
            kind,
            source,
            from,
            remote,
            unit,
            path,
            serial,
            since,
            sudo,
            lines,
            meta,
            reconnect,
            reconnect_max,
            command,
        } => {
            let spec = parse_source_spec(&from, remote, unit, path, serial, since, sudo, lines)?;
            tail::tail_command(
                &db_path,
                tail::TailOptions {
                    tag,
                    kind: normalize_kind(kind),
                    source,
                    meta: meta::parse_cli_meta(&meta)?,
                    reconnect,
                    reconnect_max_secs: reconnect_max,
                    spec,
                },
                &command,
            )
        }
        Commands::Config { init, force } => {
            let path = cli.config.unwrap_or_else(config::default_config_path);
            if init {
                if path.exists() && !force {
                    return Err(Error::Message(format!(
                        "{} already exists (use --force to overwrite)",
                        path.display()
                    )));
                }
                let cfg = config::Config::default();
                config::save(&path, &cfg)?;
                rlit_cli::add_artifact(&path, "config");
                rlit_cli::set_data(serde_json::json!({ "path": path, "action": "init" }));
                if !rlit_cli::json_mode() {
                    println!("wrote {}", path.display());
                }
                return Ok(());
            }
            let cfg = config::load(&path)?;
            rlit_cli::set_data(&cfg);
            if !rlit_cli::json_mode() {
                println!("# {}", path.display());
                print!(
                    "{}",
                    toml::to_string_pretty(&cfg)
                        .map_err(|e| Error::Message(format!("could not render config: {e}")))?
                );
            }
            Ok(())
        }
        Commands::View {
            addr,
            since,
            columns,
            open,
        } => {
            ensure_loopback(&addr)?;
            let config_path = cli.config.unwrap_or_else(config::default_config_path);
            let loaded = match config::load(&config_path) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("[looplog] {e}; continuing with defaults");
                    config::Config::default()
                }
            };
            let columns = if columns.is_empty() {
                None
            } else {
                Some(
                    columns
                        .iter()
                        .map(|c| parse_column(c))
                        .collect::<Result<Vec<config::Column>>>()?,
                )
            };
            let cfg = loaded.with_overrides(since, columns);
            view::serve(
                db_path,
                view::ViewOptions {
                    addr,
                    config: cfg,
                    config_path,
                    open,
                },
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_source_spec(
    from: &str,
    remote: Option<String>,
    unit: Option<String>,
    path: Option<String>,
    serial: Option<String>,
    since: Option<String>,
    sudo: bool,
    lines: Option<usize>,
) -> Result<source::SourceSpec> {
    let need = |value: Option<String>, what: &str| -> Result<String> {
        value.ok_or_else(|| Error::Message(format!("--from {from} requires {what}")))
    };
    let lines = lines.unwrap_or(500);
    Ok(match from {
        "cmd" => source::SourceSpec::Cmd {
            command: Vec::new(),
        },
        "logcat" => source::SourceSpec::Logcat { serial, lines },
        "journald" => source::SourceSpec::Journald {
            unit: need(unit, "--unit <unit>")?,
            remote,
            since,
            lines,
            sudo,
        },
        "nginx" => source::SourceSpec::Nginx {
            path: need(path, "--path <access.log>")?,
            remote,
            lines,
            sudo,
            follow: true,
        },
        "file" => source::SourceSpec::File {
            path: need(path, "--path <file>")?,
            lines,
            follow: true,
        },
        other => {
            return Err(Error::Message(format!(
                "unknown --from `{other}`, expected cmd|journald|nginx|file|logcat"
            )))
        }
    })
}

fn parse_column(spec: &str) -> Result<config::Column> {
    match spec.split_once(':') {
        Some((label, tag)) if !tag.trim().is_empty() => Ok(config::Column {
            label: label.trim().to_string(),
            tag: tag.trim().to_string(),
        }),
        Some(_) => Err(Error::Message(format!(
            "invalid column `{spec}`, expected label:tag"
        ))),
        None if !spec.trim().is_empty() => Ok(config::Column {
            label: spec.trim().to_string(),
            tag: spec.trim().to_string(),
        }),
        None => Err(Error::Message("empty column spec".to_string())),
    }
}

impl TryFrom<FilterArgs> for QueryFilter {
    type Error = Error;

    fn try_from(value: FilterArgs) -> Result<Self> {
        Ok(QueryFilter {
            run_id: value.run_id,
            tag: value.tag,
            kind: normalize_kind(value.kind),
            appid: value.appid,
            project: value.project,
            page: value.page,
            session: value.session,
            trace: value.trace,
            since_ms: value.since.as_deref().map(parse_since).transpose()?,
        })
    }
}

fn normalize_kind(kind: Option<String>) -> Option<String> {
    kind.map(|k| {
        if k == "wechat" || k == "wx" {
            meta::DEFAULT_KIND.to_string()
        } else {
            k
        }
    })
}

fn cap_keep_hours(keep_hours: u64) -> u64 {
    keep_hours.min(DEFAULT_KEEP_HOURS)
}

/// Non-loopback binds are opt-in; the token requirement is enforced again by
/// the server so a bad `--addr` cannot silently expose writes.
fn ensure_bind_allowed(addr: &str, allow_lan: bool) -> Result<()> {
    if allow_lan || is_loopback(addr) {
        return Ok(());
    }
    Err(Error::Message(format!(
        "refusing to bind non-loopback address `{addr}` without --allow-lan"
    )))
}

fn is_loopback(addr: &str) -> bool {
    addr.starts_with("127.") || addr.starts_with("localhost:") || addr.starts_with("[::1]:")
}

fn ensure_loopback(addr: &str) -> Result<()> {
    if is_loopback(addr) {
        Ok(())
    } else {
        Err(Error::Message(format!(
            "refusing to bind non-loopback address `{addr}`"
        )))
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_matches_rlit_contract() {
        let issues = rlit_cli::check_contract(&CONTRACT, &Cli::command());
        assert!(issues.is_empty(), "contract issues: {issues:#?}");
    }
}
