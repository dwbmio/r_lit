use murmur::{Swarm, SwarmEvent, FileOps};
use std::io::{self, BufRead, Write};
use std::path::Path;

/// Interactive CLI for testing murmur-scope extension.
///
/// group_id is hardcoded to "murmur-scope" to match the bridge.
///
/// Usage:
///   cargo run --example cli_dev_scope -- <name>
///   cargo run --example cli_dev_scope -- alice
///
/// Commands:
///   put <key> <value>   — write a key-value pair
///   get <key>           — read a key
///   peers               — list connected peers
///   keys                — list all local keys (via get attempts)
///   <anything else>     — shorthand for `put msg:<name> <text>`

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("murmur=info")
        .with_writer(io::stderr)
        .init();

    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dev".into());

    let pid = std::process::id();
    let storage_path = format!("/tmp/murmur-cli-test-{}-{}", name, pid);

    eprintln!("╔══════════════════════════════════════╗");
    eprintln!("║  cli-dev-scope · {}  ", name);
    eprintln!("║  group: murmur-scope                 ║");
    eprintln!("║  storage: {}  ", storage_path);
    eprintln!("╚══════════════════════════════════════╝");

    let swarm = Swarm::builder()
        .storage_path(&storage_path)
        .group_id("murmur-scope")
        .build()
        .await?;

    swarm.start().await?;

    let node_id = swarm.node_id().await;
    eprintln!("Node ID: {}", node_id);
    match swarm.node_addr().await {
        Ok(addr) => eprintln!("Address: {}", serde_json::to_string(&addr).unwrap_or_default()),
        Err(e) => eprintln!("Address: (err: {})", e),
    }

    // Announce nickname so other nodes can display it
    if let Err(e) = swarm.announce(&name).await {
        eprintln!("Warning: failed to announce nickname: {}", e);
    } else {
        eprintln!("Announced as: {}", name);
    }
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  put <key> <value>     write KV");
    eprintln!("  get <key>             read KV");
    eprintln!("  peers                 list peers");
    eprintln!("  upload <path>         upload file to swarm");
    eprintln!("  download <key> <out>  download file");
    eprintln!("  files                 list shared files");
    eprintln!("  history <key>         file version history");
    eprintln!("  audit                 recent file operations");
    eprintln!("  <text>                shorthand for put msg:<name> <text>");
    eprintln!("  quit                  exit");
    eprintln!();

    // Event listener task
    let mut events = swarm.subscribe();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(SwarmEvent::PeerConnected { node_id }) => {
                    eprintln!("\r  [event] + peer connected: {}…", &node_id[..8.min(node_id.len())]);
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Ok(SwarmEvent::PeerDisconnected { node_id }) => {
                    eprintln!("\r  [event] - peer disconnected: {}…", &node_id[..8.min(node_id.len())]);
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Ok(SwarmEvent::DataSynced { key, value }) => {
                    // Decompress LZ4 if magic byte present, else treat as plain text
                    let val_str = if !value.is_empty() && value[0] == 0x04 {
                        lz4_flex::decompress_size_prepended(&value[1..])
                            .ok()
                            .and_then(|v| String::from_utf8(v).ok())
                            .unwrap_or_else(|| String::from_utf8_lossy(&value).into_owned())
                    } else {
                        String::from_utf8_lossy(&value).into_owned()
                    };
                    let display = if val_str.len() > 80 {
                        format!("{}…", &val_str[..80])
                    } else {
                        val_str.to_string()
                    };
                    eprintln!("\r  [event] synced: {} = {}", key, display);
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Ok(SwarmEvent::ConflictDetected { file_name, .. }) => {
                    eprintln!("\r  [event] conflict detected: {}", file_name);
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Ok(SwarmEvent::ConflictResolved { file_name, new_version, .. }) => {
                    eprintln!("\r  [event] conflict resolved: {} → v{}", file_name, new_version);
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Ok(SwarmEvent::SyncStarted) => {
                    eprintln!("\r  [event] sync started…");
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Ok(SwarmEvent::SyncCompleted { hash, key_count }) => {
                    eprintln!("\r  [event] sync completed: hash={} keys={}", hash, key_count);
                    eprint!("> ");
                    let _ = io::stderr().flush();
                }
                Err(_) => break,
            }
        }
    });

    // Wait for network to settle
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let peers = swarm.connected_peers().await;
    eprintln!("Connected peers: {}", peers.len());
    for p in &peers {
        eprintln!("  - {}…", &p[..16.min(p.len())]);
    }
    eprintln!();

    // Stdin REPL
    let name_clone = name.clone();
    let swarm_clone = swarm.clone();
    let repl = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let stdin = io::stdin();
        let reader = stdin.lock();
        eprint!("> ");
        let _ = io::stderr().flush();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim().to_string();
            if line.is_empty() {
                eprint!("> ");
                let _ = io::stderr().flush();
                continue;
            }

            if line == "quit" || line == "exit" {
                break;
            }

            let rt = tokio::runtime::Handle::current();

            if line == "peers" {
                let peers = rt.block_on(swarm_clone.connected_peers());
                eprintln!("  peers ({}): {:?}", peers.len(),
                    peers.iter().map(|p| &p[..8.min(p.len())]).collect::<Vec<_>>());
            } else if let Some(key) = line.strip_prefix("get ") {
                let key = key.trim();
                match rt.block_on(swarm_clone.get(key)) {
                    Ok(Some(v)) => eprintln!("  {} = {}", key, String::from_utf8_lossy(&v)),
                    Ok(None) => eprintln!("  {} = (not found)", key),
                    Err(e) => eprintln!("  error: {}", e),
                }
            } else if let Some(rest) = line.strip_prefix("put ") {
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if parts.len() < 2 {
                    eprintln!("  usage: put <key> <value>");
                } else {
                    let key = parts[0].trim();
                    let val = parts[1].trim();
                    match rt.block_on(swarm_clone.put(key, val.as_bytes())) {
                        Ok(()) => eprintln!("  ok: {} = {}", key, val),
                        Err(e) => eprintln!("  error: {}", e),
                    }
                }
            } else if let Some(file_path) = line.strip_prefix("upload ") {
                let file_path = file_path.trim();
                let p = Path::new(file_path);
                if !p.exists() {
                    eprintln!("  file not found: {}", file_path);
                } else {
                    match rt.block_on(swarm_clone.put_file(p)) {
                        Ok(key) => {
                            eprintln!("  uploaded: {}", key);
                            if let Ok(Some(meta)) = rt.block_on(swarm_clone.file_metadata(&key)) {
                                eprintln!("    name: {}, v{}, {} bytes", meta.name, meta.version, meta.size);
                            }
                        }
                        Err(e) => eprintln!("  error: {}", e),
                    }
                }
            } else if let Some(rest) = line.strip_prefix("download ") {
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if parts.len() < 2 {
                    eprintln!("  usage: download <key> <output_path>");
                } else {
                    let key = parts[0].trim();
                    let out = Path::new(parts[1].trim());
                    match rt.block_on(swarm_clone.get_file(key, out)) {
                        Ok(_) => eprintln!("  downloaded: {}", out.display()),
                        Err(e) => eprintln!("  error: {}", e),
                    }
                }
            } else if line == "files" {
                match rt.block_on(swarm_clone.list_files()) {
                    Ok(files) if files.is_empty() => eprintln!("  (no files)"),
                    Ok(files) => {
                        for f in &files {
                            eprintln!("  {} (v{}, {} bytes)", f.name, f.version, f.size);
                        }
                    }
                    Err(e) => eprintln!("  error: {}", e),
                }
            } else if let Some(key) = line.strip_prefix("history ") {
                match rt.block_on(swarm_clone.file_history(key.trim())) {
                    Ok(h) if h.is_empty() => eprintln!("  (no history)"),
                    Ok(h) => {
                        for entry in h.iter().rev() {
                            eprintln!("  v{} {:?} {} bytes by {} at {}",
                                entry.version, entry.operation, entry.size,
                                entry.author, entry.timestamp);
                        }
                    }
                    Err(e) => eprintln!("  error: {}", e),
                }
            } else if line == "audit" {
                match rt.block_on(swarm_clone.audit_trail(Some(20))) {
                    Ok(trail) if trail.is_empty() => eprintln!("  (no audit trail)"),
                    Ok(trail) => {
                        for entry in trail.iter().rev().take(20) {
                            eprintln!("  {:?} {} by {} at {}",
                                entry.operation, entry.content_key,
                                entry.author, entry.timestamp);
                        }
                    }
                    Err(e) => eprintln!("  error: {}", e),
                }
            } else {
                // Shorthand: treat as message with seq key + lz4 compression
                let ts = chrono::Utc::now().timestamp_millis();
                let key = format!("msg:{}:{}", name_clone, ts);
                let compressed = lz4_flex::compress_prepend_size(line.as_bytes());
                let mut data = Vec::with_capacity(1 + compressed.len());
                data.push(0x04u8); // LZ4 magic byte
                data.extend_from_slice(&compressed);
                match rt.block_on(swarm_clone.put(&key, &data)) {
                    Ok(()) => eprintln!("  sent: {} = {}", key, line),
                    Err(e) => eprintln!("  error: {}", e),
                }
            }

            eprint!("> ");
            let _ = io::stderr().flush();
        }

        Ok(())
    });

    repl.await??;

    eprintln!("\nShutting down…");
    swarm.shutdown().await?;
    Ok(())
}
