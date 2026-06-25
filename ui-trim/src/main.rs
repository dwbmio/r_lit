use clap::Parser;
use std::path::PathBuf;
use ui_trim::{trim_file, TrimOptions};

#[derive(Parser, Debug)]
#[command(
    name = "ui-trim",
    version,
    about = "Clean pseudo-transparent UI asset backgrounds into tight PNGs",
    long_about = "Deterministic local cleaner for AI-generated UI assets.\n\n\
                  It removes edge-connected pseudo-transparent matte backgrounds,\n\
                  checkerboard-like white/gray pixels, optional red crop guides,\n\
                  then trims the final alpha bbox with padding."
)]
struct Cli {
    /// Input PNG path.
    #[arg(short = 'i', long, value_name = "PNG")]
    input: PathBuf,

    /// Output PNG path.
    #[arg(short = 'o', long, value_name = "PNG")]
    output: PathBuf,

    /// Padding to preserve around final alpha bbox.
    #[arg(long, default_value_t = 6)]
    padding: u32,

    /// Pixels with alpha <= this value are treated as background.
    #[arg(long, default_value_t = 4)]
    alpha_threshold: u8,

    /// Softens foreground alpha near removed background, in pixels.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(0..=3))]
    feather: u32,

    /// Max RGB distance from sampled edge matte clusters.
    #[arg(long, default_value_t = 48.0)]
    max_bg_distance: f32,

    /// Remove edge-connected red crop / guide lines.
    #[arg(long)]
    remove_red_guides: bool,

    /// Print JSON metadata to stdout.
    #[arg(long)]
    json: bool,
}

fn main() {
    let cli = Cli::parse();
    let options = TrimOptions {
        padding: cli.padding,
        alpha_threshold: cli.alpha_threshold,
        feather: cli.feather,
        max_bg_distance: cli.max_bg_distance,
        remove_red_guides: cli.remove_red_guides,
    };

    match trim_file(&cli.input, &cli.output, &options) {
        Ok(report) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serialize report")
                );
            } else {
                println!(
                    "ui-trim: {}x{} -> {}x{} bbox={:?} removed={} alpha_ratio={:.3} total_ms={:.2} throughput_mp_s={:.2}",
                    report.input_width,
                    report.input_height,
                    report.output_width,
                    report.output_height,
                    report.trim_bbox,
                    report.removed_pixels,
                    report.alpha_ratio,
                    report.timings_ms.total,
                    report.throughput_mp_s
                );
            }
        }
        Err(err) => {
            if cli.json {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "error": err.to_string(),
                    })
                );
            } else {
                eprintln!("ui-trim: {err}");
            }
            std::process::exit(1);
        }
    }
}
