use clap::{Parser, Subcommand};
use mini_reg::{
    error::{invalid, RegError, Result},
    model::Platform,
};
use rlit_cli::{runlog, CommandSpec, Contract, Effects, ErrorKind, Io};
use std::path::PathBuf;

mod subcmd;

static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[
        CommandSpec::new("init", Effects::Pure).io(&[("output", Io::OutputFile)]),
        CommandSpec::new("import", Effects::LocalWrite).io(&[("manifest", Io::InputFile)]),
        CommandSpec::new("list", Effects::ReadOnly),
        CommandSpec::new("next", Effects::LocalWrite),
        CommandSpec::new("show", Effects::ReadOnly),
        CommandSpec::new("open", Effects::External),
        CommandSpec::new("fill", Effects::External),
        CommandSpec::new("confirm", Effects::LocalWrite),
        CommandSpec::new("export", Effects::LocalWrite).io(&[("output", Io::OutputFile)]),
        CommandSpec::new("browser bind", Effects::External)
            .io(&[("user_data_dir", Io::InputDir)]),
        CommandSpec::new("browser tabs", Effects::External),
        CommandSpec::new("selectors", Effects::LocalWrite),
    ],
};

#[derive(Parser, Debug)]
#[command(
    name = "mini_reg",
    version,
    about = "Persistent mini-app registration helper / 自家小程序半自动注册辅助"
)]
pub struct Cli {
    /// State database; defaults to the OS app-data directory / 持久状态库
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// One JSON envelope on stdout / 输出单个 JSON 结果
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Write a batch template without overwriting / 创建批次模板
    Init {
        /// Output batch TOML template path / 输出模板路径
        output: PathBuf,
    },
    /// Import a batch atomically; repeated input is a no-op / 整批导入并排重
    Import {
        /// Batch TOML manifest path / 批次清单路径
        manifest: PathBuf,
    },
    /// List durable tasks / 查看持久任务
    List {
        /// Filter by batch id / 按批次过滤
        #[arg(long)]
        batch: Option<String>,
    },
    /// Resume the next unconfirmed task / 继续下一个未确认任务
    Next {
        /// Filter by batch id / 按批次过滤
        #[arg(long)]
        batch: Option<String>,
        /// Open a portal only when this task has no saved tab / 没有页面记录时打开官方后台
        #[arg(long)]
        open: bool,
    },
    /// Show a task and its official portal / 查看任务与官方入口
    Show {
        /// Application task id / 任务 ID
        id: String,
    },
    /// Open the official portal in the bound enterprise Chrome / 打开企业浏览器中的官方后台
    Open {
        /// Application task id / 任务 ID
        id: String,
    },
    /// Preview fields; --apply fills visible text only / 预览填表，明确传 --apply 才写入
    Fill {
        /// Application task id / 任务 ID
        id: String,
        /// Explicit portal tab; otherwise use the task's saved tab / 指定页面编号
        #[arg(long)]
        tab: Option<String>,
        /// Actually write visible form fields / 确认写入表单字段
        #[arg(long)]
        apply: bool,
    },
    /// Record your confirmation after checking the platform / 人工核实后登记应用编号
    Confirm {
        /// Application task id / 任务 ID
        id: String,
        /// Platform-issued app id / 平台应用编号
        #[arg(long)]
        app_id: String,
    },
    /// Export metadata; never login data / 导出进度与应用编号
    Export {
        /// Output JSON file path / 导出 JSON 路径
        output: PathBuf,
        /// Filter by batch id / 按批次过滤
        #[arg(long)]
        batch: Option<String>,
    },
    /// Bind or inspect the enterprise Chrome / 企业浏览器连接
    Browser {
        #[command(subcommand)]
        command: BrowserCommand,
    },
    /// Persist exact selectors when a portal changes / 保存明确的表单选择器
    Selectors {
        /// Target mini-app platform / 目标平台
        #[arg(long, value_enum)]
        platform: Platform,
        /// CSS selector for app name field / 名称输入框选择器
        #[arg(long)]
        name: Option<String>,
        /// CSS selector for summary field / 简介输入框选择器
        #[arg(long)]
        summary: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BrowserCommand {
    /// Verify corporate profile and process fingerprint / 核实企业身份与进程指纹
    Bind {
        /// Enterprise Chrome user-data directory / 企业浏览器配置目录
        #[arg(long)]
        user_data_dir: PathBuf,
        /// Required; never choose a default personal profile / 必填，不自动选择个人配置
        #[arg(long)]
        profile_directory: String,
    },
    /// List only official portal tabs, without URL tokens / 仅列出官方后台页面
    Tabs,
}

fn db_path(path: Option<PathBuf>) -> Result<PathBuf> {
    path.map(Ok).unwrap_or_else(|| {
        directories::ProjectDirs::from("com", "gamesci-lite", "mini_reg")
            .map(|p| p.data_local_dir().join("state.sqlite3"))
            .ok_or_else(|| invalid("No app-data directory; pass --db / 请用 --db 指定状态库"))
    })
}

fn classify_error(err: &RegError) -> ErrorKind {
    match err {
        RegError::Io(io) => rlit_cli::io_error_kind(io),
        RegError::Json(_) | RegError::Toml(_) => ErrorKind::Input,
        RegError::Http(_) | RegError::WebSocket(_) => ErrorKind::External,
        RegError::Invalid(_) => ErrorKind::Usage,
        RegError::Database(_) => ErrorKind::Runtime,
    }
}

fn run(cli: &Cli) -> Result<()> {
    let path = db_path(cli.db.clone())?;
    let command = match &cli.command {
        Commands::Init { .. } => "init",
        Commands::Import { .. } => "import",
        Commands::List { .. } => "list",
        Commands::Next { .. } => "next",
        Commands::Show { .. } => "show",
        Commands::Open { .. } => "open",
        Commands::Fill { .. } => "fill",
        Commands::Confirm { .. } => "confirm",
        Commands::Export { .. } => "export",
        Commands::Browser { .. } => "browser",
        Commands::Selectors { .. } => "selectors",
    };
    log::info!("command={command}");
    let result = subcmd::execute(&cli.command, &path);

    let log_path = match &cli.command {
        Commands::Init { output } | Commands::Export { output, .. } => output.with_extension("log"),
        _ => path.with_extension("log"),
    };
    let mut header = runlog::standard_header();
    header.push(format!("command: {command}"));

    match &result {
        Ok(value) => {
            rlit_cli::set_data(value);
            match &cli.command {
                Commands::Init { output } | Commands::Export { output, .. } => {
                    rlit_cli::add_artifact(output, "output");
                }
                _ => {}
            }
            if !rlit_cli::json_mode() {
                if let Ok(text) = serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "result": value,
                })) {
                    println!("{text}");
                }
            }
        }
        Err(error) => {
            log::error!("{error}");
        }
    }

    runlog::flush(&log_path, &header);
    rlit_cli::add_artifact(&log_path, "log");
    result.map(|_| ())
}

fn main() {
    runlog::init_stderr();
    let cli: Cli = CONTRACT.parse();
    let result = run(&cli);
    rlit_cli::finish(result, classify_error);
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
