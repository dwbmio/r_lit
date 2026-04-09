mod color;
mod effect;
mod error;
mod font;
mod render;

use clap::{Parser, Subcommand};

/// Generate stylized text images with visual effects
/// 生成带视觉效果的艺术字图片
#[derive(Parser)]
#[command(name = "textexture", version, about, long_about = None)]
struct Cli {
    /// Output in JSON format / JSON 格式输出
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

        /// Effect spec, repeatable: name:key=val,key=val / 效果规格，可重复
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

fn init_logger() -> std::result::Result<(), Box<dyn std::error::Error>> {
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
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = init_logger() {
        eprintln!("Failed to init logger: {}", e);
    }

    let cli = Cli::parse();

    let result = run(&cli).await;

    match result {
        Ok(()) => {}
        Err(e) => {
            if cli.json {
                eprintln!(
                    "{}",
                    serde_json::json!({"status": "error", "error": format!("{}", e)})
                );
            } else {
                log::error!("{}", e);
            }
            std::process::exit(1);
        }
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
        Commands::ListEffects => {
            let effects = effect::list_effects();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&effects)?);
            } else {
                for e in &effects {
                    println!("  {} — {}", e.name, e.description);
                }
            }
        }
        Commands::ListFonts { search } => {
            let fonts = font::list_fonts(search.as_deref());
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&fonts)?);
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
