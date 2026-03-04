//! Simulates the exact workbench GUI flow end-to-end:
//!
//! 1. "Client A" creates a group, announces, stays alive
//! 2. "Client B" runs the search flow (same logic as start_search)
//! 3. Verify B sees A's group in the discovered list
//! 4. B "joins" the group (shutdown search swarm, create new one)
//! 5. Verify both clients are connected and can exchange data

use murmur::Swarm;
use std::collections::HashMap;
use std::time::Duration;

#[tokio::test]
async fn test_full_gui_flow_simulation() -> murmur::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let group_name = format!("project_{}", ts);
    let path_a = format!("/tmp/murmur_gui_a_{}", ts);
    let path_b_search = format!("/tmp/murmur_gui_b_search_{}", ts);
    let path_b_join = format!("/tmp/murmur_gui_b_join_{}", ts);

    // =============================================
    // Step 1: Client A — "创建新群组" button clicked
    // =============================================
    println!("========================================");
    println!("STEP 1: Client A creates group '{}'", group_name);
    println!("========================================\n");

    let swarm_a = Swarm::builder()
        .storage_path(&path_a)
        .group_id(&group_name)
        .build()
        .await?;
    swarm_a.start().await?;
    swarm_a.announce("Alice").await?;

    println!("[A] Group created, announced as 'Alice'");
    println!("[A] Node ID: {}", &swarm_a.node_id().await[..20]);
    println!("[A] SwarmState -> Connected (waiting for peers)\n");

    // =============================================
    // Step 2: Client B — "搜索本地网络上的群组" clicked
    // =============================================
    println!("========================================");
    println!("STEP 2: Client B starts group search");
    println!("========================================\n");

    // This is the exact logic from start_search()
    println!("[B] SwarmState -> Connecting");
    println!("[B] Creating search swarm with group_id='_search_'...");

    let swarm_b_search = Swarm::builder()
        .storage_path(&path_b_search)
        .group_id("_search_")
        .build()
        .await?;
    swarm_b_search.start().await?;

    println!("[B] Search swarm started, waiting 8s for discovery...");
    tokio::time::sleep(Duration::from_secs(8)).await;

    let count = swarm_b_search.discover_and_connect_local_peers().await?;
    println!("[B] Connected to {} peer(s)", count);

    println!("[B] Waiting 3s for CRDT sync...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let peers = swarm_b_search.list_announced_peers().await?;
    println!("[B] Found {} announced peer(s)", peers.len());

    // Group by group_id, filter internal
    let mut groups: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (node_id, nickname, group_id) in &peers {
        if !group_id.starts_with('_') {
            groups.entry(group_id.clone())
                .or_default()
                .push((node_id.clone(), nickname.clone()));
        }
    }

    // Verify search results (what render_searching would show)
    println!("\n--- Search Results (UI would display): ---");
    if groups.is_empty() {
        println!("  (empty - error message would show)");
    } else {
        for (gid, members) in &groups {
            let names: Vec<&str> = members.iter().map(|(_, n)| n.as_str()).collect();
            println!("  群组: {}  |  {} 人  |  {}", gid, members.len(), names.join(", "));
        }
    }
    println!();

    assert!(!groups.is_empty(), "Should find at least one group");
    assert!(groups.contains_key(&group_name), "Should find the target group");

    let target_members = &groups[&group_name];
    assert!(target_members.iter().any(|(_, n)| n == "Alice"), "Should see Alice");
    println!("[B] VERIFIED: Found group '{}' with member 'Alice'\n", group_name);

    // =============================================
    // Step 3: Client B — clicks "加入" on the group
    // =============================================
    println!("========================================");
    println!("STEP 3: Client B joins group '{}'", group_name);
    println!("========================================\n");

    // Shutdown search swarm first (like try_shutdown_sync does)
    println!("[B] Shutting down search swarm...");
    swarm_b_search.shutdown().await?;
    println!("[B] Search swarm shutdown complete");

    // Create new swarm with the real group_id (like join_group does)
    println!("[B] Creating group swarm with group_id='{}'...", group_name);
    let swarm_b_join = Swarm::builder()
        .storage_path(&path_b_join)
        .group_id(&group_name)
        .build()
        .await?;
    swarm_b_join.start().await?;
    swarm_b_join.announce("Bob").await?;
    println!("[B] Announced as 'Bob' in group '{}'", group_name);
    println!("[B] SwarmState -> Connecting");

    println!("[B] Waiting 8s for peer discovery...");
    tokio::time::sleep(Duration::from_secs(8)).await;

    let join_count = swarm_b_join.discover_and_connect_local_peers().await?;
    println!("[B] Connected to {} peer(s)", join_count);

    if join_count > 0 {
        println!("[B] SwarmState -> Connected");
        println!("[B] Toast: '已连接 {} 个节点'", join_count);
    } else {
        println!("[B] SwarmState -> Connected (no peers yet)");
        println!("[B] Toast: '群组已创建，等待其他成员加入...'");
    }

    // =============================================
    // Step 4: Verify bidirectional data exchange
    // =============================================
    println!("\n========================================");
    println!("STEP 4: Verify data exchange");
    println!("========================================\n");

    // A also discovers B now
    let a_new = swarm_a.discover_and_connect_local_peers().await?;
    println!("[A] Discovered {} new peer(s)", a_new);

    tokio::time::sleep(Duration::from_secs(2)).await;

    // A writes data
    swarm_a.put("doc:readme", b"Hello from Alice!").await?;
    println!("[A] Put: doc:readme = 'Hello from Alice!'");

    tokio::time::sleep(Duration::from_secs(2)).await;

    // B reads it
    let val_b = swarm_b_join.get("doc:readme").await?;
    println!(
        "[B] Get: doc:readme = {:?}",
        val_b.as_ref().map(|v| String::from_utf8_lossy(v).to_string())
    );

    // B writes data
    swarm_b_join.put("doc:notes", b"Hello from Bob!").await?;
    println!("[B] Put: doc:notes = 'Hello from Bob!'");

    tokio::time::sleep(Duration::from_secs(2)).await;

    // A reads it
    let val_a = swarm_a.get("doc:notes").await?;
    println!(
        "[A] Get: doc:notes = {:?}",
        val_a.as_ref().map(|v| String::from_utf8_lossy(v).to_string())
    );

    // Verify member list from both sides
    println!("\n--- Final member list (from A's view): ---");
    let peers_a = swarm_a.list_announced_peers().await?;
    for (nid, nick, gid) in &peers_a {
        println!("  {}  |  {}  |  group={}", nick, &nid[..16], gid);
    }

    println!("\n--- Final member list (from B's view): ---");
    let peers_b = swarm_b_join.list_announced_peers().await?;
    for (nid, nick, gid) in &peers_b {
        println!("  {}  |  {}  |  group={}", nick, &nid[..16], gid);
    }

    // Assertions
    assert_eq!(
        val_b.as_deref(),
        Some(b"Hello from Alice!".as_ref()),
        "B should see A's data"
    );
    assert_eq!(
        val_a.as_deref(),
        Some(b"Hello from Bob!".as_ref()),
        "A should see B's data"
    );
    println!("\n[OK] Bidirectional data exchange verified");

    assert!(peers_a.len() >= 2, "A should see at least 2 announced peers");
    assert!(peers_b.len() >= 2, "B should see at least 2 announced peers");
    println!("[OK] Both nodes see each other in member list");

    // =============================================
    // Cleanup
    // =============================================
    swarm_a.shutdown().await?;
    swarm_b_join.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b_search);
    let _ = std::fs::remove_dir_all(&path_b_join);

    println!("\n========================================");
    println!("ALL GUI FLOW SIMULATION TESTS PASSED");
    println!("========================================");
    Ok(())
}
