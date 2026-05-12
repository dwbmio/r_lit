//! gamereel CLI — front door to the video generation pipeline.
//!
//! Two subcommands today:
//!   * `list-protocols` — prints every registered ProtocolParser
//!     (driven by `inventory::iter` so the binary auto-discovers
//!     every proto-* crate linked into it).
//!   * `render --protocol <name> --input <file> [--output <file>]`
//!     — parses the binary blob with the named protocol, prints the
//!     resulting `ParsedReplay`. Actual video rendering plumbed in a
//!     follow-up commit; this scaffolding proves the dispatch works.
//!
//! The link line in `Cargo.toml` decides which protocols are built in.
//! Removing a `proto-*` dep line removes that game from the binary
//! with no source edits.

use clap::{Parser, Subcommand};
use gamereel_core::protocol::{build_parser, registered_protocols};
use std::path::PathBuf;
use std::process::ExitCode;

// Force-link the proto-* crates so their `inventory::submit!`-emitted
// link-time constructors are actually pulled into the binary. Without
// these `use … as _` lines the linker strips the unused crates and the
// registry shows up empty at runtime — that's the trap the cli_e2e
// tests guard against.
use proto_bubble as _;
use proto_puzzle as _;

#[derive(Parser, Debug)]
#[command(
    name = "gamereel",
    version,
    about = "Generate short-form videos from game protocol replays.",
    long_about = "gamereel takes a binary game-protocol message blob (battle report, match \
                  result, replay frames) and renders it into a TikTok/IG-Reels-shaped MP4 \
                  via the gamereel-core engine. Each supported game lives in its own proto-* \
                  crate and self-registers at link time."
)]
struct Cli {
    /// Emit machine-readable JSON instead of human text. Matches the
    /// repo-wide CLI convention.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List every game protocol parser linked into this binary.
    ListProtocols,

    /// Parse a replay blob and render it through the chosen protocol.
    /// Render side is wired up after the M5 farm lands; today this
    /// proves dispatch by printing the parsed metadata.
    Render {
        /// Protocol name (see `list-protocols`).
        #[arg(long)]
        protocol: String,
        /// Path to the binary protocol message file.
        #[arg(long)]
        input: PathBuf,
        /// Where to write the resulting MP4 (default: parser-suggested name in cwd).
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if cli.json {
                let payload = serde_json::json!({"ok": false, "error": format!("{e}")});
                println!("{}", serde_json::to_string(&payload).unwrap_or_default());
            } else {
                eprintln!("error: {e}");
            }
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.cmd {
        Command::ListProtocols => list_protocols(cli.json),
        Command::Render { protocol, input, output } => {
            render(cli.json, protocol, input, output.as_deref())
        }
    }
}

fn list_protocols(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let regs = registered_protocols();
    if json {
        let payload: Vec<_> = regs
            .iter()
            .map(|d| serde_json::json!({"name": d.name, "description": d.description}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "ok": true,
            "protocols": payload,
        }))?);
    } else {
        println!("registered protocols ({}):", regs.len());
        for d in &regs {
            println!("  {:<10}  {}", d.name, d.description);
        }
        if regs.is_empty() {
            println!("  (none — make sure at least one proto-* crate is in apps/gamereel-cli/Cargo.toml)");
        }
    }
    Ok(())
}

fn render(
    json: bool,
    protocol: &str,
    input: &std::path::Path,
    _output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parser = build_parser(protocol)?;
    let bytes = std::fs::read(input)?;
    let replay = parser.parse(&bytes)?;
    if json {
        let payload = serde_json::json!({
            "ok": true,
            "protocol": parser.name(),
            "input": input,
            "input_bytes": bytes.len(),
            "suggested_filename": replay.suggested_filename,
            "frames": replay.frames,
            "metadata": replay.metadata,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("parsed via '{}':", parser.name());
        println!("  input bytes:        {}", bytes.len());
        println!("  suggested filename: {}", replay.suggested_filename);
        println!("  frames:             {}", replay.frames);
        println!("  metadata:           {}", serde_json::to_string(&replay.metadata)?);
        println!();
        println!("(actual MP4 render plumbed once M5 farm lands; this command proves dispatch.)");
    }
    Ok(())
}
