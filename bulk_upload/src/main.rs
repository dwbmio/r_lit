mod error;
mod subcmd;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bulk_upload",
    version,
    about = "批量拉取文件并上传到 S3 对象存储的工具"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 接收 JSON 文本（直接参数或 stdin），提取所有 URL 并分批并发下载后上传到 S3
    Jq {
        /// JSON 文本内容。若省略则从 stdin 读取（适合管道传入）
        json_text: Option<String>,

        /// .s3 配置文件的绝对路径（dotenv 格式，包含 S3_BUCKET/ACCESS_KEY/SECRET_KEY/ENDPOINT/REGION）
        #[arg(short, long)]
        s3: PathBuf,

        /// S3 上传目标前缀路径 (例如: assets/images/)
        #[arg(short, long, default_value = "")]
        prefix: String,

        /// 每批并发下载/上传数量
        #[arg(short, long, default_value_t = 10)]
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
            subcmd::jq::exec(&text, &s3, &prefix, concurrency).await?;
        }
    }

    Ok(())
}
