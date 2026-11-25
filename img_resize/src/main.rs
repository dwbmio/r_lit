use std::fs;
use std::path::PathBuf;

use clap::Subcommand;
use img_resize::error::ReError;
use img_resize::optimize::{self, OptimizeOptions};
use img_resize::subcmd;
use rlit_cli::{runlog, CommandSpec, Contract, Effects, ErrorKind, Io};

/// Machine-call annotations for `__schema` (see `docs/cli-contract.md`).
static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[
        CommandSpec::new("r_resize", Effects::Pure)
            .io(&[
                ("path", Io::InputPath),
                ("resize_config", Io::InputFile),
                ("out_dir", Io::OutputDir),
            ])
            .note("Without --out-dir the input files are OVERWRITTEN in place; automated callers should always pass --out-dir. Exactly one mode: --max-pixel, --rw+--rh, --force-jpg, or --resize-config. Batch runs exit 5 (partial) when some files fail; per-file results are in data.results."),
        CommandSpec::new("optimize", Effects::Pure)
            .io(&[
                ("profile", Io::InputFile),
                ("path", Io::InputPath),
                ("out_dir", Io::OutputDir),
                ("report", Io::OutputFile),
            ])
            .note("Profile YAML follows `$engine_$channel.yml` (e.g. godot_wx-minip.yml, godot_qq-minip.yml, godot_dy-minip.yml, godot_zfb-minip.yml). Pass --out-dir (or set output in YAML) to avoid in-place overwrite; use --dry-run to audit effective pixel ratios and projected savings without writing files. Batch runs exit 5 (partial) when some files fail."),
        CommandSpec::new("profile list", Effects::ReadOnly)
            .note("Lists built-in `$engine_$channel` best-practice texture optimization presets."),
        CommandSpec::new("profile init", Effects::Pure)
            .io(&[("output", Io::OutputFile)])
            .note("Writes a commented `$engine_$channel.yml` starter profile to disk (defaults to `./<preset>.yml`)."),
    ],
};

#[derive(clap::Parser)]
#[command(
    name = "img_resize",
    version,
    about = "Resize, audit & optimize game textures (PNG/JPG/WebP) with channel profiles / 图片尺寸调整与渠道纹理优化工具",
    long_about = "跨平台图片缩放与游戏纹理体积优化工具（纯本地离线，无需网络依赖）。\n\n\
                  功能特性:\n  \
                  - 渠道配置优化 (`optimize $engine_$channel.yml`): Godot 微信/QQ/抖音/支付宝小游戏等最佳实践\n  \
                  - 有效像素分析: 假透明剥离、透明区 RGB 归零+边界扩色、低有效像素占比审计告警\n  \
                  - 质量无感压缩: 本地 imagequant (TinyPNG 同源) + PSNR 质量兜底 + oxipng + JPG/WebP 编码 + 体积不增保护\n  \
                  - 兼容原有 `r_resize` 单文件/批量缩放与 YAML 多尺寸导出\n\n\
                  示例 / Examples:\n  \
                  img_resize profile list\n  \
                  img_resize profile init godot_wx-minip\n  \
                  img_resize optimize godot_wx-minip.yml ./assets -o ./out --dry-run --json\n  \
                  img_resize optimize godot_dy-minip.yml ./assets -o ./out --report report.json --json\n  \
                  img_resize r_resize -m 800 image.jpg -o out/",
    after_help = "Machine-readable output (r_lit CLI contract v1):\n  \
                  --json: stdout is one envelope {ok, tool, version, command, data, artifacts, error}.\n  \
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
    /// Optimize textures using a `$engine_$channel.yml` profile / 按 `$engine_$channel.yml` 渠道配置优化纹理体积
    #[command(
        name = "optimize",
        long_about = "按 `$engine_$channel.yml` 渠道配置对单张图片或整个目录执行纹理体积优化与有效像素审计。\n\n\
                      支持引擎/渠道预设（自动按文件名 `$engine_$channel.yml` 或 YAML 内 engine/channel 继承默认值）:\n  \
                      - godot_wx-minip.yml  : Godot WASM 微信小游戏\n  \
                      - godot_qq-minip.yml  : Godot WASM QQ 小游戏\n  \
                      - godot_dy-minip.yml  : Godot WASM 抖音小游戏\n  \
                      - godot_zfb-minip.yml : Godot WASM 支付宝小游戏\n  \
                      - godot_mobile.yml    : Godot 原生移动端\n  \
                      - raw_png-jpg.yml     : 通用 PNG/JPG 渠道\n\n\
                      使用 `img_resize profile init godot_wx-minip` 生成带完整注释的模板配置。"
    )]
    Optimize {
        /// Path to `$engine_$channel.yml` profile config (e.g. godot_wx-minip.yml) /
        /// `$engine_$channel.yml` 渠道配置文件路径
        #[arg(value_name = "PROFILE_YML")]
        profile: PathBuf,

        /// Input image file or directory (overrides `input:` in YAML) /
        /// 输入图片或目录路径（可覆盖 YAML 中的 `input`）
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,

        /// Output directory (overrides `output:` in YAML; omit to overwrite in place) /
        /// 输出目录（覆盖 YAML 中的 `output`，不指定则原地覆盖）
        #[arg(short = 'o', long, value_name = "DIR")]
        out_dir: Option<PathBuf>,

        /// Analyze & simulate encoding without writing image files /
        /// 仅分析并模拟压缩，不写出图片文件
        #[arg(long)]
        dry_run: bool,

        /// Write detailed JSON audit & savings report to this path /
        /// 将详细审计与压缩报告写为 JSON 文件
        #[arg(long, value_name = "FILE")]
        report: Option<PathBuf>,

        /// Parallel worker thread count (defaults to CPU cores) /
        /// 并行处理线程数（默认按 CPU 核心数）
        #[arg(long, value_name = "N")]
        jobs: Option<usize>,
    },

    /// List or scaffold built-in `$engine_$channel.yml` profiles / 查看或生成内置渠道配置模板
    #[command(name = "profile")]
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },

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
                      注意: 此功能已禁用；请改用本地离线的 `img_resize optimize <$engine_$channel.yml>`"
    )]
    Tinyfy {
        /// 图片文件路径或目录路径（支持 PNG/JPG）
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// 执行最佳尺寸优化
        #[arg(short = 'd', long, help = "执行最佳尺寸优化")]
        do_size_perf: bool,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List built-in `$engine_$channel` best-practice presets / 列出内置渠道预设
    #[command(name = "list")]
    List,

    /// Generate a starter `$engine_$channel.yml` file / 生成带注释的 `$engine_$channel.yml` 模板
    #[command(name = "init")]
    Init {
        /// Preset name, e.g. godot_wx-minip, godot_qq-minip, godot_dy-minip, godot_zfb-minip /
        /// 预设名（如 godot_wx-minip）
        #[arg(value_name = "PRESET")]
        preset: String,

        /// Output YAML file path (defaults to `./<preset>.yml`) /
        /// 输出 YAML 文件路径（默认 `./<preset>.yml`）
        #[arg(short = 'o', long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Overwrite existing file if it already exists / 覆盖已存在文件
        #[arg(short = 'f', long)]
        force: bool,
    },
}

/// Sidecar `.log` path for a run.
fn log_target(command: &Commands) -> Option<PathBuf> {
    match command {
        Commands::RResize {
            out_dir: Some(dir), ..
        } => Some(dir.join("img_resize.log")),
        Commands::RResize { path, .. } => Some(if path.is_dir() {
            path.join("img_resize.log")
        } else {
            path.with_extension("log")
        }),
        Commands::Optimize {
            dry_run: true,
            report: Some(rep),
            ..
        } => Some(rep.with_extension("log")),
        Commands::Optimize { dry_run: true, .. } => None,
        Commands::Optimize {
            out_dir: Some(dir), ..
        } => Some(dir.join("img_resize.log")),
        Commands::Optimize {
            path: Some(p), ..
        } => Some(if p.is_dir() {
            p.join("img_resize.log")
        } else {
            p.with_extension("log")
        }),
        Commands::Optimize { profile, .. } => Some(profile.with_extension("log")),
        Commands::Profile { .. } | Commands::Tinyfy { .. } => None,
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn exec_profile_init(
    preset: &str,
    output: Option<PathBuf>,
    force: bool,
    json_output: bool,
) -> Result<(), ReError> {
    let Some(entry) = optimize::presets::find_preset_by_name(preset) else {
        let available: Vec<&str> = optimize::presets::PRESETS.iter().map(|p| p.id).collect();
        return Err(ReError::Input(format!(
            "unknown preset {:?}; available: {}",
            preset,
            available.join(", ")
        )));
    };

    let dest = output.unwrap_or_else(|| PathBuf::from(entry.filename));
    if dest.exists() && !force {
        return Err(ReError::Input(format!(
            "output file already exists: {} (pass --force to overwrite)",
            dest.display()
        )));
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let yaml = optimize::presets::render_yaml_template(entry);
    fs::write(&dest, yaml)?;
    rlit_cli::add_artifact(&dest, "profile");
    if !json_output {
        log::info!("wrote profile template: {}", dest.display());
    }
    rlit_cli::set_data(serde_json::json!({
        "preset": entry.id,
        "engine": entry.engine,
        "channel": entry.channel,
        "output": dest.display().to_string()
    }));
    Ok(())
}

#[tokio::main]
async fn main() {
    runlog::init();

    let cli: Cli = CONTRACT.parse();
    let log_path = log_target(&cli.command);

    let result: Result<(), ReError> = match cli.command {
        Commands::Optimize {
            profile,
            path,
            out_dir,
            dry_run,
            report,
            jobs,
        } => {
            let opts = OptimizeOptions {
                profile_path: profile,
                input_override: path,
                out_dir_override: out_dir,
                dry_run,
                report_path: report.clone(),
                jobs,
                json_output: cli.json,
            };
            match optimize::run_optimize(&opts) {
                Ok(rep) => {
                    if !rep.dry_run {
                        for r in &rep.results {
                            if matches!(r.status.as_str(), "optimized" | "unchanged") {
                                if let Some(out) = &r.output {
                                    rlit_cli::add_artifact(out, "image");
                                }
                            }
                        }
                    }
                    if let Some(report_file) = &report {
                        rlit_cli::add_artifact(report_file, "report");
                    }

                    if !cli.json {
                        log::info!(
                            "optimize done ({}_{}, dry_run={}): total={} optimized={} unchanged={} skipped={} failed={} | {} -> {} (saved {:.2}%)",
                            rep.profile.engine,
                            rep.profile.channel,
                            rep.dry_run,
                            rep.summary.total,
                            rep.summary.optimized,
                            rep.summary.unchanged,
                            rep.summary.skipped,
                            rep.summary.failed,
                            format_bytes(rep.summary.total_original_bytes),
                            format_bytes(rep.summary.total_output_bytes),
                            rep.summary.saved_pct
                        );
                        if !rep.audit_hotspots.is_empty() {
                            log::info!(
                                "audit warnings on {} texture(s) (showing top {}):",
                                rep.audit_hotspots.len(),
                                rep.audit_hotspots.len().min(5)
                            );
                            for h in rep.audit_hotspots.iter().take(5) {
                                log::warn!(
                                    "  {} ({}, eff={:.1}%): {}",
                                    h.file,
                                    format_bytes(h.new_bytes),
                                    h.effective_pixel_ratio.unwrap_or(1.0) * 100.0,
                                    h.warnings.join("; ")
                                );
                            }
                        }
                    }

                    if rep.summary.failed > 0 {
                        for r in rep.results.iter().filter(|r| r.status == "failed") {
                            log::warn!(
                                "failed: {} ({})",
                                r.file,
                                r.error.as_deref().unwrap_or("")
                            );
                        }
                        rlit_cli::set_failure(
                            ErrorKind::Partial,
                            format!("{} of {} file(s) failed", rep.summary.failed, rep.summary.total),
                        );
                    }

                    rlit_cli::set_data(rep);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Profile { action } => match action {
            ProfileAction::List => {
                let presets = optimize::presets::list_presets();
                if !cli.json {
                    for p in &presets {
                        println!(
                            "{:<18} ({:<22}) max={}px png={:?}({}-{}) jpg={} webp={:.0} — {}",
                            p.id,
                            p.filename,
                            p.max_dimension,
                            p.png_mode,
                            p.png_quality.0,
                            p.png_quality.1,
                            p.jpg_quality,
                            p.webp_quality,
                            p.summary
                        );
                    }
                }
                rlit_cli::set_data(serde_json::json!({
                    "presets": presets
                }));
                Ok(())
            }
            ProfileAction::Init {
                preset,
                output,
                force,
            } => exec_profile_init(&preset, output, force, cli.json),
        },
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
        Commands::Tinyfy { .. } => Err(ReError::Usage(
            "tinyfy is disabled (OpenSSL/musl conflict); use local `img_resize optimize <$engine_$channel.yml>`".into(),
        )),
    };

    // Flush the buffered run log next to the processed target (errors too).
    if let Some(path) = &log_path {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        runlog::flush(path, &runlog::standard_header());
        rlit_cli::add_artifact(path, "log");
    }

    rlit_cli::finish(result, error_kind);
}

/// Map errors onto the contract's exit-code classes.
fn error_kind(e: &ReError) -> ErrorKind {
    use ReError as E;
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
        assert!(
            problems.is_empty(),
            "contract problems:\n{}",
            problems.join("\n")
        );
    }
}
