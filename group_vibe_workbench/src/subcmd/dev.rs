use crate::error::Result;
use std::process::{Command, Child};
use std::path::PathBuf;
use std::env;

/// Development mode: launch multiple instances for collaboration testing
pub fn run(count: u32, base_port: u16, width: u32, height: u32) -> Result<()> {
    log::info!("Starting {} instances for development testing", count);
    log::info!("Base port: {}, Window size: {}x{}", base_port, width, height);

    // Get the current executable path
    let exe_path = env::current_exe()
        .map_err(|e| crate::error::AppError::Other(format!("Failed to get executable path: {}", e)))?;

    let mut children: Vec<Child> = Vec::new();
    let user_names = vec!["Alice", "Bob", "Charlie", "David", "Eve", "Frank", "Grace", "Henry"];

    // Calculate window positions to tile them
    let screen_width = 1920; // Assume standard screen width
    let screen_height = 1080;
    let cols = (count as f32).sqrt().ceil() as u32;
    let rows = (count as f32 / cols as f32).ceil() as u32;

    for i in 0..count {
        let user_name = user_names.get(i as usize).unwrap_or(&"User").to_string();
        let port = base_port + i as u16;

        // Calculate window position for tiling
        let col = i % cols;
        let row = i / cols;
        let x_offset = (col * width) as i32;
        let y_offset = (row * height) as i32;

        // Create unique data directory for each instance
        let data_dir = format!("./workbench_data_dev_{}", i);
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| crate::error::AppError::Io(e))?;

        log::info!("Launching instance {}: {} (port: {}, pos: {}x{})",
                   i, user_name, port, x_offset, y_offset);

        // Launch instance with unique data directory
        let child = Command::new(&exe_path)
            .arg("launch")
            .arg("--width").arg(width.to_string())
            .arg("--height").arg(height.to_string())
            .arg("-n").arg(format!("{} (Dev{})", user_name, i))
            .env("WORKBENCH_DATA_DIR", &data_dir)
            .env("WORKBENCH_PORT", port.to_string())
            .env("WORKBENCH_WINDOW_X", x_offset.to_string())
            .env("WORKBENCH_WINDOW_Y", y_offset.to_string())
            .spawn()
            .map_err(|e| crate::error::AppError::Other(format!("Failed to spawn instance {}: {}", i, e)))?;

        children.push(child);

        // Small delay between launches to avoid race conditions
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║  Development Mode: {} Instances Running                    ", count);
    println!("╠════════════════════════════════════════════════════════════╣");
    println!("║                                                            ║");
    println!("║  Test Collaboration:                                       ║");
    println!("║  1. Create a group in one instance                         ║");
    println!("║  2. Join the group from other instances                    ║");
    println!("║  3. Click '开始协作' in all instances                       ║");
    println!("║  4. Edit ../chat.ctx to test file sync                     ║");
    println!("║                                                            ║");
    println!("║  Shared File: ../chat.ctx                                  ║");
    println!("║  Data Dirs: ./workbench_data_dev_0 to _{}                  ", count - 1);
    println!("║                                                            ║");
    println!("║  Press Ctrl+C to stop all instances                        ║");
    println!("║                                                            ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    // Wait for Ctrl+C
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        println!("\n\nReceived Ctrl+C, shutting down all instances...");
        tx.send(()).expect("Could not send signal");
    }).map_err(|e| crate::error::AppError::Other(format!("Failed to set Ctrl+C handler: {}", e)))?;

    // Wait for signal
    rx.recv().expect("Could not receive signal");

    // Kill all child processes
    for (i, mut child) in children.into_iter().enumerate() {
        log::info!("Stopping instance {}...", i);
        let _ = child.kill();
        let _ = child.wait();
    }

    println!("All instances stopped.");

    // Cleanup option
    println!("\nCleanup development data directories? (y/N): ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)
        .map_err(|e| crate::error::AppError::Io(e))?;

    if input.trim().eq_ignore_ascii_case("y") {
        for i in 0..count {
            let data_dir = format!("./workbench_data_dev_{}", i);
            if let Err(e) = std::fs::remove_dir_all(&data_dir) {
                log::warn!("Failed to remove {}: {}", data_dir, e);
            } else {
                log::info!("Removed {}", data_dir);
            }
        }
        println!("Development data cleaned up.");
    } else {
        println!("Development data preserved for next run.");
    }

    Ok(())
}
