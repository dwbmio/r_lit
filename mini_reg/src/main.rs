use clap::{Parser, Subcommand};
use mini_reg::{
    error::{invalid, Result},
    model::Platform,
    runlog,
};
use std::path::PathBuf;

mod subcmd;

#[derive(Parser, Debug)]
#[command(
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
    Init { output: PathBuf },
    /// Import a batch atomically; repeated input is a no-op / 整批导入并排重
    Import { manifest: PathBuf },
    /// List durable tasks / 查看持久任务
    List {
        #[arg(long)]
        batch: Option<String>,
    },
    /// Show a task and its official portal / 查看任务与官方入口
    Show { id: String },
    /// Open the official portal in the bound enterprise Chrome / 打开企业浏览器中的官方后台
    Open { id: String },
    /// Preview fields; --apply fills visible text only / 预览填表，明确传 --apply 才写入
    Fill {
        id: String,
        /// Explicit portal tab; otherwise use the task's saved tab / 指定页面编号
        #[arg(long)]
        tab: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Record your confirmation after checking the platform / 人工核实后登记应用编号
    Confirm {
        id: String,
        #[arg(long)]
        app_id: String,
    },
    /// Export metadata; never login data / 导出进度与应用编号
    Export {
        output: PathBuf,
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
        #[arg(long, value_enum)]
        platform: Platform,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        summary: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BrowserCommand {
    /// Verify corporate profile and process fingerprint / 核实企业身份与进程指纹
    Bind {
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

fn main() {
    let cli = Cli::parse();
    runlog::init();
    let path = match db_path(cli.db.clone()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let command = match &cli.command {
        Commands::Init { .. } => "init",
        Commands::Import { .. } => "import",
        Commands::List { .. } => "list",
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
    let (value, code) = match result {
        Ok(value) => (serde_json::json!({"ok": true, "result": value}), 0),
        Err(error) => {
            log::error!("{error}");
            (
                serde_json::json!({"ok": false, "error": error.to_string()}),
                1,
            )
        }
    };
    let log_path = match &cli.command {
        Commands::Init { output } | Commands::Export { output, .. } => output.with_extension("log"),
        _ => path.with_extension("log"),
    };
    if let Err(error) = runlog::flush(&log_path, command) {
        eprintln!("Could not save run log / 运行日志写入失败: {error}");
    }
    let serialized = if cli.json {
        serde_json::to_string(&value)
    } else {
        serde_json::to_string_pretty(&value)
    };
    match serialized {
        Ok(text) => println!("{text}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    std::process::exit(code);
}
