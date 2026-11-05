mod bmfont;
mod color;
mod effect;
mod error;
mod font;
mod render;
mod runlog;

use clap::Subcommand;
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io};
use std::path::PathBuf;

/// Machine-call annotations for `__schema` (see `docs/cli-contract.md`).
static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[
        CommandSpec::new("render", Effects::LocalWrite)
            .io(&[("output", Io::OutputFile)])
            .note("Writes one PNG (+ <output>.log). Output is reproducible when --font is a .ttf/.otf path; a family name resolves against fonts installed on the machine. List effect specs with `textexture list-effects --json`."),
        CommandSpec::new("bmfont", Effects::LocalWrite)
            .io(&[("output", Io::OutputStem)])
            .note("Writes <output>.fnt + <output>.png (BMFont text format, page 0). Same font-resolution caveat as render."),
        CommandSpec::new("list-effects", Effects::ReadOnly),
        CommandSpec::new("list-fonts", Effects::ReadOnly).note("Lists fonts installed on this machine; results differ per host."),
    ],
};

/// Generate stylized text images with visual effects
/// 生成带视觉效果的艺术字图片
#[derive(clap::Parser)]
#[command(
    name = "textexture",
    version,
    about,
    long_about = "Generate stylized art-text PNGs and BMFont bitmap fonts \
        (shadow / stroke / gradient / glow / neon).\n\
        生成带视觉效果的艺术字图片与 BMFont 位图字体。\n\n\
        Examples:\n  \
        textexture render \"Level Up\" -o title.png -s 96 -e outline:color=#000000,width=4 -e glow:color=#ffcc00,radius=12\n  \
        textexture render \"胜利\" -f ./fonts/zcool.ttf --transparent -o win.png --json\n  \
        textexture bmfont \"0123456789\" -o digits -e gradient:start=#ffffff,end=#ffcc00,angle=90\n  \
        textexture list-effects --json",
    after_help = "Machine-readable output (r_lit CLI contract v1):\n  \
        --json works on every subcommand; stdout is one envelope {ok, tool, version, command, data, artifacts, error}.\n  \
        Exit codes: 0 ok, 1 runtime, 2 usage (bad color / effect spec), 3 input (font not found).\n  \
        `textexture __schema` prints a JSON Schema for every subcommand."
)]
struct Cli {
    /// Print one machine-readable JSON envelope to stdout / JSON 信封输出
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render text to image / 渲染文字为图片
    Render {
        /// Text to render / 要渲染的文字
        text: String,

        /// Output path / 输出路径
        #[arg(short, long, default_value = "textexture_output.png")]
        output: String,

        /// Font family name or .ttf/.otf file path / 字体名或字体文件路径
        #[arg(short, long)]
        font: Option<String>,

        /// Font size in pixels / 字号（像素）
        #[arg(short = 's', long, default_value_t = 72.0)]
        font_size: f32,

        /// Text color (CSS format) / 文字颜色
        #[arg(short, long, default_value = "#ffffff")]
        color: String,

        /// Background: color / gradient / image path / 背景：颜色/渐变/图片
        /// Single color: "#ff0000"
        /// Gradient: "#ff0000,#0000ff" or "#ff0000,#00ff00,#0000ff@45"
        /// Image: "./bg.jpg"
        #[arg(long, default_value = "#000000")]
        bg: String,

        /// Transparent background / 透明背景
        #[arg(long)]
        transparent: bool,

        /// Image width (auto if omitted) / 图片宽度
        #[arg(short = 'W', long)]
        width: Option<u32>,

        /// Image height (auto if omitted) / 图片高度
        #[arg(short = 'H', long)]
        height: Option<u32>,

        /// Padding in pixels / 内边距
        #[arg(long, default_value_t = 40)]
        padding: u32,

        /// Effect spec, repeatable: name:key=val,key=val / 效果规格，可重复.
        /// Effects: shadow(color,ox,oy,blur) outline(color,width)
        /// gradient(start,end,angle) glow(color,radius) neon(color,radius)
        /// grain(amount,seed,scale). See `list-effects --json`.
        #[arg(short, long = "effect")]
        effects: Vec<String>,
    },

    /// Bake stylized glyphs into a BMFont (.fnt + atlas) / 导出 BMFont 位图字体
    Bmfont {
        /// Characters to include (deduped) / 要收录的字符集
        chars: String,

        /// Output basename; writes <out>.fnt + <out>.png / 输出基名
        #[arg(short, long, default_value = "textexture_font")]
        output: String,

        /// Font family name or .ttf/.otf path / 字体名或路径
        #[arg(short, long)]
        font: Option<String>,

        /// Font size in pixels / 字号（像素）
        #[arg(short = 's', long, default_value_t = 72.0)]
        font_size: f32,

        /// Text color (CSS) — used when no fill effect / 文字颜色（无 fill 效果时）
        #[arg(short, long, default_value = "#ffffff")]
        color: String,

        /// Padding per glyph cell in px (room for effect bleed) / 每字内边距
        #[arg(long, default_value_t = 12)]
        padding: u32,

        /// Effect spec, repeatable: name:key=val,... / 效果规格，可重复.
        /// Same specs as `render -e` (see `list-effects --json`).
        #[arg(short, long = "effect")]
        effects: Vec<String>,
    },

    /// List available effects / 列出可用效果
    ListEffects,

    /// List available fonts / 列出可用字体
    ListFonts {
        /// Search filter / 搜索过滤
        #[arg(long)]
        search: Option<String>,
    },
}

/// Sidecar `.log` path for a run, derived from the command's output target.
/// `Render`/`Bmfont` attach the log next to their artifact (`<output>.log`);
/// the read-only `list-*` commands produce nothing, so they get no sidecar.
fn log_target(command: &Commands) -> Option<PathBuf> {
    match command {
        Commands::Render { output, .. } | Commands::Bmfont { output, .. } => {
            Some(PathBuf::from(output).with_extension("log"))
        }
        Commands::ListEffects | Commands::ListFonts { .. } => None,
    }
}

#[tokio::main]
async fn main() {
    runlog::init();

    let cli: Cli = CONTRACT.parse();
    let log_target = log_target(&cli.command);

    let result = run(&cli).await;

    // Flush the buffered run log next to the produced artifact (errors too).
    if let Some(path) = &log_target {
        runlog::flush(path, &runlog::standard_header());
        rlit_cli::add_artifact(path, "log");
    }

    rlit_cli::finish(result, error_kind);
}

/// Map errors onto the contract's exit-code classes.
fn error_kind(e: &error::AppError) -> ErrorKind {
    use error::AppError as E;
    match e {
        E::Io(io) => rlit_cli::io_error_kind(io),
        E::FontNotFound(_) | E::FontLoad(_) | E::Image(_) => ErrorKind::Input,
        E::ColorParse(_) | E::UnknownEffect(_) | E::InvalidEffectParam(_) => ErrorKind::Usage,
        E::Render(_) | E::PngEncode(_) => ErrorKind::Runtime,
    }
}

async fn run(cli: &Cli) -> error::Result<()> {
    match &cli.command {
        Commands::Render {
            text,
            output,
            font,
            font_size,
            color,
            bg,
            transparent,
            width,
            height,
            padding,
            effects,
        } => {
            let opts = render::RenderOpts {
                text: text.clone(),
                output: output.clone(),
                font: font.clone(),
                font_size: *font_size,
                color: color.clone(),
                bg: bg.clone(),
                transparent: *transparent,
                width: *width,
                height: *height,
                padding: *padding,
                effects: effects.clone(),
                json: cli.json,
            };
            render::execute(&opts)?;
        }
        Commands::Bmfont {
            chars,
            output,
            font,
            font_size,
            color,
            padding,
            effects,
        } => {
            let opts = bmfont::BmfontOpts {
                chars: chars.clone(),
                output: output.clone(),
                font: font.clone(),
                font_size: *font_size,
                color: color.clone(),
                padding: *padding,
                effects: effects.clone(),
                json: cli.json,
            };
            bmfont::execute(&opts)?;
        }
        Commands::ListEffects => {
            let effects = effect::list_effects();
            if cli.json {
                rlit_cli::set_data(&effects);
            } else {
                for e in &effects {
                    println!("  {} — {}", e.name, e.description);
                }
            }
        }
        Commands::ListFonts { search } => {
            let fonts = font::list_fonts(search.as_deref());
            if cli.json {
                rlit_cli::set_data(&fonts);
            } else {
                for f in &fonts {
                    println!("  {} ({})", f.family, f.style);
                }
            }
        }
    }
    Ok(())
}

// serde_json::Error → AppError
impl From<serde_json::Error> for error::AppError {
    fn from(e: serde_json::Error) -> Self {
        error::AppError::Render(format!("JSON serialization error: {}", e))
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use clap::CommandFactory;

    /// Every subcommand/arg must carry help text and a machine-call spec.
    #[test]
    fn cli_contract_is_complete() {
        let problems = rlit_cli::check_contract(&CONTRACT, &Cli::command());
        assert!(problems.is_empty(), "contract problems:\n{}", problems.join("\n"));
    }
}
