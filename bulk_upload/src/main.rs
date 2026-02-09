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
    /// 从 JSON 文件解析 URL 列表，分批并发下载后上传到 S3
    Jp {
        /// JSON 文件路径，文件内容为 URL 字符串数组
        json_path: PathBuf,

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
        Commands::Jp {
            json_path,
            s3,
            prefix,
            concurrency,
        } => {
            subcmd::jp::exec(&json_path, &s3, &prefix, concurrency).await?;
        }
    }

    Ok(())
}
