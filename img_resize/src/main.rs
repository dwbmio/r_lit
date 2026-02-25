use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod error;
mod subcmd;

#[derive(Parser)]
#[command(
    name = "img_resize",
    version,
    about = "图片尺寸调整和压缩工具",
    long_about = "跨平台图片处理工具，支持批量调整尺寸和压缩。\n\n\
                  功能特性:\n  \
                  - 纯 Rust 实现，无需网络依赖\n  \
                  - 支持 PNG 和 JPG 格式\n  \
                  - 批量处理目录\n  \
                  - TinyPNG API 集成\n\n\
                  示例:\n  \
                  img_resize r_resize -m 800 image.jpg\n  \
                  img_resize r_resize --rw 1920 --rh 1080 image.jpg\n  \
                  img_resize tinyfy images/"
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
    /// 使用纯 Rust 调整图片尺寸
    #[command(
        name = "r_resize",
        long_about = "使用纯 Rust 库调整图片尺寸，无需网络依赖。\n\n\
                      支持三种调整模式:\n  \
                      1. 配置文件模式: 使用 YAML 配置批量生成多个尺寸\n  \
                      2. 等比缩放模式: 指定最大像素值，保持宽高比\n  \
                      3. 精确调整模式: 指定目标宽度和高度\n\n\
                      YAML 配置文件格式:\n  \
                      vec_size:\n    \
                      - [1920, 1080]\n    \
                      - [800, 600]\n  \
                      vec_f:\n    \
                      - \"output/large.png\"\n    \
                      - \"output/small.png\"\n  \
                      base_f: \"/output/base/path\""
    )]
    RResize {
        /// 图片文件路径或目录路径（支持 PNG/JPG）
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// YAML 配置文件路径（与其他尺寸参数互斥）
        #[arg(
            short = 'c',
            long,
            value_name = "FILE",
            conflicts_with_all = ["max_pixel", "resize_width", "resize_height"],
            help = "YAML 配置文件路径",
            long_help = "YAML 格式的调整配置文件路径。\n\
                         使用此参数时，将忽略其他尺寸参数。\n\
                         配置文件可以定义多个输出尺寸和路径"
        )]
        resize_config: Option<PathBuf>,

        /// 最大像素值（等比缩放，与 --rw/--rh 互斥）
        #[arg(
            short = 'm',
            long,
            value_name = "SIZE",
            conflicts_with_all = ["resize_width", "resize_height"],
            help = "最大像素值（等比缩放）",
            long_help = "设置最大像素值进行等比缩放。\n\
                         图片的宽和高都不会超过此值，保持原始宽高比。\n\
                         如果图片已经小于此值，则跳过处理"
        )]
        max_pixel: Option<u32>,

        /// 目标宽度（必须与 --rh 一起使用）
        #[arg(
            long,
            value_name = "WIDTH",
            requires = "resize_height",
            conflicts_with = "max_pixel",
            help = "目标宽度（像素）",
            long_help = "设置目标宽度（像素）。\n\
                         必须与 --rh 一起使用，进行精确尺寸调整。\n\
                         不保持原始宽高比"
        )]
        rw: Option<u32>,

        /// 目标高度（必须与 --rw 一起使用）
        #[arg(
            long,
            value_name = "HEIGHT",
            requires = "resize_width",
            conflicts_with = "max_pixel",
            help = "目标高度（像素）",
            long_help = "设置目标高度（像素）。\n\
                         必须与 --rw 一起使用，进行精确尺寸调整。\n\
                         不保持原始宽高比"
        )]
        rh: Option<u32>,

        /// 强制转换为 JPG 格式
        #[arg(
            short = 'j',
            long,
            help = "强制转换为 JPG 格式",
            long_help = "将图片强制转换为 JPG 格式。\n\
                         PNG 图片将被转换为 JPG，可能丢失透明度信息"
        )]
        force_jpg: bool,
    },

    /// 使用 TinyPNG API 压缩图片
    #[command(
        name = "tinyfy",
        long_about = "使用 TinyPNG API 压缩图片。\n\n\
                      在保持视觉质量的同时显著减小文件大小。\n\
                      支持单文件或目录批量处理。\n\n\
                      注意: 此功能暂时禁用，等待 rustls 支持"
    )]
    Tinyfy {
        /// 图片文件路径或目录路径（支持 PNG/JPG）
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// 执行最佳尺寸优化
        #[arg(
            short = 'd',
            long,
            help = "执行最佳尺寸优化",
            long_help = "启用 TinyPNG 的最佳尺寸优化功能。\n\
                         可能会进一步减小文件大小"
        )]
        do_size_perf: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let c = fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                message
            ))
        })
        // Add blanket level filter -
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .chain(std::io::stdout())
        .apply()?;

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::RResize {
            path,
            resize_config,
            max_pixel,
            rw,
            rh,
            force_jpg,
        } => {
            subcmd::r_tp::exec(
                &path,
                resize_config.as_ref(),
                max_pixel,
                rw,
                rh,
                force_jpg,
                cli.json,
            )
            .await
        }
        Commands::Tinyfy { path, do_size_perf } => {
            eprintln!("Error: tinyfy command is temporarily disabled");
            eprintln!("Reason: OpenSSL dependency conflicts with musl static builds");
            eprintln!("Use r_resize command instead for image processing");
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        if cli.json {
            eprintln!(
                "{}",
                serde_json::json!({
                    "status": "error",
                    "error": format!("{:?}", e)
                })
            );
        } else {
            log::error!("{:?}", e);
        }
        std::process::exit(1);
    }

    Ok(())
}
