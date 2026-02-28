//! Manual test for collaborative editing using iroh's LocalSwarmDiscovery
//!
//! Run this in two separate terminals to test P2P collaboration

use murmur::{Swarm, Result};
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::new("info,iroh_net::netcheck=warn")
        )
        .init();

    println!("🚀 Murmur Collaborative Editing Test");
    println!("=====================================\n");

    // Get user input
    print!("Enter your nickname: ");
    io::stdout().flush().unwrap();
    let mut nickname = String::new();
    io::stdin().read_line(&mut nickname).unwrap();
    let nickname = nickname.trim();

    print!("Enter group ID (or press Enter for 'test_group'): ");
    io::stdout().flush().unwrap();
    let mut group_id = String::new();
    io::stdin().read_line(&mut group_id).unwrap();
    let group_id = if group_id.trim().is_empty() {
        "test_group".to_string()
    } else {
        group_id.trim().to_string()
    };

    let storage_path = format!("/tmp/murmur_manual_test_{}", nickname.replace(" ", "_"));

    println!("\n📦 Creating Swarm...");
    println!("   Storage: {}", storage_path);
    println!("   Group: {}", group_id);

    // Create and start swarm
    let swarm = Swarm::builder()
        .storage_path(&storage_path)
        .group_id(&group_id)
        .build()
        .await?;

    swarm.start().await?;

    let node_id = swarm.node_id().await;
    println!("✅ Swarm started");
    println!("   Node ID: {}", node_id);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 Connection Information");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("   Nickname: {}", nickname);
    println!("   Group ID: {}", group_id);
    println!("   Node ID:  {}", node_id);
    println!();
    println!("   iroh's LocalSwarmDiscovery is running automatically");
    println!("   Peers on the same network will be discovered automatically");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    // Wait a bit for discovery to work
    println!("⏳ Waiting for peer discovery (10 seconds)...");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // Try to connect to discovered peers
    println!("🔍 Connecting to discovered peers...");
    match swarm.discover_and_connect_local_peers().await {
        Ok(count) if count > 0 => {
            println!("✅ Connected to {} new peer(s)", count);
        }
        Ok(_) => {
            println!("⚠️  No new peers found.");
        }
        Err(e) => {
            println!("❌ Discovery error: {}", e);
        }
    }

    // Check connected peers
    let peers = swarm.connected_peers().await;
    if peers.is_empty() {
        println!("⚠️  No peers connected yet.");
        println!("   Make sure another instance is running on the same network.");
    } else {
        println!("✅ Connected to {} peer(s):", peers.len());
        for peer in &peers {
            println!("   - {}", peer);
        }
    }

    // Interactive mode
    println!("\n📝 Interactive Mode");
    println!("Commands:");
    println!("  write <key> <value> - Write a value");
    println!("  read <key>          - Read a value");
    println!("  list                - List connected peers");
    println!("  connect <node_id>   - Manually connect to a peer by Node ID");
    println!("  info                - Show this node's information");
    println!("  quit                - Exit");
    println!();

    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.get(0).map(|s| *s) {
            Some("write") if parts.len() >= 3 => {
                let key = parts[1];
                let value = parts[2..].join(" ");
                match swarm.put(key, value.as_bytes()).await {
                    Ok(()) => println!("✅ Wrote: {} = {}", key, value),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
            Some("read") if parts.len() == 2 => {
                let key = parts[1];
                match swarm.get(key).await {
                    Ok(Some(value)) => {
                        let value_str = String::from_utf8_lossy(&value);
                        println!("✅ Read: {} = {}", key, value_str);
                    }
                    Ok(None) => println!("⚠️  Key not found: {}", key),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
            Some("list") => {
                let peers = swarm.connected_peers().await;
                if peers.is_empty() {
                    println!("No connected peers");
                } else {
                    println!("Connected peers ({}):", peers.len());
                    for peer in peers {
                        println!("  - {}", peer);
                    }
                }
            }
            Some("connect") if parts.len() == 2 => {
                let node_id = parts[1];
                println!("Connecting to {}...", node_id);
                match swarm.connect_peer_by_id(node_id).await {
                    Ok(()) => println!("✅ Connected"),
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
            Some("info") => {
                let node_id = swarm.node_id().await;
                let is_leader = swarm.is_leader().await;
                let leader_id = swarm.leader_id().await;
                let peers = swarm.connected_peers().await;

                println!("Node Information:");
                println!("  Node ID: {}", node_id);
                println!("  Is Leader: {}", is_leader);
                println!("  Leader ID: {:?}", leader_id);
                println!("  Connected Peers: {}", peers.len());
            }
            Some("quit") => {
                println!("Shutting down...");
                swarm.shutdown().await?;
                break;
            }
            _ => {
                println!("Unknown command. Available commands:");
                println!("  write <key> <value>");
                println!("  read <key>");
                println!("  list");
                println!("  connect <node_id>");
                println!("  info");
                println!("  quit");
            }
        }
    }

    Ok(())
}
