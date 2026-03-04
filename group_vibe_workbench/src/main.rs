mod error;
mod subcmd;
mod shared_file;
mod gui;
mod user_db;
mod config;
mod swarm_manager;
mod instance_lock;
mod model;
mod controller;

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

        /// Your nickname (optional, will prompt if not provided)
        #[arg(long, short = 'n')]
        nickname: Option<String>,
    },

    /// Development mode: launch multiple instances for testing collaboration
    #[command(about = "Launch multiple instances for collaboration testing")]
    Dev {
        /// Number of instances to launch
        #[arg(long, short = 'c', default_value = "2")]
        count: u32,

        /// Base port for instances (each instance uses port + index)
        #[arg(long, default_value = "9000")]
        base_port: u16,

        /// Window width
        #[arg(long, default_value = "800")]
        width: u32,

        /// Window height
        #[arg(long, default_value = "600")]
        height: u32,
    },
}

fn init_logger() {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}][{}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Warn)
        .level_for("group_vibe_workbench", log::LevelFilter::Info)
        .level_for("murmur", log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()
        .expect("Failed to initialize logger");
}

fn main() -> Result<()> {
    init_logger();

    // Try to acquire instance lock (only enforced in release mode)
    let _instance_lock = match instance_lock::InstanceLock::try_acquire() {
        Ok(lock) => {
            if !cfg!(debug_assertions) {
                log::info!("✅ Instance lock acquired");
            }
            lock
        }
        Err(e) => {
            // Show alert dialog in release mode
            if !cfg!(debug_assertions) {
                show_already_running_alert();
            }
            return Err(error::AppError::Other(format!(
                "Application is already running: {}",
                e
            )));
        }
    };

    let cli = Cli::parse();

    match cli.command {
        Commands::Launch { width, height, nickname } => {
            log::info!("Launching workbench with dimensions: {}x{}", width, height);
            subcmd::launch::run(width, height, nickname)?;
        }
        Commands::Dev { count, base_port, width, height } => {
            log::info!("Launching {} instances for development testing", count);
            subcmd::dev::run(count, base_port, width, height)?;
        }
    }

    Ok(())
}

/// Show alert dialog when another instance is already running
fn show_already_running_alert() {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(r#"display dialog "Group Vibe Workbench is already running.\n\nOnly one instance can run at a time." buttons {"OK"} default button "OK" with icon caution with title "Application Already Running""#)
            .output();
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let _ = Command::new("msg")
            .arg("*")
            .arg("Group Vibe Workbench is already running. Only one instance can run at a time.")
            .output();
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        // Try zenity first, then kdialog, then xmessage
        let _ = Command::new("zenity")
            .arg("--error")
            .arg("--title=Application Already Running")
            .arg("--text=Group Vibe Workbench is already running.\n\nOnly one instance can run at a time.")
            .output()
            .or_else(|_| {
                Command::new("kdialog")
                    .arg("--error")
                    .arg("Group Vibe Workbench is already running.\n\nOnly one instance can run at a time.")
                    .output()
            })
            .or_else(|_| {
                Command::new("xmessage")
                    .arg("-center")
                    .arg("Group Vibe Workbench is already running.\n\nOnly one instance can run at a time.")
                    .output()
            });
    }

    // Also log to stderr
    eprintln!("❌ Group Vibe Workbench is already running.");
    eprintln!("   Only one instance can run at a time.");
}
