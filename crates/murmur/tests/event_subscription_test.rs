//! Event subscription integration test
//!
//! Verifies that SwarmEvent::PeerConnected is emitted promptly on BOTH sides
//! when two nodes establish a connection:
//!   - The active side (caller of connect_peer)
//!   - The passive side (accepting the incoming connection)

use murmur::{Swarm, SwarmEvent, Result};
use std::time::Duration;
use tokio::time::timeout;

const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

fn unique_paths(label: &str) -> (String, String, String) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let group = format!("evt_test_{}_{}", label, ts);
    let path_a = format!("/tmp/murmur_evt_{}_a_{}", label, ts);
    let path_b = format!("/tmp/murmur_evt_{}_b_{}", label, ts);
    (group, path_a, path_b)
}

async fn build_and_start(path: &str, group: &str) -> Result<Swarm> {
    let swarm = Swarm::builder()
        .storage_path(path)
        .group_id(group)
        .build()
        .await?;
    swarm.start().await?;
    Ok(swarm)
}

/// Wait for a PeerConnected event for a specific peer, return elapsed time.
async fn wait_for_peer_connected(
    rx: &mut tokio::sync::broadcast::Receiver<SwarmEvent>,
    expected_peer_id: &str,
    deadline: Duration,
) -> std::result::Result<Duration, String> {
    let start = std::time::Instant::now();
    loop {
        match timeout(deadline.saturating_sub(start.elapsed()), rx.recv()).await {
            Ok(Ok(SwarmEvent::PeerConnected { node_id })) => {
                if node_id == expected_peer_id {
                    return Ok(start.elapsed());
                }
            }
            Ok(Ok(_other)) => {
                // ignore non-PeerConnected events, keep waiting
                continue;
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                eprintln!("  [warn] receiver lagged by {} messages", n);
                continue;
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                return Err("event channel closed".into());
            }
            Err(_) => {
                return Err(format!(
                    "timed out after {:?} waiting for PeerConnected({})",
                    deadline, expected_peer_id
                ));
            }
        }
    }
}

/// Core test: connect_peer() should fire PeerConnected on both sides.
#[tokio::test]
async fn test_connect_peer_fires_event_both_sides() -> Result<()> {
    init_tracing();
    let (group, path_a, path_b) = unique_paths("both");

    let swarm_a = build_and_start(&path_a, &group).await?;
    let swarm_b = build_and_start(&path_b, &group).await?;

    let id_a = swarm_a.node_id().await;
    let id_b = swarm_b.node_id().await;
    println!("Node A: {}", id_a);
    println!("Node B: {}", id_b);

    // Subscribe BEFORE connecting so we don't miss the event
    let mut rx_a = swarm_a.subscribe();
    let mut rx_b = swarm_b.subscribe();

    // B connects to A
    let addr_a = swarm_a.node_addr().await?;
    println!("\n--- B connecting to A via connect_peer ---");
    swarm_b.connect_peer(&addr_a).await?;
    println!("connect_peer() returned OK");

    // Check passive side (A) — should receive PeerConnected
    println!("\n--- Checking passive side (A) for PeerConnected event ---");
    match wait_for_peer_connected(&mut rx_a, &id_b, EVENT_TIMEOUT).await {
        Ok(elapsed) => {
            println!("  PASS: A received PeerConnected({}) in {:?}", id_b, elapsed);
        }
        Err(e) => {
            println!("  FAIL: A did NOT receive PeerConnected for B: {}", e);
        }
    }

    // Check active side (B) — should also receive PeerConnected
    println!("\n--- Checking active side (B) for PeerConnected event ---");
    let elapsed_b = wait_for_peer_connected(&mut rx_b, &id_a, EVENT_TIMEOUT).await
        .expect("active side (B) should receive PeerConnected");
    println!("  PASS: B received PeerConnected({}) in {:?}", id_a, elapsed_b);

    // Summarize connectivity state
    let peers_a = swarm_a.connected_peers().await;
    let peers_b = swarm_b.connected_peers().await;
    println!("\n--- Connection state ---");
    println!("  A peers: {:?}", peers_a);
    println!("  B peers: {:?}", peers_b);

    // Cleanup
    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b);

    Ok(())
}

/// Test that the passive side (being connected to) fires PeerConnected promptly.
#[tokio::test]
async fn test_passive_side_event_timing() -> Result<()> {
    init_tracing();
    let (group, path_a, path_b) = unique_paths("passive");

    let swarm_a = build_and_start(&path_a, &group).await?;
    let swarm_b = build_and_start(&path_b, &group).await?;

    let id_b = swarm_b.node_id().await;

    let mut rx_a = swarm_a.subscribe();

    let addr_a = swarm_a.node_addr().await?;
    let connect_start = std::time::Instant::now();
    swarm_b.connect_peer(&addr_a).await?;
    let connect_elapsed = connect_start.elapsed();
    println!("connect_peer() took {:?}", connect_elapsed);

    match wait_for_peer_connected(&mut rx_a, &id_b, EVENT_TIMEOUT).await {
        Ok(elapsed) => {
            println!("Passive side event arrived in {:?} (total from connect start)", elapsed);
            assert!(
                elapsed < Duration::from_secs(5),
                "PeerConnected on passive side should arrive within 5s, took {:?}",
                elapsed
            );
            println!("PASS: passive side received PeerConnected promptly");
        }
        Err(e) => {
            panic!("Passive side did not receive PeerConnected: {}", e);
        }
    }

    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b);

    Ok(())
}

/// Test that the active side (caller of connect_peer) fires PeerConnected.
/// This test is expected to FAIL with the current code, exposing the bug.
#[tokio::test]
async fn test_active_side_event_fires() -> Result<()> {
    init_tracing();
    let (group, path_a, path_b) = unique_paths("active");

    let swarm_a = build_and_start(&path_a, &group).await?;
    let swarm_b = build_and_start(&path_b, &group).await?;

    let id_a = swarm_a.node_id().await;

    let mut rx_b = swarm_b.subscribe();

    let addr_a = swarm_a.node_addr().await?;
    swarm_b.connect_peer(&addr_a).await?;

    // The active side (B) should receive PeerConnected for A.
    // With the current code, Network::connect() does NOT emit PeerEvent::Connected,
    // so this is expected to time out.
    let short_timeout = Duration::from_secs(5);
    println!("Waiting up to {:?} for active side PeerConnected event...", short_timeout);

    let elapsed = wait_for_peer_connected(&mut rx_b, &id_a, short_timeout).await
        .expect("active side should receive PeerConnected");
    println!("PASS: active side received PeerConnected({}) in {:?}", id_a, elapsed);

    let peers_b = swarm_b.connected_peers().await;
    println!("B peers (should contain A): {:?}", peers_b);
    assert!(peers_b.contains(&id_a));

    swarm_a.shutdown().await?;
    swarm_b.shutdown().await?;
    let _ = std::fs::remove_dir_all(&path_a);
    let _ = std::fs::remove_dir_all(&path_b);

    Ok(())
}
