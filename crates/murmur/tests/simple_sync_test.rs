//! Simplified test for CRDT synchronization without mDNS discovery
//!
//! This test focuses on verifying that the CRDT and storage layers work correctly,
//! without relying on network discovery or P2P connections.

use murmur::{Swarm, Result};
use std::time::Duration;
use tokio::time::sleep;

/// Test basic put/get operations on a single node
#[tokio::test]
async fn test_single_node_storage() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    println!("🧪 Test: Single node storage");

    // Create a swarm instance
    let swarm = Swarm::builder()
        .storage_path("/tmp/murmur_single_node_test")
        .group_id("test_single")
        .build()
        .await?;

    swarm.start().await?;

    let node_id = swarm.node_id().await;
    println!("📍 Node ID: {}", node_id);

    // Test 1: Put and get
    println!("\n📝 Test 1: Put and get");
    let key = "test_key";
    let value = b"test_value";
    swarm.put(key, value).await?;
    println!("✅ Put: key={}, value={:?}", key, String::from_utf8_lossy(value));

    let retrieved = swarm.get(key).await?;
    assert_eq!(retrieved.as_deref(), Some(value.as_ref()));
    println!("✅ Get: key={}, value={:?}", key, retrieved.as_ref().map(|v| String::from_utf8_lossy(v)));

    // Test 2: Update existing key
    println!("\n📝 Test 2: Update existing key");
    let new_value = b"updated_value";
    swarm.put(key, new_value).await?;
    println!("✅ Put: key={}, value={:?}", key, String::from_utf8_lossy(new_value));

    let retrieved = swarm.get(key).await?;
    assert_eq!(retrieved.as_deref(), Some(new_value.as_ref()));
    println!("✅ Get: key={}, value={:?}", key, retrieved.as_ref().map(|v| String::from_utf8_lossy(v)));

    // Test 3: Delete key
    println!("\n📝 Test 3: Delete key");
    swarm.delete(key).await?;
    println!("✅ Deleted: key={}", key);

    let retrieved = swarm.get(key).await?;
    assert!(retrieved.is_none());
    println!("✅ Get after delete: None");

    // Test 4: Multiple keys
    println!("\n📝 Test 4: Multiple keys");
    swarm.put("key1", b"value1").await?;
    swarm.put("key2", b"value2").await?;
    swarm.put("key3", b"value3").await?;
    println!("✅ Put 3 keys");

    let v1 = swarm.get("key1").await?;
    let v2 = swarm.get("key2").await?;
    let v3 = swarm.get("key3").await?;
    assert_eq!(v1.as_deref(), Some(b"value1".as_ref()));
    assert_eq!(v2.as_deref(), Some(b"value2".as_ref()));
    assert_eq!(v3.as_deref(), Some(b"value3".as_ref()));
    println!("✅ Retrieved all 3 keys correctly");

    // Cleanup
    swarm.shutdown().await?;
    let _ = std::fs::remove_dir_all("/tmp/murmur_single_node_test");

    println!("\n✅ All single-node tests passed!");
    Ok(())
}

/// Test that two nodes with different storage paths don't interfere
#[tokio::test]
async fn test_isolated_nodes() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    println!("🧪 Test: Isolated nodes (different groups)");

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create two nodes in different groups
    let swarm1 = Swarm::builder()
        .storage_path(format!("/tmp/murmur_isolated_1_{}", timestamp))
        .group_id(format!("group_a_{}", timestamp))
        .build()
        .await?;

    let swarm2 = Swarm::builder()
        .storage_path(format!("/tmp/murmur_isolated_2_{}", timestamp))
        .group_id(format!("group_b_{}", timestamp))
        .build()
        .await?;

    swarm1.start().await?;
    swarm2.start().await?;

    println!("📍 Node 1 ID: {}", swarm1.node_id().await);
    println!("📍 Node 2 ID: {}", swarm2.node_id().await);

    // Node 1 writes data
    swarm1.put("shared_key", b"node1_data").await?;
    println!("✅ Node 1 wrote data");

    // Node 2 should have its own independent storage
    let value2 = swarm2.get("shared_key").await?;
    assert!(value2.is_none(), "Node 2 should not see Node 1's data (different groups)");
    println!("✅ Node 2 correctly has no data (isolated)");

    // Node 2 writes its own data
    swarm2.put("shared_key", b"node2_data").await?;
    println!("✅ Node 2 wrote its own data");

    // Verify both nodes have their own data
    let value1 = swarm1.get("shared_key").await?;
    let value2 = swarm2.get("shared_key").await?;
    assert_eq!(value1.as_deref(), Some(b"node1_data".as_ref()));
    assert_eq!(value2.as_deref(), Some(b"node2_data".as_ref()));
    println!("✅ Both nodes have independent data");

    // Cleanup
    swarm1.shutdown().await?;
    swarm2.shutdown().await?;
    let _ = std::fs::remove_dir_all(format!("/tmp/murmur_isolated_1_{}", timestamp));
    let _ = std::fs::remove_dir_all(format!("/tmp/murmur_isolated_2_{}", timestamp));

    println!("\n✅ Isolation test passed!");
    Ok(())
}

/// Test persistence: data survives restart
#[tokio::test]
async fn test_persistence() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    println!("🧪 Test: Data persistence across restarts");

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let storage_path = format!("/tmp/murmur_persistence_test_{}", timestamp);
    let group_id = "test_persistence";

    // First session: write data
    {
        let swarm = Swarm::builder()
            .storage_path(&storage_path)
            .group_id(group_id)
            .build()
            .await?;

        swarm.start().await?;
        swarm.put("persistent_key", b"persistent_value").await?;
        println!("✅ Session 1: Wrote data");

        swarm.shutdown().await?;
        println!("✅ Session 1: Shutdown");

        // Explicitly drop swarm to ensure cleanup
        drop(swarm);
    }

    // Wait longer for file locks to be released
    println!("⏳ Waiting for file locks to be released...");
    sleep(Duration::from_secs(2)).await;

    // Second session: read data
    {
        let swarm = Swarm::builder()
            .storage_path(&storage_path)
            .group_id(group_id)
            .build()
            .await?;

        swarm.start().await?;

        let value = swarm.get("persistent_key").await?;
        assert_eq!(value.as_deref(), Some(b"persistent_value".as_ref()));
        println!("✅ Session 2: Read persisted data: {:?}", value.as_ref().map(|v| String::from_utf8_lossy(v)));

        swarm.shutdown().await?;
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&storage_path);

    println!("\n✅ Persistence test passed!");
    Ok(())
}
