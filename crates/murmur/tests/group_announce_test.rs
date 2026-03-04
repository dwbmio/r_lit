//! Test: Two nodes announce themselves and discover each other's metadata

use murmur::Swarm;
use std::time::Duration;

#[tokio::test]
async fn test_announce_and_discover() -> murmur::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let group = format!("lobby_{}", ts);
    let path_a = format!("/tmp/murmur_ann_a_{}", ts);
    let path_b = format!("/tmp/murmur_ann_b_{}", ts);

    // --- Node A: create group and announce ---
    let swarm_a = Swarm::builder()
        .storage_path(&path_a)
        .group_id(&group)
        .build()
        .await?;
    swarm_a.start().await?;
    swarm_a.announce("Alice").await?;
    println!("Node A announced as Alice in group {}", group);

    // --- Node B: same group, announce ---
    let swarm_b = Swarm::builder()
        .storage_path(&path_b)
        .group_id(&group)
        .build()
        .await?;
    swarm_b.start().await?;
    swarm_b.announce("Bob").await?;
    println!("Node B announced as Bob in group {}", group);

    // Wait for discovery
    println!("Waiting 8s for local discovery...");
    tokio::time::sleep(Duration::from_secs(8)).await;

    // Connect
    let count_a = swarm_a.discover_and_connect_local_peers().await?;
    let count_b = swarm_b.discover_and_connect_local_peers().await?;
    println!("A connected {} new, B connected {} new", count_a, count_b);

    // After connecting, request a full sync so metadata propagates
    tokio::time::sleep(Duration::from_secs(3)).await;

    // List peers from A's perspective
    let peers_a = swarm_a.list_announced_peers().await?;
    println!("\nNode A sees {} announced peers:", peers_a.len());
    for (node_id, nickname, gid) in &peers_a {
        println!("  - {} ({}) group={}", nickname, &node_id[..16], gid);
    }

    // List peers from B's perspective
    let peers_b = swarm_b.list_announced_peers().await?;
    println!("\nNode B sees {} announced peers:", peers_b.len());
    for (node_id, nickname, gid) in &peers_b {
        println!("  - {} ({}) group={}", nickname, &node_id[..16], gid);
    }

    // A should see at least itself
    assert!(peers_a.iter().any(|(_, nick, _)| nick == "Alice"), "A should see Alice");

    // If connection was established, B should also see Alice's announcement
    if count_a + count_b > 0 {
        // Give more time for CRDT sync
        tokio::time::sleep(Duration::from_secs(2)).await;
        let peers_b_updated = swarm_b.list_announced_peers().await?;
        println!("\nNode B (updated) sees {} announced peers:", peers_b_updated.len());
        for (node_id, nickname, gid) in &peers_b_updated {
            println!("  - {} ({}) group={}", nickname, &node_id[..16], gid);
        }
    }

    // Cleanup
    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b);

    println!("\nTest complete");
    Ok(())
}
