# Murmur

> Distributed P2P collaboration library. Zero config, zero servers, zero drama.

## What's This?

A Rust library that syncs data across devices without needing a central server. Your laptop and desktop can share data just by being on the same WiFi 🚀

Think of it as Git meets CRDT meets P2P networking - but simpler.

## Why Use This?

- **No servers needed** - Pure P2P, runs on your local network
- **Zero configuration** - mDNS discovery means it just works™
- **CRDT sync** - Automerge-based conflict-free replication
- **Conflict detection & resolution** - Automatic locking + event-driven resolution workflow
- **Built-in versioning** - Time travel through your data (optional)
- **Audit trail** - Full per-file change history with author, timestamp and operation type
- **Rust native** - Fast, safe, and memory-efficient

Perfect for building collaborative apps, offline-first tools, or anything that needs to sync data without the cloud.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
murmur = { path = "../murmur" }
tokio = { version = "1", features = ["full"] }
```

Basic usage:

```rust
use murmur::Swarm;

#[tokio::main]
async fn main() -> Result<()> {
    // Create and start a swarm
    let swarm = Swarm::builder()
        .storage_path("./data")
        .build()
        .await?;

    swarm.start().await?;

    // Store some data
    swarm.put("user:alice", b"Alice").await?;

    // Retrieve it
    if let Some(value) = swarm.get("user:alice").await? {
        println!("Found: {}", String::from_utf8_lossy(&value));
    }

    Ok(())
}
```

That's it! Your data is now syncing across all nodes on the network.

## Core Features

### P2P Networking
Built on `iroh-net` with automatic NAT traversal and relay selection. Nodes discover each other via mDNS on the local network.

### CRDT Synchronization
Uses Automerge for conflict-free updates. Multiple nodes can modify data simultaneously without conflicts.

### Leader Election
Automatic coordinator selection using the Bully algorithm. Useful for coordinating distributed operations.

### Flexible Storage
Choose your backend: redb (default), SQLite, or RocksDB. All provide persistent local storage.

## File Operations (Optional)

Want to sync files with automatic versioning? Enable the `file-ops` feature:

```toml
[dependencies]
murmur = { path = "../murmur", features = ["file-ops"] }
```

Then you can:

```rust
use murmur::{Swarm, FileOps};
use std::path::Path;

// Upload a file (automatically versioned)
let key = swarm.put_file(Path::new("document.txt")).await?;

// Download it
swarm.get_file(&key, Path::new("output.txt")).await?;

// Time travel - get version 3
swarm.get_file_version(&key, 3, Path::new("old.txt")).await?;

// View the history
let history = swarm.file_history(&key).await?;
for entry in history {
    println!("v{} by {} at {}", entry.version, entry.author, entry.timestamp);
}
```

**What you get:**
- Automatic versioning (every write creates a new version)
- Full audit trail (who changed what, when) via `audit_trail()`
- Conflict detection with distributed file locking
- Event-driven resolution: `ConflictDetected` / `ConflictResolved` via `subscribe()`
- Size limits (10MB max by default)

### Conflict Resolution Flow

When two nodes write the same file concurrently:

1. CRDT detects concurrent writes → file is **locked** on all nodes
2. All nodes receive `SwarmEvent::ConflictDetected`
3. The designated resolver calls `resolve_conflict()` (KeepLocal / KeepRemote / MergeWith)
4. All nodes receive `SwarmEvent::ConflictResolved`, file unlocks, sync resumes

```rust
use murmur::{Swarm, SwarmEvent, ConflictResolution, FileOps};

let mut events = swarm.subscribe();
loop {
    match events.recv().await? {
        SwarmEvent::ConflictDetected { file_name, resolver_node, .. } => {
            if resolver_node == swarm.node_id().await {
                swarm.resolve_conflict(&file_name, ConflictResolution::KeepLocal).await?;
            }
        }
        SwarmEvent::ConflictResolved { file_name, new_version, .. } => {
            println!("{} resolved → v{}", file_name, new_version);
        }
        _ => {}
    }
}
```

## Architecture

```
┌─────────────────────────────────────┐
│  Your Application                   │
├─────────────────────────────────────┤
│  Murmur API (KV + File Ops)        │
├─────────────────────────────────────┤
│  CRDT Layer (Automerge)             │
├─────────────────────────────────────┤
│  P2P Network (iroh-net)             │
└─────────────────────────────────────┘
```

Simple layered design: your app talks to the API, which uses CRDT for sync and P2P for networking.

## Use Cases

- **Collaborative editing** - Multiple users editing documents in real-time
- **LAN gaming** - Sync game state across players on the same network
- **Offline-first apps** - Work locally, sync when connected
- **Config management** - Keep configs in sync across your machines
- **Team data sharing** - Share data within a team without cloud services

## Examples

Check out the `examples/` directory:

```bash
# Basic key-value store
cargo run --example basic

# Group chat application
cargo run --example group_chat

# File sync with versioning
cargo run --example file_sync --features file-ops

# Performance benchmark
cargo run --example benchmark --release
```

## Configuration

### Storage Backends

```toml
# Default: redb (fast, embedded)
murmur = { path = "../murmur" }

# SQLite (more mature, widely used)
murmur = { path = "../murmur", features = ["sqlite-backend"] }

# RocksDB (high performance, production-ready)
murmur = { path = "../murmur", features = ["rocksdb-backend"] }
```

### Optional Features

```toml
# Enable file operations with version control
murmur = { path = "../murmur", features = ["file-ops"] }
```

## Error Handling

All operations return `Result<T, Error>` for proper error handling:

```rust
use murmur::Error;

match swarm.put_file(path).await {
    Ok(key) => println!("Uploaded: {}", key),
    Err(Error::FileTooLarge { size, max }) => {
        eprintln!("File too large: {} bytes (max: {})", size, max);
    }
    Err(Error::VersionConflict { expected, current }) => {
        eprintln!("Version conflict: expected {}, got {}", expected, current);
    }
    Err(Error::FileConflictLocked { file_name }) => {
        eprintln!("File {} is locked due to unresolved conflict", file_name);
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

## Performance

Benchmarked on a modern laptop:

- **Throughput**: ~10,000 operations/second (single node)
- **Latency**: <10ms for local network operations
- **Memory**: ~10MB baseline + your data
- **Storage**: Depends on backend choice

Run your own benchmarks: `cargo run --example benchmark --release`

## Documentation

- [ROADMAP.md](ROADMAP.md) - Project roadmap and future plans
- [docs/](docs/) - Detailed technical documentation
- [examples/](examples/) - Working code examples

## Contributing

Contributions welcome! Whether it's bug reports, feature requests, or code contributions.

Keep it simple, keep it fast, keep it reliable.

## License

MIT OR Apache-2.0

---

Built with Rust 🦀
