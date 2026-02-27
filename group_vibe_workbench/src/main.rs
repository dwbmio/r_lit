mod error;
mod subcmd;

use clap::{Parser, Subcommand};
use error::Result;

#[derive(Parser)]
#[command(
    name = "group_vibe_workbench",
    version,
    about = "Group collaboration workbench with GPUI",
    long_about = "A desktop application for team collaboration and productivity.\n\n\
                  Built with GPUI for native performance and modern UI."
)]
struct Cli {
    /// Enable JSON format output
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the GUI application
    #[command(about = "Start the workbench GUI")]
    Launch {
        /// Window width
        #[arg(long, default_value = "1280")]
        width: u32,

        /// Window height
        #[arg(long, default_value = "720")]
        height: u32,
    },
}

fn init_logger() {
    let log_level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                message
            ))
        })
        .level(log_level)
        .chain(std::io::stdout())
        .apply()
        .expect("Failed to initialize logger");
}

fn main() -> Result<()> {
    init_logger();

    let cli = Cli::parse();

    match cli.command {
        Commands::Launch { width, height } => {
            log::info!("Launching workbench with dimensions: {}x{}", width, height);
            subcmd::launch::run(width, height)?;
        }
    }

    Ok(())
}
