use murmur::Swarm;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Get configuration from args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <node-name> [peer-addr]", args[0]);
        eprintln!("\nExample:");
        eprintln!("  Terminal 1: cargo run --example group_chat alice");
        eprintln!("  Terminal 2: cargo run --example group_chat bob <alice-addr>");
        std::process::exit(1);
    }

    let node_name = &args[1];
    let peer_addr = args.get(2);

    println!("🚀 Starting node: {}", node_name);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Create swarm with group ID
    let swarm = Swarm::builder()
        .storage_path(format!("./data/{}", node_name))
        .group_id("my-chat-room")  // 同一个群组
        .build()
        .await?;

    // Start the swarm
    swarm.start().await?;

    // Display connection info
    println!("\n📍 Node Address (share this with others):");
    println!("   {}", serde_json::to_string(&swarm.node_addr().await?).unwrap_or_default());
    println!("\n🆔 Node ID: {}", swarm.node_id().await);

    // Connect to peer if provided
    if let Some(addr_str) = peer_addr {
        println!("\n🔗 Connecting to peer: {}", addr_str);
        let addr: murmur::NodeAddr = serde_json::from_str(addr_str)?;
        swarm.connect_peer(&addr).await?;
        println!("✓ Connected!");
    }

    // Wait for election
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Show role
    println!("\n👑 Role:");
    if swarm.is_leader().await {
        println!("   I am the LEADER");
    } else if let Some(leader_id) = swarm.leader_id().await {
        println!("   I am a follower (leader: {})", &leader_id[..8]);
    }

    // Show connected peers
    let peers = swarm.connected_peers().await;
    println!("\n👥 Connected peers: {}", peers.len());
    for peer in &peers {
        println!("   - {}", &peer[..16]);
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💬 Chat Room Ready!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Simulate sending messages
    let message_key = format!("msg:{}", node_name);
    let message_value = format!("Hello from {}!", node_name);

    println!("📤 Broadcasting message: {}", message_value);
    swarm.put(&message_key, message_value.as_bytes()).await?;

    // Wait a bit for sync
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Read all messages
    println!("\n📥 Messages in the room:");
    for peer_name in &["alice", "bob", "charlie"] {
        let key = format!("msg:{}", peer_name);
        if let Some(value) = swarm.get(&key).await? {
            println!("   {} says: {}", peer_name, String::from_utf8_lossy(&value));
        }
    }

    // Keep running
    println!("\n⏳ Running (press Ctrl+C to stop)...\n");
    tokio::signal::ctrl_c().await?;

    println!("\n👋 Shutting down...");
    swarm.shutdown().await?;

    Ok(())
}
