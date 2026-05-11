mod db;
mod error;
mod ingest;
mod meta;
mod output;
mod protocol;
mod server;
mod storage;

use clap::{Parser, Subcommand};
use db::{default_db_path, parse_since, QueryFilter, SqliteStorage, DEFAULT_KEEP_HOURS};
use error::{Error, Result};
use ingest::IngestOptions;
use std::path::PathBuf;

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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the loopback-only HTTP intake server.
    Serve {
        /// Address to bind. Must be loopback for the MVP.
        #[arg(long, default_value = "127.0.0.1:3768")]
        addr: String,
        /// Retention window in hours. Values above 24 are capped to 24.
        #[arg(long, default_value_t = DEFAULT_KEEP_HOURS)]
        keep_hours: u64,
    },
    /// Run a command, capture stdout/stderr, and return the child exit code.
    Run {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        /// Metadata as key=value. Useful keys: appid, project_path, page, session, trace_id.
        #[arg(long = "meta", value_name = "KEY=VALUE")]
        meta: Vec<String>,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Read stdin and store it as one run.
    Push {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        level: Option<String>,
        #[arg(long = "meta", value_name = "KEY=VALUE")]
        meta: Vec<String>,
    },
    /// List recent runs.
    List {
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show lines from a run.
    Show {
        run_id: String,
        #[arg(long)]
        tail: Option<usize>,
        #[arg(long)]
        stream: Option<String>,
    },
    /// Search log lines.
    Grep {
        pattern: String,
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Delete expired records. Defaults to 24 hours retention.
    Clean {
        #[arg(long, default_value_t = DEFAULT_KEEP_HOURS)]
        keep_hours: u64,
        #[arg(long)]
        vacuum: bool,
    },
}

#[derive(clap::Args, Debug, Clone, Default)]
struct FilterArgs {
    #[arg(long = "run")]
    run_id: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    appid: Option<String>,
    #[arg(long = "project")]
    project: Option<String>,
    #[arg(long)]
    page: Option<String>,
    #[arg(long)]
    session: Option<String>,
    #[arg(long = "trace")]
    trace: Option<String>,
    /// Relative duration like 2h/30m or RFC3339 timestamp.
    #[arg(long)]
    since: Option<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("looplog: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(default_db_path);

    match cli.command {
        Commands::Serve { addr, keep_hours } => {
            ensure_loopback(&addr)?;
            server::serve(&addr, db_path, cap_keep_hours(keep_hours))
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
            if cli.json {
                output::print_json(&serde_json::json!({"status": "ok", "run_id": run_id}))?;
            } else {
                println!("{}", run_id);
            }
            Ok(())
        }
        Commands::List { filter, limit } => {
            let mut storage = SqliteStorage::open(&db_path)?;
            let runs = storage.list_runs(&filter.try_into()?, limit)?;
            if cli.json {
                output::print_json(&serde_json::json!({"status": "ok", "runs": runs}))?;
            } else {
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
            if cli.json {
                output::print_json(&serde_json::json!({
                    "status": "ok",
                    "run_id": run_id,
                    "lines": lines
                }))?;
            } else {
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
            if cli.json {
                output::print_json(&serde_json::json!({
                    "status": "ok",
                    "search_backend": "like",
                    "hits": hits
                }))?;
            } else {
                output::print_hits(&hits);
            }
            Ok(())
        }
        Commands::Clean { keep_hours, vacuum } => {
            let mut storage = SqliteStorage::open(&db_path)?;
            let stats = storage.clean(cap_keep_hours(keep_hours), vacuum)?;
            if cli.json {
                output::print_json(&serde_json::json!({"status": "ok", "clean": stats}))?;
            } else {
                output::print_clean(&stats);
            }
            Ok(())
        }
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

fn ensure_loopback(addr: &str) -> Result<()> {
    if addr.starts_with("127.") || addr.starts_with("localhost:") || addr.starts_with("[::1]:") {
        Ok(())
    } else {
        Err(Error::Message(format!(
            "refusing to bind non-loopback address `{}` in the MVP",
            addr
        )))
    }
}
