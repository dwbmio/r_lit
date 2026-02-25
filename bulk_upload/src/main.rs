mod error;
mod subcmd;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bulk_upload",
    version,
    about = "批量拉取文件并上传到 S3 对象存储的工具",
    long_about = "从 JSON 数据中提取 URL，批量下载文件并上传到 S3 兼容的对象存储。\n\
                  支持 MinIO、AWS S3、阿里云 OSS 等 S3 协议存储。\n\n\
                  示例:\n  \
                  cat data.json | bulk_upload jq -s ~/.s3config -p \"images/\" -c 20\n  \
                  bulk_upload jq '{\"urls\":[\"https://example.com/1.jpg\"]}' -s ~/.s3config"
)]
struct Cli {
    /// 启用 JSON 格式输出（便于程序解析）
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 从 JSON 中提取 URL 并批量上传到 S3
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
        /// JSON 文本内容。若省略则从 stdin 读取（适合管道传入）
        #[arg(value_name = "JSON_TEXT")]
        json_text: Option<String>,

        /// .s3 配置文件的绝对路径（dotenv 格式）
        #[arg(
            short,
            long,
            value_name = "FILE",
            help = ".s3 配置文件路径",
            long_help = ".s3 配置文件的绝对路径，dotenv 格式，必须包含:\n  \
                         S3_BUCKET - 存储桶名称\n  \
                         S3_ACCESS_KEY - 访问密钥\n  \
                         S3_SECRET_KEY - 密钥\n  \
                         S3_ENDPOINT - 端点 URL\n  \
                         S3_REGION - 区域（可选，默认 us-east-1）"
        )]
        s3: PathBuf,

        /// S3 上传目标前缀路径 (例如: assets/images/)
        #[arg(
            short,
            long,
            default_value = "",
            value_name = "PREFIX",
            help = "S3 对象键前缀",
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
            help = "并发数量",
            long_help = "每批并发下载/上传的文件数量，用于控制并发度。\n\
                         建议值: 10-50，过高可能导致网络拥塞或被限流"
        )]
        concurrency: usize,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // init logger
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                message
            ))
        })
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .chain(std::io::stdout())
        .apply()?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Jq {
            json_text,
            s3,
            prefix,
            concurrency,
        } => {
            let text = match json_text {
                Some(t) => t,
                None => {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .expect("从 stdin 读取 JSON 文本失败");
                    buf
                }
            };
            subcmd::jq::exec(&text, &s3, &prefix, concurrency, cli.json).await?;
        }
    }

    Ok(())
}
