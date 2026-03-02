//! File synchronization example with version control
//!
//! Demonstrates version control, conflict detection, and audit trail features.

use murmur::{Swarm, FileOps, Result};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 Murmur File Sync with Version Control");
    println!("=========================================\n");

    // Create swarm
    let swarm = Swarm::builder()
        .storage_path("/tmp/murmur_file_sync")
        .group_id("file_sync_demo")
        .build()
        .await?;

    swarm.start().await?;

    println!("✅ Swarm started");
    println!("   Node ID: {}\n", swarm.node_id().await);

    // Wait for peer discovery
    println!("⏳ Waiting for peer discovery (5 seconds)...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // Connect to discovered peers
    let count = swarm.discover_and_connect_local_peers().await?;
    println!("✅ Connected to {} peer(s)\n", count);

    // Interactive mode
    println!("📝 File Sync Commands:");
    println!("  upload <file_path>              - Upload a file (auto-increment version)");
    println!("  upload-safe <file_path> <ver>   - Upload with version check");
    println!("  download <key> <output_path>    - Download latest version");
    println!("  download-ver <key> <ver> <out>  - Download specific version");
    println!("  info <key>                      - Show file metadata");
    println!("  history <key>                   - Show version history");
    println!("  list                            - List all files");
    println!("  delete <key>                    - Delete a file");
    println!("  audit                           - Show audit trail");
    println!("  quit                            - Exit\n");

    loop {
        print!("> ");
        use std::io::{self, Write};
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let parts: Vec<&str> = input.trim().split_whitespace().collect();

        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "upload" => {
                if parts.len() < 2 {
                    println!("❌ Usage: upload <file_path>");
                    continue;
                }
                let file_path = Path::new(parts[1]);
                match swarm.put_file(file_path).await {
                    Ok(key) => {
                        if let Ok(Some(meta)) = swarm.file_metadata(&key).await {
                            println!("✅ Uploaded: {}", key);
                            println!("   Version: {}", meta.version);
                            println!("   Size: {} bytes", meta.size);
                            println!("   Author: {}", meta.author);
                        }
                    }
                    Err(murmur::Error::FileTooLarge { size, max }) => {
                        println!("❌ File too large: {} bytes (max: {} bytes)", size, max);
                        println!("   Hint: Use chunked upload or compress the file");
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "upload-safe" => {
                if parts.len() < 3 {
                    println!("❌ Usage: upload-safe <file_path> <expected_version>");
                    continue;
                }
                let file_path = Path::new(parts[1]);
                let expected_version: u64 = match parts[2].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("❌ Invalid version number");
                        continue;
                    }
                };

                match swarm.put_file_with_version(file_path, Some(expected_version)).await {
                    Ok(key) => {
                        if let Ok(Some(meta)) = swarm.file_metadata(&key).await {
                            println!("✅ Uploaded: {}", key);
                            println!("   Version: {} (was {})", meta.version, expected_version);
                            println!("   Size: {} bytes", meta.size);
                        }
                    }
                    Err(murmur::Error::VersionConflict { expected, current }) => {
                        println!("❌ Version conflict:");
                        println!("   Expected: {}", expected);
                        println!("   Current: {}", current);
                        println!("   Hint: Use 'info <key>' to see current version and author");
                    }
                    Err(murmur::Error::FileTooLarge { size, max }) => {
                        println!("❌ File too large: {} bytes (max: {} bytes)", size, max);
                    }
                    Err(e) => {
                        println!("❌ Error: {}", e);
                    }
                }
            }

            "download" => {
                if parts.len() < 3 {
                    println!("❌ Usage: download <key> <output_path>");
                    continue;
                }
                let key = parts[1];
                let output_path = Path::new(parts[2]);
                match swarm.get_file(key, output_path).await {
                    Ok(_) => println!("✅ Downloaded to: {}", output_path.display()),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "download-ver" => {
                if parts.len() < 4 {
                    println!("❌ Usage: download-ver <key> <version> <output_path>");
                    continue;
                }
                let key = parts[1];
                let version: u64 = match parts[2].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("❌ Invalid version number");
                        continue;
                    }
                };
                let output_path = Path::new(parts[3]);
                match swarm.get_file_version(key, version, output_path).await {
                    Ok(_) => println!("✅ Downloaded version {} to: {}", version, output_path.display()),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "info" => {
                if parts.len() < 2 {
                    println!("❌ Usage: info <key>");
                    continue;
                }
                let key = parts[1];
                match swarm.file_metadata(key).await {
                    Ok(Some(meta)) => {
                        println!("📄 File: {}", meta.name);
                        println!("   Version: {}", meta.version);
                        println!("   Size: {} bytes", meta.size);
                        println!("   Modified: {}", meta.modified);
                        println!("   Author: {}", meta.author);
                        println!("   Checksum: {}", meta.checksum);
                    }
                    Ok(None) => println!("❌ File not found"),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "history" => {
                if parts.len() < 2 {
                    println!("❌ Usage: history <key>");
                    continue;
                }
                let key = parts[1];
                match swarm.file_history(key).await {
                    Ok(history) => {
                        if history.is_empty() {
                            println!("📜 No history found");
                        } else {
                            println!("📜 Version History:");
                            for entry in history.iter().rev() {
                                let op_symbol = match entry.operation {
                                    murmur::file::FileOperation::Create => "➕",
                                    murmur::file::FileOperation::Update => "✏️",
                                    murmur::file::FileOperation::Delete => "🗑️",
                                };
                                println!("   {} v{} - {} bytes by {} at {}",
                                    op_symbol,
                                    entry.version,
                                    entry.size,
                                    entry.author,
                                    entry.timestamp
                                );
                            }
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "list" => {
                match swarm.list_files().await {
                    Ok(files) => {
                        if files.is_empty() {
                            println!("📂 No files found (list_files not fully implemented yet)");
                        } else {
                            println!("📂 Files:");
                            for file in files {
                                println!("   {} (v{}, {} bytes)", file.name, file.version, file.size);
                            }
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "delete" => {
                if parts.len() < 2 {
                    println!("❌ Usage: delete <key>");
                    continue;
                }
                let key = parts[1];
                match swarm.delete_file(key).await {
                    Ok(_) => println!("✅ Deleted: {}", key),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "audit" => {
                match swarm.audit_trail(Some(20)).await {
                    Ok(trail) => {
                        if trail.is_empty() {
                            println!("📋 No audit trail (audit_trail not fully implemented yet)");
                        } else {
                            println!("📋 Recent Operations:");
                            for entry in trail.iter().rev().take(20) {
                                let op_name = match entry.operation {
                                    murmur::file::FileOperation::Create => "CREATE",
                                    murmur::file::FileOperation::Update => "UPDATE",
                                    murmur::file::FileOperation::Delete => "DELETE",
                                };
                                println!("   {} - {} by {} at {}",
                                    op_name,
                                    entry.content_key,
                                    entry.author,
                                    entry.timestamp
                                );
                            }
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }

            "quit" | "exit" => {
                println!("👋 Shutting down...");
                swarm.shutdown().await?;
                break;
            }

            _ => {
                println!("❌ Unknown command: {}", parts[0]);
            }
        }
    }

    Ok(())
}
