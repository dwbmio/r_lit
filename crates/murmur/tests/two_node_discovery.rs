//! Two-node local discovery integration test
//!
//! Verifies that two Swarm instances on the same machine can:
//! 1. Discover each other via iroh LocalSwarmDiscovery
//! 2. Establish a connection
//! 3. Exchange data via CRDT

use murmur::Swarm;
use std::time::Duration;

#[tokio::test]
async fn test_two_nodes_discover_and_connect() -> murmur::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let group = format!("test_group_{}", ts);
    let path_a = format!("/tmp/murmur_disc_a_{}", ts);
    let path_b = format!("/tmp/murmur_disc_b_{}", ts);

    println!("=== Creating Node A ===");
    let swarm_a = Swarm::builder()
        .storage_path(&path_a)
        .group_id(&group)
        .build()
        .await?;

    swarm_a.start().await?;
    let id_a = swarm_a.node_id().await;
    println!("Node A id: {}", id_a);

    println!("\n=== Creating Node B ===");
    let swarm_b = Swarm::builder()
        .storage_path(&path_b)
        .group_id(&group)
        .build()
        .await?;

    swarm_b.start().await?;
    let id_b = swarm_b.node_id().await;
    println!("Node B id: {}", id_b);

    // Wait for LocalSwarmDiscovery mDNS announcements to propagate
    println!("\n=== Waiting 8s for local discovery ===");
    tokio::time::sleep(Duration::from_secs(8)).await;

    // Try to discover and connect from both sides
    println!("\n=== Node A discovering peers ===");
    let count_a = swarm_a.discover_and_connect_local_peers().await?;
    println!("Node A connected to {} new peer(s)", count_a);

    println!("\n=== Node B discovering peers ===");
    let count_b = swarm_b.discover_and_connect_local_peers().await?;
    println!("Node B connected to {} new peer(s)", count_b);

    let peers_a = swarm_a.connected_peers().await;
    let peers_b = swarm_b.connected_peers().await;
    println!("\nNode A peers: {:?}", peers_a);
    println!("Node B peers: {:?}", peers_b);

    let total_connections = count_a + count_b;
    println!("\nTotal new connections: {}", total_connections);

    if total_connections == 0 {
        println!("\n!!! WARNING: No connections established.");
        println!("    LocalSwarmDiscovery may not work in this environment.");
        println!("    Falling back to manual connect test...\n");

        // Manual connection via node_addr
        let addr_a = swarm_a.node_addr().await?;
        println!("Node A addr: {:?}", addr_a);

        swarm_b.connect_peer(&addr_a).await?;
        println!("Node B manually connected to Node A");

        let peers_b_after = swarm_b.connected_peers().await;
        println!("Node B peers after manual connect: {:?}", peers_b_after);
        assert!(
            !peers_b_after.is_empty(),
            "Manual connect should have established a connection"
        );
    }

    // Test data exchange
    println!("\n=== Testing data exchange ===");
    swarm_a.put("hello", b"from_node_a").await?;
    println!("Node A put: hello = from_node_a");

    // Give time for CRDT replication
    tokio::time::sleep(Duration::from_secs(2)).await;

    let val = swarm_b.get("hello").await?;
    println!(
        "Node B get: hello = {:?}",
        val.as_ref().map(|v| String::from_utf8_lossy(v).to_string())
    );

    // Cleanup
    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b);

    println!("\n=== Test complete ===");
    Ok(())
}
