mod error;
mod runlog;
mod subcmd;

use clap::Subcommand;
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io};
use std::path::PathBuf;

/// Machine-call annotations for `__schema` (see `docs/cli-contract.md`).
static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[CommandSpec::new("jq", Effects::External)
        .io(&[("s3", Io::InputFile)])
        .env(&["S3_BUCKET", "S3_ACCESS_KEY", "S3_SECRET_KEY", "S3_ENDPOINT", "S3_REGION"])
        .stdin("JSON document to scan for URLs, used when JSON_TEXT is omitted")
        .note("Downloads every http(s) URL found anywhere in the JSON and uploads it to s3://$S3_BUCKET/<prefix><file name>. Credentials come from --s3 <dotenv file> or, when --s3 is omitted, from the S3_* environment variables. Exit 5 = some URLs failed, 4 = all failed; per-URL results in data.files.")],
};

#[derive(clap::Parser)]
#[command(
    name = "bulk_upload",
    version,
    about = "Extract URLs from JSON, download them and upload to S3-compatible storage / 批量拉取文件并上传到 S3 对象存储的工具",
    long_about = "从 JSON 数据中提取 URL，批量下载文件并上传到 S3 兼容的对象存储。\n\
                  支持 MinIO、AWS S3、阿里云 OSS 等 S3 协议存储。\n\n\
                  示例:\n  \
                  cat data.json | bulk_upload jq -s ~/.s3config -p \"images/\" -c 20\n  \
                  bulk_upload jq '{\"urls\":[\"https://example.com/1.jpg\"]}' -s ~/.s3config\n  \
                  S3_BUCKET=b S3_ACCESS_KEY=.. S3_SECRET_KEY=.. S3_ENDPOINT=https://.. bulk_upload jq @data.json --json",
    after_help = "Machine-readable output (r_lit CLI contract v1):\n  \
                  --json: stdout is one envelope {ok, tool, version, command, data, artifacts, error};\n  \
                  data = {bucket, total_urls, total_success, total_failed, batches, files:[{source_url, s3_key, status, error}]}.\n  \
                  Exit codes: 0 ok, 1 runtime, 2 usage, 3 input (bad JSON / missing config), 4 external (all failed), 5 partial.\n  \
                  `bulk_upload __schema` prints a JSON Schema of every subcommand."
)]
struct Cli {
    /// Print one machine-readable JSON envelope to stdout / 启用 JSON 信封输出
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract http(s) URLs from JSON and upload them to S3 / 从 JSON 中提取 URL 并批量上传到 S3
    #[command(
        long_about = "从 JSON 数据中递归提取所有 HTTP/HTTPS URL，批量下载后上传到 S3。\n\n\
                      功能特性:\n  \
                      - 自动递归遍历 JSON 结构提取 URL\n  \
                      - URL 自动去重\n  \
                      - 批量并发下载和上传\n  \
                      - 支持从参数或 stdin 读取 JSON\n\n\
                      .s3 配置文件格式 (dotenv):\n  \
                      S3_BUCKET=my-bucket\n  \
                      S3_ACCESS_KEY=your-access-key\n  \
                      S3_SECRET_KEY=your-secret-key\n  \
                      S3_ENDPOINT=https://s3.example.com\n  \
                      S3_REGION=us-east-1"
    )]
    Jq {
        /// JSON text, or @FILE to read a JSON file. Omit (or pass `-`) to read stdin.
        /// JSON 文本内容，或 @文件路径；省略则从 stdin 读取（适合管道传入）
        #[arg(value_name = "JSON_TEXT")]
        json_text: Option<String>,

        /// .s3 配置文件的绝对路径（dotenv 格式）
        #[arg(
            short,
            long,
            value_name = "FILE",
            help = ".s3 dotenv config; omit to use S3_* env vars / .s3 配置文件路径",
            long_help = ".s3 dotenv config file. When omitted, the same keys are read from\n\
                         environment variables (S3_BUCKET, S3_ACCESS_KEY, ...).\n\
                         .s3 配置文件的绝对路径，dotenv 格式，必须包含:\n  \
                         S3_BUCKET - 存储桶名称\n  \
                         S3_ACCESS_KEY - 访问密钥\n  \
                         S3_SECRET_KEY - 密钥\n  \
                         S3_ENDPOINT - 端点 URL\n  \
                         S3_REGION - 区域（可选，默认 us-east-1）"
        )]
        s3: Option<PathBuf>,

        /// S3 上传目标前缀路径 (例如: assets/images/)
        #[arg(
            short,
            long,
            default_value = "",
            value_name = "PREFIX",
            help = "S3 object key prefix / S3 对象键前缀",
            long_help = "S3 上传目标前缀路径，例如: 'assets/images/'\n\
                         文件将上传为: s3://bucket/PREFIX/filename.jpg"
        )]
        prefix: String,

        /// 每批并发下载/上传数量（1-100）
        #[arg(
            short,
            long,
            default_value_t = 10,
            value_name = "N",
            value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100),
            help = "Files per concurrent batch (1-100) / 并发数量",
            long_help = "每批并发下载/上传的文件数量，用于控制并发度。\n\
                         建议值: 10-50，过高可能导致网络拥塞或被限流"
        )]
        concurrency: usize,
    },
}

#[tokio::main]
async fn main() {
    runlog::init();

    let cli: Cli = CONTRACT.parse();

    // No single output file (uploads land in S3), so the run log is dropped
    // next to the working directory as `bulk_upload.log` for after-the-fact
    // review of what was downloaded/uploaded.
    let log_path = std::env::current_dir()
        .unwrap_or_default()
        .join("bulk_upload.log");

    let result = run(cli).await;

    runlog::flush(&log_path, &runlog::standard_header());
    rlit_cli::add_artifact(&log_path, "log");

    rlit_cli::finish(result, error_kind);
}

/// Map errors onto the contract's exit-code classes.
fn error_kind(e: &error::AppError) -> ErrorKind {
    use error::AppError as E;
    match e {
        E::IoError(io) => rlit_cli::io_error_kind(io),
        E::JsonError(_) | E::S3ConfigError(_) => ErrorKind::Input,
        E::ReqwestError(_) | E::S3PutError(_) | E::DownloadFailed(..) => ErrorKind::External,
        E::CustomError(_) => ErrorKind::Usage,
    }
}

async fn run(cli: Cli) -> Result<(), error::AppError> {
    match cli.command {
        Commands::Jq {
            json_text,
            s3,
            prefix,
            concurrency,
        } => {
            let text = match json_text {
                Some(t) if t.starts_with('@') => std::fs::read_to_string(&t[1..]).map_err(|e| {
                    error::AppError::S3ConfigError(format!("cannot read JSON file {}: {}", &t[1..], e))
                })?,
                Some(t) if t != "-" => t,
                _ => {
                    use std::io::{IsTerminal, Read};
                    if std::io::stdin().is_terminal() {
                        return Err(error::AppError::CustomError(
                            "no JSON_TEXT given and stdin is a terminal; pass JSON, @FILE, or pipe it in".into(),
                        ));
                    }
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf).map_err(|e| {
                        error::AppError::CustomError(format!("从 stdin 读取 JSON 文本失败: {}", e))
                    })?;
                    buf
                }
            };
            subcmd::jq::exec(&text, s3.as_deref(), &prefix, concurrency, cli.json).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_contract_is_complete() {
        let problems = rlit_cli::check_contract(&CONTRACT, &Cli::command());
        assert!(problems.is_empty(), "contract problems:\n{}", problems.join("\n"));
    }
}
