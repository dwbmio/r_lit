//! Multi-node list status test
//!
//! Simulates the workbench scenario with multiple groups and members:
//! - Group "alpha": Alice + Bob
//! - Group "beta": Charlie
//! - Searcher node discovers all groups and verifies member lists

use murmur::Swarm;
use std::collections::HashMap;
use std::time::Duration;

struct TestNode {
    swarm: Swarm,
    path: String,
}

impl TestNode {
    async fn new(name: &str, group: &str, ts: u128) -> murmur::Result<Self> {
        let path = format!("/tmp/murmur_multi_{}_{}", name, ts);
        let swarm = Swarm::builder()
            .storage_path(&path)
            .group_id(group)
            .build()
            .await?;
        swarm.start().await?;
        swarm.announce(name).await?;
        println!("[{}] started in group '{}', id={}", name, group, &swarm.node_id().await[..16]);
        Ok(Self { swarm, path })
    }

    async fn cleanup(self) {
        let _ = self.swarm.shutdown().await;
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn test_multi_node_group_list() -> murmur::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let group_alpha = format!("alpha_{}", ts);
    let group_beta = format!("beta_{}", ts);

    // === Create 3 group nodes ===
    println!("=== Creating group nodes ===\n");

    let alice = TestNode::new("Alice", &group_alpha, ts).await?;
    let bob = TestNode::new("Bob", &group_alpha, ts).await?;
    let charlie = TestNode::new("Charlie", &group_beta, ts).await?;

    // Let Alice and Bob discover each other within group alpha
    println!("\n=== Waiting 8s for peer discovery ===");
    tokio::time::sleep(Duration::from_secs(8)).await;

    let ac = alice.swarm.discover_and_connect_local_peers().await?;
    let bc = bob.swarm.discover_and_connect_local_peers().await?;
    let cc = charlie.swarm.discover_and_connect_local_peers().await?;
    println!("Alice connected {} new, Bob {} new, Charlie {} new", ac, bc, cc);

    // Allow CRDT sync between Alice and Bob (same group members)
    tokio::time::sleep(Duration::from_secs(3)).await;

    // === Searcher node ===
    println!("\n=== Starting searcher node ===");
    let search_path = format!("/tmp/murmur_multi_search_{}", ts);
    let searcher = Swarm::builder()
        .storage_path(&search_path)
        .group_id("_search_")
        .build()
        .await?;
    searcher.start().await?;
    println!("[Searcher] id={}", &searcher.node_id().await[..16]);

    println!("\n=== Waiting 8s for searcher discovery ===");
    tokio::time::sleep(Duration::from_secs(8)).await;

    let sc = searcher.discover_and_connect_local_peers().await?;
    println!("[Searcher] connected to {} peer(s)", sc);

    // CRDT sync time
    tokio::time::sleep(Duration::from_secs(4)).await;

    // === Read and verify results ===
    let peers = searcher.list_announced_peers().await?;
    println!("\n=== Searcher sees {} announced peer(s) ===", peers.len());
    for (nid, nick, gid) in &peers {
        println!("  node={}... nick={:<10} group={}", &nid[..12], nick, gid);
    }

    // Group by group_id (skip internal groups)
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for (_, nickname, group_id) in &peers {
        if !group_id.starts_with('_') {
            groups.entry(group_id.clone())
                .or_default()
                .push(nickname.clone());
        }
    }

    println!("\n=== Discovered group list ===");
    let mut group_ids: Vec<&String> = groups.keys().collect();
    group_ids.sort();
    for gid in &group_ids {
        let members = &groups[*gid];
        println!("  [{}] {} member(s): {:?}", gid, members.len(), members);
    }

    // --- Assertions ---
    println!("\n=== Verifying ===");

    // 1) Both groups should be found
    assert!(
        groups.contains_key(&group_alpha),
        "Should find group alpha ({}), found: {:?}",
        group_alpha, groups.keys().collect::<Vec<_>>()
    );
    assert!(
        groups.contains_key(&group_beta),
        "Should find group beta ({}), found: {:?}",
        group_beta, groups.keys().collect::<Vec<_>>()
    );
    println!("  [OK] Both groups discovered");

    // 2) Group alpha should have Alice and Bob
    let alpha_members = &groups[&group_alpha];
    assert!(alpha_members.contains(&"Alice".to_string()), "Alpha missing Alice");
    assert!(alpha_members.contains(&"Bob".to_string()), "Alpha missing Bob");
    assert_eq!(alpha_members.len(), 2, "Alpha should have exactly 2 members");
    println!("  [OK] Group alpha has Alice + Bob");

    // 3) Group beta should have Charlie
    let beta_members = &groups[&group_beta];
    assert!(beta_members.contains(&"Charlie".to_string()), "Beta missing Charlie");
    assert_eq!(beta_members.len(), 1, "Beta should have exactly 1 member");
    println!("  [OK] Group beta has Charlie");

    // 4) Our two groups should have exactly 3 members total
    // (other groups from stale mDNS peers may exist, so only count ours)
    let our_total = alpha_members.len() + beta_members.len();
    assert_eq!(our_total, 3, "Our groups should have 3 members total");
    println!("  [OK] Total 3 members across our 2 groups");

    // === Cleanup ===
    alice.cleanup().await;
    bob.cleanup().await;
    charlie.cleanup().await;
    let _ = searcher.shutdown().await;
    let _ = std::fs::remove_dir_all(&search_path);

    println!("\n=== Multi-node list test PASSED ===");
    Ok(())
}
