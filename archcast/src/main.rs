use archcast::backend::{BackendChoice, resolve_backend};
use archcast::error::{ArchcastError, Result};
use archcast::raster::{GifMode, GifOptions};
use archcast::schema::starter_template;
use archcast::{
    RenderFormats, extract_inputs, render_inputs, spec_to_yaml, validate_file, write_atomic,
};
use clap::{Parser, Subcommand, ValueEnum};
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io, runlog};
use std::path::PathBuf;

static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[
        CommandSpec::new("render", Effects::Pure)
            .io(&[("input", Io::InputPath), ("output_dir", Io::OutputDir)])
            .note("Compiles YAML/JSON specs into standalone HTML (with WebGPU/Canvas2D motion), standalone root-level SVG files, canonical Markdown, and optional PNG/animated GIF exports."),
        CommandSpec::new("extract", Effects::Pure)
            .io(&[("input", Io::InputPath), ("output_dir", Io::OutputDir)])
            .note("Reverse-parses existing HTML+SVG overview pages into canonical YAML SSOT and Markdown documentation."),
        CommandSpec::new("validate", Effects::ReadOnly)
            .io(&[("input", Io::InputFile)])
            .note("Validates YAML/JSON schema, card IDs, edge topology, and flow table integrity. Exits 3 on validation failure."),
        CommandSpec::new("init", Effects::Pure)
            .io(&[("output", Io::OutputFile)])
            .note("Generates a starter 4-column architecture overview YAML spec template."),
        CommandSpec::new("info", Effects::ReadOnly)
            .note("Reports active backend, SIMD ISA profile, Rayon worker count, and optional WebGPU adapter status."),
    ],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Emit HTML + standalone SVG(s) + Markdown (default) / 输出 HTML + 独立 SVG + Markdown（默认）
    All,
    /// Alias for HTML + standalone SVG(s) + Markdown / 同 all
    Both,
    /// Emit HTML + SVG + Markdown + raster PNG + animated GIF / 输出全套含 PNG 与动态 GIF
    Media,
    /// Emit standalone HTML only / 仅输出 HTML
    Html,
    /// Emit standalone root-level SVG file(s) only / 仅输出独立 SVG 文件
    Svg,
    /// Emit canonical Markdown + Mermaid only / 仅输出 Markdown + Mermaid
    Md,
    /// Emit raster PNG file(s) only / 仅输出光栅 PNG
    Png,
    /// Emit animated flow-pulse GIF file(s) only / 仅输出流动动效 GIF
    Gif,
}

#[derive(Parser)]
#[command(
    name = "archcast",
    version,
    about = "AI-first architecture & flow report compiler (SIMD + Rayon + optional WebGPU) / 面向 AI 的确定性架构与流程汇报编译器",
    long_about = "Compile concise YAML/JSON architecture specs into deterministic 1:1 standalone HTML reports (with WebGPU/Canvas2D motion), standalone root-level SVG diagrams, optional raster PNG & animated GIF exports, and canonical Markdown + Mermaid documentation.\n\n将精简的 YAML/JSON 架构规范确定性编译为 1:1 固定样式 HTML 汇报页（内置 WebGPU/Canvas2D 流动动效）、根目录独立 SVG、可选 PNG 光栅图与动态 GIF 动图，以及标准 Markdown + Mermaid 文档。",
    after_help = "Examples:\n  archcast render overview.yaml -o out/ --json\n  archcast render overview.yaml -o out/ --png --gif --json\n  archcast render overview.yaml -o out/ --format media --backend gpu --json\n  archcast extract legacy.html -o extracted/ --json\n  archcast validate overview.yaml --json\n\nMachine contract: --json emits one rlit_cli envelope; __schema exports CLI JSON Schema.\nExit codes: 0 ok, 1 runtime, 2 usage, 3 input, 5 partial."
)]
struct Cli {
    /// Print one machine-readable JSON envelope / 输出机器调用 JSON 信封
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile YAML/JSON spec(s) to HTML, standalone SVG(s), Markdown, PNG, and GIF / 编译 YAML/JSON 规范为多格式产物
    Render {
        /// Spec file (.yaml/.yml/.json) or directory for Rayon batch compilation / 规范文件或批量目录
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Output directory (standalone SVGs, HTML, MD, PNG, GIF are written to its root) / 输出根目录
        #[arg(short, long, value_name = "DIR")]
        output_dir: PathBuf,

        /// Output preset (all, both, media, html, svg, md, png, gif) / 输出格式组合
        #[arg(short, long, value_enum, default_value_t = OutputFormat::All)]
        format: OutputFormat,

        /// Also emit raster PNG for each diagram into output_dir root / 额外导出各图光栅 PNG
        #[arg(long)]
        png: bool,

        /// Also emit animated flow-pulse GIF for each diagram into output_dir root / 额外导出各图流动动效 GIF
        #[arg(long)]
        gif: bool,

        /// GIF layering mode: `overlay` (default, transparent minimal dynamic box) or `delta` (static base + minimal dirty-rect deltas) / GIF 分层模式：overlay（默认透明最小动态区）或 delta（静态底图+最小脏区增量帧）
        #[arg(long, value_enum, default_value_t = GifMode::Overlay)]
        gif_mode: GifMode,

        /// PNG rasterization scale factor (0.2..3.0) / PNG 光栅化缩放倍率
        #[arg(long, default_value_t = 1.0)]
        scale: f32,

        /// Animated GIF scale factor (0.2..2.0) / GIF 动图缩放倍率
        #[arg(long, default_value_t = 0.65)]
        gif_scale: f32,

        /// Number of animation frames per GIF loop (4..60) / GIF 循环帧数
        #[arg(long, default_value_t = 20)]
        frames: usize,

        /// Animated GIF playback frame rate (2..30) / GIF 播放帧率
        #[arg(long, default_value_t = 12)]
        fps: u16,

        /// Disable browser WebGPU/Canvas2D interactive motion JS inside HTML / 关闭 HTML 内置 WebGPU/Canvas2D 动效，仅输出纯静态 HTML
        #[arg(long)]
        static_html: bool,

        /// Render backend selection (auto, cpu, gpu) / 渲染后端选择
        #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
        backend: BackendChoice,

        /// Reject specs with validation errors before rendering / 渲染前执行严格校验
        #[arg(long)]
        strict: bool,
    },

    /// Reverse-extract HTML+SVG overview(s) into canonical YAML + Markdown / 将既有 HTML+SVG 逆向提取为 YAML 与 Markdown
    Extract {
        /// HTML file or directory containing HTML+SVG overviews / HTML 文件或批量目录
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Output directory for extracted YAML, Markdown, and log / 输出目录
        #[arg(short, long, value_name = "DIR")]
        output_dir: PathBuf,

        /// Skip Markdown generation and write YAML only / 仅输出 YAML，不生成 Markdown
        #[arg(long)]
        yaml_only: bool,
    },

    /// Validate a YAML/JSON spec without writing outputs / 校验 YAML/JSON 规范合法性
    Validate {
        /// Spec file (.yaml/.yml/.json) to validate / 待校验规范文件
        #[arg(value_name = "INPUT")]
        input: PathBuf,
    },

    /// Write a starter 4-column architecture YAML template / 生成标准四栏架构 YAML 模板
    Init {
        /// Destination YAML file path / 目标 YAML 文件路径
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,

        /// Document title for the starter template / 模板标题
        #[arg(short, long, default_value = "系统链路全景与改造落点")]
        title: String,
    },

    /// Show runtime SIMD, Rayon, and optional WebGPU backend capabilities / 查看运行期后端与 SIMD/Rayon/WebGPU 能力
    Info {
        /// Backend preference to probe (auto, cpu, gpu) / 探测指定后端
        #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
        backend: BackendChoice,
    },
}

fn main() {
    runlog::init();
    let cli: Cli = CONTRACT.parse();
    let log_path = sidecar_log_path(&cli.command);

    let result = run(&cli);
    if let Err(err) = &result {
        log::error!("{err}");
    }

    if let Some(lp) = &log_path
        && lp.parent().is_some_and(|p| p.is_dir())
    {
        runlog::flush(lp, &runlog::standard_header());
        rlit_cli::add_artifact(lp, "log");
    }

    rlit_cli::finish(result, |err| err.kind());
}

fn sidecar_log_path(cmd: &Commands) -> Option<PathBuf> {
    match cmd {
        Commands::Render { output_dir, .. } | Commands::Extract { output_dir, .. } => {
            Some(output_dir.join("archcast.log"))
        }
        _ => None,
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Commands::Render {
            input,
            output_dir,
            format,
            png,
            gif,
            gif_mode,
            scale,
            gif_scale,
            frames,
            fps,
            static_html,
            backend,
            strict,
        } => {
            let resolved = resolve_backend(*backend)?;
            let binfo = resolved.info();
            log::info!(
                "render start: input={} output_dir={} backend={} simd={} threads={}",
                input.display(),
                output_dir.display(),
                binfo.active_backend,
                binfo.simd_profile,
                binfo.rayon_threads
            );

            let formats = RenderFormats {
                html: matches!(
                    format,
                    OutputFormat::All | OutputFormat::Both | OutputFormat::Media | OutputFormat::Html
                ),
                svg: matches!(
                    format,
                    OutputFormat::All | OutputFormat::Both | OutputFormat::Media | OutputFormat::Svg
                ),
                markdown: matches!(
                    format,
                    OutputFormat::All | OutputFormat::Both | OutputFormat::Media | OutputFormat::Md
                ),
                png: *png || matches!(format, OutputFormat::Media | OutputFormat::Png),
                gif: *gif || matches!(format, OutputFormat::Media | OutputFormat::Gif),
                motion: !*static_html,
                png_scale: *scale,
                gif_opts: GifOptions {
                    mode: *gif_mode,
                    scale: *gif_scale,
                    frames: *frames,
                    fps: *fps,
                },
            };

            let items = render_inputs(input, output_dir, formats, *strict, resolved.as_ref())?;

            let mut ok_count = 0usize;
            let mut fail_count = 0usize;
            for item in &items {
                if item.ok {
                    ok_count += 1;
                    if let Some(hp) = &item.html_output {
                        rlit_cli::add_artifact(hp, "report-html");
                    }
                    for sp in &item.svg_outputs {
                        rlit_cli::add_artifact(sp, "diagram-svg");
                    }
                    if let Some(mp) = &item.markdown_output {
                        rlit_cli::add_artifact(mp, "doc-markdown");
                    }
                    for pp in &item.png_outputs {
                        rlit_cli::add_artifact(pp, "diagram-png");
                    }
                    for gp in &item.gif_outputs {
                        rlit_cli::add_artifact(&gp.path, "diagram-gif");
                    }
                } else {
                    fail_count += 1;
                }
            }

            rlit_cli::set_data(serde_json::json!({
                "backend": binfo,
                "total": items.len(),
                "succeeded": ok_count,
                "failed": fail_count,
                "items": items,
            }));

            if fail_count > 0 && ok_count > 0 {
                rlit_cli::set_failure(
                    ErrorKind::Partial,
                    format!("{fail_count} of {} spec(s) failed to render", items.len()),
                );
            } else if fail_count > 0 {
                let first_err = items
                    .iter()
                    .find_map(|i| i.error.clone())
                    .unwrap_or_else(|| "render failed".to_string());
                return Err(ArchcastError::Usage(first_err));
            }

            if !rlit_cli::json_mode() {
                println!(
                    "archcast render: {ok_count}/{} succeeded (backend: {}, threads: {})",
                    items.len(),
                    binfo.active_backend,
                    binfo.rayon_threads
                );
                for item in &items {
                    if let Some(hp) = &item.html_output {
                        println!("  HTML: {}", hp.display());
                    }
                    for sp in &item.svg_outputs {
                        println!("  SVG:  {}", sp.display());
                    }
                    if let Some(mp) = &item.markdown_output {
                        println!("  MD:   {}", mp.display());
                    }
                    for pp in &item.png_outputs {
                        println!("  PNG:  {}", pp.display());
                    }
                    for gp in &item.gif_outputs {
                        println!(
                            "  GIF:  {} ({:?}, {}x{} @ +{},{} in {}x{})",
                            gp.path.display(),
                            gp.layer.mode,
                            gp.layer.width,
                            gp.layer.height,
                            gp.layer.offset_x,
                            gp.layer.offset_y,
                            gp.layer.canvas_width,
                            gp.layer.canvas_height
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Extract {
            input,
            output_dir,
            yaml_only,
        } => {
            log::info!(
                "extract start: input={} output_dir={}",
                input.display(),
                output_dir.display()
            );
            let items = extract_inputs(input, output_dir, !*yaml_only)?;
            let mut ok_count = 0usize;
            let mut fail_count = 0usize;
            for item in &items {
                if item.ok {
                    ok_count += 1;
                    if let Some(yp) = &item.yaml_output {
                        rlit_cli::add_artifact(yp, "spec-yaml");
                    }
                    if let Some(mp) = &item.markdown_output {
                        rlit_cli::add_artifact(mp, "doc-markdown");
                    }
                } else {
                    fail_count += 1;
                }
            }

            rlit_cli::set_data(serde_json::json!({
                "total": items.len(),
                "succeeded": ok_count,
                "failed": fail_count,
                "items": items,
            }));

            if fail_count > 0 && ok_count > 0 {
                rlit_cli::set_failure(
                    ErrorKind::Partial,
                    format!("{fail_count} of {} HTML file(s) failed to extract", items.len()),
                );
            } else if fail_count > 0 {
                let first_err = items
                    .iter()
                    .find_map(|i| i.error.clone())
                    .unwrap_or_else(|| "extract failed".to_string());
                return Err(ArchcastError::Usage(first_err));
            }

            if !rlit_cli::json_mode() {
                println!("archcast extract: {ok_count}/{} succeeded", items.len());
                for item in &items {
                    if let Some(yp) = &item.yaml_output {
                        println!("  YAML: {}", yp.display());
                    }
                    if let Some(mp) = &item.markdown_output {
                        println!("  MD:   {}", mp.display());
                    }
                }
            }
            Ok(())
        }
        Commands::Validate { input } => {
            let report = validate_file(input)?;
            rlit_cli::set_data(&report);
            if !report.valid {
                rlit_cli::set_failure_code(
                    ErrorKind::Input,
                    "validation_failed",
                    format!("spec validation failed with {} error(s)", report.error_count),
                );
            }
            if !rlit_cli::json_mode() {
                if report.valid {
                    println!(
                        "archcast validate: OK ({} warning(s))",
                        report.warning_count
                    );
                } else {
                    eprintln!(
                        "archcast validate: FAILED ({} error(s), {} warning(s))",
                        report.error_count, report.warning_count
                    );
                }
                for issue in &report.issues {
                    eprintln!("  [{:?}] {}: {}", issue.severity, issue.path, issue.message);
                }
            }
            Ok(())
        }
        Commands::Init { output, title } => {
            let spec = starter_template(title);
            let yaml = spec_to_yaml(&spec)?;
            write_atomic(output, yaml.as_bytes())?;
            rlit_cli::add_artifact(output, "spec-yaml");
            rlit_cli::set_data(serde_json::json!({
                "output": output,
                "title": title,
            }));
            if !rlit_cli::json_mode() {
                println!("archcast init: wrote starter spec to {}", output.display());
            }
            Ok(())
        }
        Commands::Info { backend } => {
            let resolved = resolve_backend(*backend)?;
            let info = resolved.info();
            rlit_cli::set_data(&info);
            if !rlit_cli::json_mode() {
                println!("backend:   {}", info.active_backend);
                println!("simd:      {}", info.simd_profile);
                println!("threads:   {}", info.rayon_threads);
                println!("gpu_built: {}", info.gpu_feature_compiled);
                if let Some(adapter) = &info.gpu_adapter {
                    println!("gpu:       {adapter}");
                }
                println!("notes:     {}", info.notes);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
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
