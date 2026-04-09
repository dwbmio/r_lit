//! End-to-end test simulating the workbench search flow:
//!
//! 1. Node A creates a group and announces itself
//! 2. Node B creates a "_search_" swarm, discovers peers, connects, syncs
//! 3. Node B reads announced peers and finds Node A's group

use murmur::Swarm;
use std::time::Duration;

#[tokio::test]
async fn test_search_discovers_created_group() -> murmur::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let real_group = format!("my_project_{}", ts);
    let path_a = format!("/tmp/murmur_search_a_{}", ts);
    let path_b = format!("/tmp/murmur_search_b_{}", ts);

    // === Node A: Create group and announce ===
    println!("--- Node A: Creating group '{}' ---", real_group);
    let swarm_a = Swarm::builder()
        .storage_path(&path_a)
        .group_id(&real_group)
        .build()
        .await?;
    swarm_a.start().await?;
    swarm_a.announce("Alice").await?;
    println!("Node A announced as Alice, id={}", swarm_a.node_id().await);

    // === Node B: Search mode (temporary group) ===
    println!("\n--- Node B: Starting search swarm ---");
    let swarm_b = Swarm::builder()
        .storage_path(&path_b)
        .group_id("_search_")
        .build()
        .await?;
    swarm_b.start().await?;
    println!("Node B search swarm started, id={}", swarm_b.node_id().await);

    // Wait for discovery
    println!("\n--- Waiting 8s for local discovery ---");
    tokio::time::sleep(Duration::from_secs(8)).await;

    let count = swarm_b.discover_and_connect_local_peers().await?;
    println!("Node B connected to {} peer(s)", count);

    // Wait for CRDT sync
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Read announced peers
    let peers = swarm_b.list_announced_peers().await?;
    println!("\nNode B sees {} announced peer(s):", peers.len());
    for p in &peers {
        println!("  node={} nick={} group={}", &p.node_id[..16], p.nickname, p.group_id);
    }

    // Group by group_id, filtering out internal groups
    let mut groups: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (_, nickname, group_id) in &peers {
        if !group_id.starts_with('_') {
            groups.entry(group_id.clone())
                .or_default()
                .push(nickname.clone());
        }
    }

    println!("\nDiscovered groups:");
    for (gid, members) in &groups {
        println!("  {} ({} members): {:?}", gid, members.len(), members);
    }

    // Verify we found the real group
    assert!(
        groups.contains_key(&real_group),
        "Should have found group '{}' but only found: {:?}",
        real_group,
        groups.keys().collect::<Vec<_>>()
    );
    assert!(
        groups[&real_group].contains(&"Alice".to_string()),
        "Group should contain Alice"
    );

    // Cleanup
    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b);

    println!("\n--- Search flow test passed! ---");
    Ok(())
}
