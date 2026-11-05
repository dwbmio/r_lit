use clap::Parser;
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io};
use std::path::PathBuf;
use ui_trim::{trim_file, TrimOptions, UiTrimError};

/// Machine-call annotations for `__schema` (see `docs/cli-contract.md`).
static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[CommandSpec::new("", Effects::Pure)
        .io(&[("input", Io::InputFile), ("output", Io::OutputFile)])
        .note("Deterministic pixel tool: one PNG in, one tight RGBA PNG out. Check data.alpha_ratio: near 1.0 means the background was probably not removed; very low means over-trim.")],
};

#[derive(Parser, Debug)]
#[command(
    name = "ui-trim",
    version,
    about = "Clean pseudo-transparent UI asset backgrounds into tight PNGs",
    long_about = "Deterministic local cleaner for AI-generated UI assets.\n\n\
                  It removes edge-connected pseudo-transparent matte backgrounds,\n\
                  checkerboard-like white/gray pixels, optional red crop guides,\n\
                  then trims the final alpha bbox with padding.\n\n\
                  Examples:\n  \
                  ui-trim -i raw.png -o clean.png\n  \
                  ui-trim -i raw.png -o clean.png --padding 12 --feather 1 --json\n  \
                  ui-trim -i raw.png -o clean.png --remove-red-guides --max-bg-distance 64",
    after_help = "Machine-readable output (r_lit CLI contract v1):\n  \
                  With --json, stdout is one envelope {ok, tool, version, command, data, artifacts, error}.\n  \
                  data = trim report: input/output size, trim_bbox [x0,y0,x1,y1], removed_pixels,\n  \
                  alpha_ratio, options, timings_ms, throughput_mp_s.\n  \
                  Exit codes: 0 ok, 1 runtime, 2 usage, 3 input (unreadable PNG).\n  \
                  `ui-trim __schema` prints a JSON Schema of the arguments."
)]
struct Cli {
    /// Input PNG path (AI-generated asset with a fake/matte background).
    #[arg(short = 'i', long, value_name = "PNG")]
    input: PathBuf,

    /// Output PNG path. Parent directories are created; existing file is overwritten.
    #[arg(short = 'o', long, value_name = "PNG")]
    output: PathBuf,

    /// Padding (px) kept around the final alpha bbox. Icons/buttons: 4-6;
    /// glow or bubble shapes: 8-16.
    #[arg(long, default_value_t = 6, value_name = "PX")]
    padding: u32,

    /// Pixels with alpha <= this value are treated as background. Keep low
    /// (default 4) to avoid deleting faint glow.
    #[arg(long, default_value_t = 4, value_name = "0-255")]
    alpha_threshold: u8,

    /// Softens foreground alpha near removed background, in pixels (0-3).
    /// Higher values may shrink thin highlights.
    #[arg(long, default_value_t = 2, value_name = "0-3", value_parser = clap::value_parser!(u32).range(0..=3))]
    feather: u32,

    /// Max RGB distance from the sampled edge matte colours. Raise for an
    /// uneven warm matte; lower if pale UI edges are being removed.
    #[arg(long, default_value_t = 48.0, value_name = "DIST")]
    max_bg_distance: f32,

    /// Also remove red crop / guide lines, but only where they touch the
    /// removed background (edge-connected).
    #[arg(long)]
    remove_red_guides: bool,

    /// Print one machine-readable JSON envelope to stdout (r_lit CLI contract).
    #[arg(long, global = true)]
    json: bool,
}

fn error_kind(e: &UiTrimError) -> ErrorKind {
    match e {
        UiTrimError::ReadImage { .. } | UiTrimError::DimensionOverflow => ErrorKind::Input,
        UiTrimError::WriteImage { .. } | UiTrimError::CreateOutputDir { .. } => ErrorKind::Runtime,
    }
}

fn main() {
    let cli: Cli = CONTRACT.parse();
    let options = TrimOptions {
        padding: cli.padding,
        alpha_threshold: cli.alpha_threshold,
        feather: cli.feather,
        max_bg_distance: cli.max_bg_distance,
        remove_red_guides: cli.remove_red_guides,
    };

    let result = trim_file(&cli.input, &cli.output, &options).map(|report| {
        rlit_cli::add_artifact(&cli.output, "image");
        if !cli.json {
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
        rlit_cli::set_data(report);
    });
    rlit_cli::finish(result, error_kind);
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_contract_is_complete() {
        let problems = rlit_cli::check_contract(&CONTRACT, &Cli::command());
        assert!(problems.is_empty(), "contract problems:\n{}", problems.join("\n"));
    }
}
