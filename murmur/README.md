# Murmur

A distributed P2P collaboration library with automatic leader election and CRDT synchronization.

## Features

- **P2P Networking**: Built on `iroh` for NAT traversal and automatic relay selection
- **CRDT Sync**: Uses `automerge` for conflict-free state synchronization
- **Local Storage**: SQLite for persistent local replicas
- **Leader Election**: Bully algorithm for automatic coordinator selection
- **Zero Configuration**: Works out of the box with sensible defaults

## Architecture

```
┌─────────────────────────────────────┐
│  Application Layer                  │
│  ├─ Simple KV API                   │
│  └─ Leader/Follower Queries         │
├─────────────────────────────────────┤
│  Murmur Core                        │
│  ├─ Leader Election (Bully)         │
│  ├─ CRDT Sync (Automerge)           │
│  └─ Local Storage (SQLite)          │
├─────────────────────────────────────┤
│  iroh P2P Layer                     │
│  ├─ NAT Traversal                   │
│  ├─ Relay Selection                 │
│  └─ QUIC Transport                  │
└─────────────────────────────────────┘
```

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
murmur = { path = "../murmur" }
tokio = { version = "1", features = ["full"] }
```

### Basic Example

```rust
use murmur::Swarm;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a new swarm instance
    let swarm = Swarm::builder()
        .storage_path("./data")
        .build()
        .await?;

    // Start the swarm (begins P2P networking and election)
    swarm.start().await?;

    // Put a value
    swarm.put("user:123", b"Alice").await?;

    // Get a value
    if let Some(value) = swarm.get("user:123").await? {
        println!("Value: {}", String::from_utf8_lossy(&value));
    }

    // Check leadership
    if swarm.is_leader().await {
        println!("I am the leader!");
    } else if let Some(leader_id) = swarm.leader_id().await {
        println!("Leader is: {}", leader_id);
    }

    // Delete a value
    swarm.delete("user:123").await?;

    // Graceful shutdown
    swarm.shutdown().await?;

    Ok(())
}
```

## How It Works

### 1. P2P Networking

Murmur uses `iroh` for peer-to-peer communication:
- Automatic NAT traversal
- Relay server selection for nodes behind firewalls
- QUIC-based transport for reliability

### 2. Leader Election

Uses the Bully algorithm:
- Node with highest ID becomes leader
- Automatic re-election on leader failure
- Heartbeat mechanism (2s interval, 5s timeout)

### 3. CRDT Synchronization

Uses Automerge for conflict-free updates:
- All nodes maintain a full replica
- Changes are broadcast to all peers
- Automatic conflict resolution
- Eventual consistency guaranteed

### 4. Local Storage

SQLite provides persistent storage:
- Each node has a complete local copy
- Survives restarts
- Fast local reads

## API Reference

### `Swarm`

Main entry point for the library.

#### Methods

- `builder() -> SwarmBuilder` - Create a new builder
- `start() -> Result<()>` - Start the swarm
- `put(key, value) -> Result<()>` - Store a key-value pair
- `get(key) -> Result<Option<Vec<u8>>>` - Retrieve a value
- `delete(key) -> Result<()>` - Delete a key
- `is_leader() -> bool` - Check if this node is the leader
- `leader_id() -> Option<String>` - Get the current leader's ID
- `node_id() -> String` - Get this node's ID
- `shutdown() -> Result<()>` - Graceful shutdown

### `SwarmBuilder`

Builder for configuring a Swarm.

#### Methods

- `storage_path(path) -> Self` - Set storage directory (default: `./murmur_data`)
- `build() -> Result<Swarm>` - Build the swarm instance

## Building

```bash
# Build the library
cargo build --release

# Run tests
cargo test

# Build with musl for static linking
cargo build --release --target x86_64-unknown-linux-musl
```

## License

MIT OR Apache-2.0
