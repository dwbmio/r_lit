//! Integration test for collaborative file editing
//!
//! This test verifies that multiple Murmur swarm instances can:
//! 1. Discover each other on the local network
//! 2. Synchronize file content changes via CRDT
//! 3. Converge to the same final state

use murmur::{Swarm, Result};
use std::time::Duration;
use tokio::time::sleep;

/// Test collaborative editing between two nodes
#[tokio::test]
async fn test_two_nodes_collaborative_editing() -> Result<()> {
    // Initialize logging for debugging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let group_id = format!("test_group_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs());

    println!("🧪 Test group ID: {}", group_id);

    // Create two swarm instances with different storage paths
    let swarm1 = Swarm::builder()
        .storage_path("/tmp/murmur_test_node1")
        .group_id(&group_id)
        .build()
        .await?;

    let swarm2 = Swarm::builder()
        .storage_path("/tmp/murmur_test_node2")
        .group_id(&group_id)
        .build()
        .await?;

    // Start both swarms
    println!("🚀 Starting swarm 1...");
    swarm1.start().await?;

    println!("🚀 Starting swarm 2...");
    swarm2.start().await?;

    // Wait for network initialization
    sleep(Duration::from_secs(2)).await;

    let node1_id = swarm1.node_id().await;
    let node2_id = swarm2.node_id().await;
    println!("📍 Node 1 ID: {}", node1_id);
    println!("📍 Node 2 ID: {}", node2_id);

    // Start advertising on local network
    println!("📡 Starting mDNS advertising...");
    let _discovery1 = swarm1.advertise_local("Node1").await?;
    let _discovery2 = swarm2.advertise_local("Node2").await?;

    // Wait for discovery
    sleep(Duration::from_secs(3)).await;

    // Discover and connect to peers
    println!("🔍 Discovering and connecting to peers...");
    let connected1 = swarm1.discover_and_connect(&group_id, 5).await?;
    let connected2 = swarm2.discover_and_connect(&group_id, 5).await?;

    println!("✅ Node 1 connected to {} peers", connected1.len());
    println!("✅ Node 2 connected to {} peers", connected2.len());

    for peer in &connected1 {
        println!("  Node 1 → {} ({})", peer.nickname, peer.node_id);
    }
    for peer in &connected2 {
        println!("  Node 2 → {} ({})", peer.nickname, peer.node_id);
    }

    // Wait for connections to stabilize
    sleep(Duration::from_secs(2)).await;

    // Test 1: Node 1 writes a file
    println!("\n📝 Test 1: Node 1 writes initial content");
    let key = "shared_file.txt";
    let content1 = b"Hello from Node 1!";
    swarm1.put(key, content1).await?;
    println!("✅ Node 1 wrote: {:?}", String::from_utf8_lossy(content1));

    // Wait for synchronization
    sleep(Duration::from_secs(2)).await;

    // Verify Node 2 received the update
    let value2 = swarm2.get(key).await?;
    println!("🔍 Node 2 read: {:?}", value2.as_ref().map(|v| String::from_utf8_lossy(v)));

    if let Some(v) = &value2 {
        assert_eq!(v, content1, "Node 2 should have received Node 1's content");
        println!("✅ Node 2 successfully received Node 1's content");
    } else {
        println!("❌ Node 2 did not receive content from Node 1");
        return Err(murmur::Error::Other("Synchronization failed".to_string()));
    }

    // Test 2: Node 2 updates the file
    println!("\n📝 Test 2: Node 2 updates content");
    let content2 = b"Hello from Node 2! Updated content.";
    swarm2.put(key, content2).await?;
    println!("✅ Node 2 wrote: {:?}", String::from_utf8_lossy(content2));

    // Wait for synchronization
    sleep(Duration::from_secs(2)).await;

    // Verify Node 1 received the update
    let value1 = swarm1.get(key).await?;
    println!("🔍 Node 1 read: {:?}", value1.as_ref().map(|v| String::from_utf8_lossy(v)));

    if let Some(v) = &value1 {
        assert_eq!(v, content2, "Node 1 should have received Node 2's updated content");
        println!("✅ Node 1 successfully received Node 2's updated content");
    } else {
        println!("❌ Node 1 did not receive updated content from Node 2");
        return Err(murmur::Error::Other("Synchronization failed".to_string()));
    }

    // Test 3: Concurrent writes (CRDT conflict resolution)
    println!("\n📝 Test 3: Concurrent writes");
    let content3_node1 = b"Concurrent write from Node 1";
    let content3_node2 = b"Concurrent write from Node 2";

    // Both nodes write simultaneously
    let (r1, r2) = tokio::join!(
        swarm1.put("concurrent_file.txt", content3_node1),
        swarm2.put("concurrent_file.txt", content3_node2)
    );
    r1?;
    r2?;
    println!("✅ Both nodes wrote concurrently");

    // Wait for CRDT to resolve conflicts
    sleep(Duration::from_secs(3)).await;

    // Both nodes should converge to the same value
    let final1 = swarm1.get("concurrent_file.txt").await?;
    let final2 = swarm2.get("concurrent_file.txt").await?;

    println!("🔍 Node 1 final: {:?}", final1.as_ref().map(|v| String::from_utf8_lossy(v)));
    println!("🔍 Node 2 final: {:?}", final2.as_ref().map(|v| String::from_utf8_lossy(v)));

    assert_eq!(final1, final2, "Both nodes should converge to the same value");
    println!("✅ CRDT successfully resolved concurrent writes");

    // Cleanup
    println!("\n🧹 Cleaning up...");
    swarm1.shutdown().await?;
    swarm2.shutdown().await?;

    // Clean up test data
    let _ = std::fs::remove_dir_all("/tmp/murmur_test_node1");
    let _ = std::fs::remove_dir_all("/tmp/murmur_test_node2");

    println!("✅ All tests passed!");
    Ok(())
}

/// Test that nodes in different groups don't interfere
#[tokio::test]
async fn test_group_isolation() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let group1 = format!("test_group_a_{}", timestamp);
    let group2 = format!("test_group_b_{}", timestamp);

    println!("🧪 Group 1: {}", group1);
    println!("🧪 Group 2: {}", group2);

    // Create nodes in different groups
    let swarm_a = Swarm::builder()
        .storage_path("/tmp/murmur_test_group_a")
        .group_id(&group1)
        .build()
        .await?;

    let swarm_b = Swarm::builder()
        .storage_path("/tmp/murmur_test_group_b")
        .group_id(&group2)
        .build()
        .await?;

    swarm_a.start().await?;
    swarm_b.start().await?;

    let _discovery_a = swarm_a.advertise_local("NodeA").await?;
    let _discovery_b = swarm_b.advertise_local("NodeB").await?;

    sleep(Duration::from_secs(3)).await;

    // Node A writes to its group
    swarm_a.put("group_file", b"Group A data").await?;
    sleep(Duration::from_secs(2)).await;

    // Node B should NOT see Node A's data
    let value_b = swarm_b.get("group_file").await?;
    assert!(value_b.is_none(), "Node B should not see Node A's data (different groups)");
    println!("✅ Group isolation verified");

    // Cleanup
    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all("/tmp/murmur_test_group_a");
    let _ = std::fs::remove_dir_all("/tmp/murmur_test_group_b");

    Ok(())
}
