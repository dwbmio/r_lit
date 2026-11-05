use clap::Subcommand;
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io};
use std::path::PathBuf;

mod error;
mod runlog;
mod subcmd;

/// Machine-call annotations for `__schema` (see `docs/cli-contract.md`).
static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[CommandSpec::new("r_resize", Effects::Pure)
        .io(&[("path", Io::InputPath), ("resize_config", Io::InputFile), ("out_dir", Io::OutputDir)])
        .note("Without --out-dir the input files are OVERWRITTEN in place; automated callers should always pass --out-dir. Exactly one mode: --max-pixel, --rw+--rh, --force-jpg, or --resize-config. Batch runs exit 5 (partial) when some files fail; per-file results are in data.results.")],
};

#[derive(clap::Parser)]
#[command(
    name = "img_resize",
    version,
    about = "Resize / convert PNG & JPG images, single file or whole directory / 图片尺寸调整和压缩工具",
    long_about = "跨平台图片处理工具，支持批量调整尺寸和压缩。\n\n\
                  功能特性:\n  \
                  - 纯 Rust 实现，无需网络依赖\n  \
                  - 支持 PNG 和 JPG 格式\n  \
                  - 批量处理目录\n  \
                  - TinyPNG API 集成\n\n\
                  示例 / Examples:\n  \
                  img_resize r_resize -m 800 image.jpg -o out/\n  \
                  img_resize r_resize --rw 1920 --rh 1080 image.jpg -o out/\n  \
                  img_resize r_resize -m 512 sprites/ -o thumbs/ --json\n  \
                  img_resize r_resize -c sizes.yaml icon.png",
    after_help = "Machine-readable output (r_lit CLI contract v1):\n  \
                  --json: stdout is one envelope {ok, tool, version, command, data, artifacts, error};\n  \
                  data = {total, results:[{file, output, status, original_size, new_size, error}]}.\n  \
                  Exit codes: 0 ok, 1 runtime, 2 usage, 3 input (path missing / bad config), 5 partial.\n  \
                  `img_resize __schema` prints a JSON Schema of every subcommand."
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
    /// Resize or convert images with pure Rust (no network) / 使用纯 Rust 调整图片尺寸
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
        /// Image file or directory (PNG/JPG, walked recursively, dot-files skipped) /
        /// 图片文件路径或目录路径（支持 PNG/JPG）
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Write results into this directory instead of overwriting the inputs
        /// in place (directory inputs are flattened by file name). Ignored with
        /// --resize-config. / 输出目录；不指定则原地覆盖输入文件
        #[arg(short = 'o', long, value_name = "DIR", conflicts_with = "resize_config")]
        out_dir: Option<PathBuf>,

        /// YAML 配置文件路径（与其他尺寸参数互斥）
        #[arg(
            short = 'c',
            long,
            value_name = "FILE",
            conflicts_with_all = ["max_pixel", "rw", "rh", "force_jpg"],
            help = "YAML config generating several sizes from one image / YAML 配置文件路径",
            long_help = "YAML config generating several sizes from one image (PNG output).\n\
                         YAML 格式的调整配置文件路径。\n\
                         使用此参数时，将忽略其他尺寸参数。\n\
                         配置文件可以定义多个输出尺寸和路径"
        )]
        resize_config: Option<PathBuf>,

        /// 最大像素值（等比缩放，与 --rw/--rh 互斥）
        #[arg(
            short = 'm',
            long,
            value_name = "SIZE",
            conflicts_with_all = ["rw", "rh"],
            help = "Max width/height in px, keeps aspect ratio / 最大像素值（等比缩放）",
            long_help = "Longest side limit in pixels; aspect ratio is kept, smaller images are left as-is.\n\
                         设置最大像素值进行等比缩放。\n\
                         图片的宽和高都不会超过此值，保持原始宽高比。\n\
                         如果图片已经小于此值，则跳过处理"
        )]
        max_pixel: Option<u32>,

        /// 目标宽度（必须与 --rh 一起使用）
        #[arg(
            long,
            value_name = "WIDTH",
            requires = "rh",
            conflicts_with = "max_pixel",
            help = "Exact target width in px, requires --rh / 目标宽度（像素）",
            long_help = "设置目标宽度（像素）。\n\
                         必须与 --rh 一起使用，进行精确尺寸调整。\n\
                         不保持原始宽高比"
        )]
        rw: Option<u32>,

        /// 目标高度（必须与 --rw 一起使用）
        #[arg(
            long,
            value_name = "HEIGHT",
            requires = "rw",
            conflicts_with = "max_pixel",
            help = "Exact target height in px, requires --rw / 目标高度（像素）",
            long_help = "设置目标高度（像素）。\n\
                         必须与 --rw 一起使用，进行精确尺寸调整。\n\
                         不保持原始宽高比"
        )]
        rh: Option<u32>,

        /// 强制转换为 JPG 格式
        #[arg(
            short = 'j',
            long,
            help = "Convert to JPG (quality 100) instead of resizing / 强制转换为 JPG 格式",
            long_help = "将图片强制转换为 JPG 格式。\n\
                         PNG 图片将被转换为 JPG，可能丢失透明度信息"
        )]
        force_jpg: bool,
    },

    /// TinyPNG compression — DISABLED in current builds / 使用 TinyPNG API 压缩图片（暂不可用）
    #[command(
        name = "tinyfy",
        hide = true,
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

/// Sidecar `.log` path for a run. The log is attached to the input target:
/// `<file>.log` for a single image, or `img_resize.log` inside a directory.
fn log_target(command: &Commands) -> Option<PathBuf> {
    match command {
        Commands::RResize { out_dir: Some(dir), .. } => Some(dir.join("img_resize.log")),
        Commands::RResize { path, .. } => Some(if path.is_dir() {
            path.join("img_resize.log")
        } else {
            path.with_extension("log")
        }),
        Commands::Tinyfy { .. } => None,
    }
}

#[tokio::main]
async fn main() {
    runlog::init();

    let cli: Cli = CONTRACT.parse();
    let log_path = log_target(&cli.command);

    let result = match cli.command {
        Commands::RResize {
            path,
            out_dir,
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
                out_dir.as_ref(),
                cli.json,
            )
            .await
        }
        Commands::Tinyfy { .. } => Err(error::ReError::Usage(
            "tinyfy is temporarily disabled (OpenSSL conflicts with musl static builds); use r_resize".into(),
        )),
    };

    // Flush the buffered run log next to the processed target (errors too).
    if let Some(path) = &log_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        runlog::flush(path, &runlog::standard_header());
        rlit_cli::add_artifact(path, "log");
    }

    rlit_cli::finish(result, error_kind);
}

/// Map errors onto the contract's exit-code classes.
fn error_kind(e: &error::ReError) -> ErrorKind {
    use error::ReError as E;
    match e {
        E::IOError(io) => rlit_cli::io_error_kind(io),
        E::ImageHandleError(_) | E::WalkDirError(_) | E::Input(_) => ErrorKind::Input,
        E::Usage(_) => ErrorKind::Usage,
        E::ParseError(_) | E::JsonError(_) | E::CustomError(_) => ErrorKind::Runtime,
    }
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
