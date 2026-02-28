use murmur::Swarm;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Get node name from args or use default
    let node_name = env::args().nth(1).unwrap_or_else(|| "node1".to_string());
    let storage_path = format!("./data/{}", node_name);

    println!("Starting {} with storage at {}", node_name, storage_path);

    // Create and start swarm
    let swarm = Swarm::builder()
        .storage_path(&storage_path)
        .build()
        .await?;

    println!("Node ID: {}", swarm.node_id().await);

    swarm.start().await?;
    println!("Swarm started!");

    // Wait a bit for election to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Check leadership
    if swarm.is_leader().await {
        println!("✓ I am the LEADER");
    } else if let Some(leader_id) = swarm.leader_id().await {
        println!("✓ I am a follower. Leader is: {}", leader_id);
    } else {
        println!("⚠ Election in progress...");
    }

    // Perform some operations
    println!("\n--- Testing KV operations ---");

    // Put some values
    swarm.put("greeting", b"Hello from Murmur!").await?;
    println!("✓ Put: greeting = 'Hello from Murmur!'");

    swarm.put(&format!("node:{}", node_name), node_name.as_bytes()).await?;
    println!("✓ Put: node:{} = '{}'", node_name, node_name);

    // Get values
    if let Some(value) = swarm.get("greeting").await? {
        println!("✓ Get: greeting = '{}'", String::from_utf8_lossy(&value));
    }

    if let Some(value) = swarm.get(&format!("node:{}", node_name)).await? {
        println!("✓ Get: node:{} = '{}'", node_name, String::from_utf8_lossy(&value));
    }

    // Keep running
    println!("\n--- Running (press Ctrl+C to stop) ---");
    tokio::signal::ctrl_c().await?;

    println!("\nShutting down...");
    swarm.shutdown().await?;

    Ok(())
}
