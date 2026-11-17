# Murmur Architecture

## Overview

Murmur is a distributed P2P collaboration library built on iroh-net with automatic leader election and CRDT synchronization. It provides a simple key-value store API with automatic conflict resolution and peer-to-peer networking.

## Core Components

### 1. Network Layer (`network.rs`)

**Responsibilities:**
- P2P connection management using iroh-net
- Message serialization and routing
- Local network discovery via LocalSwarmDiscovery
- ALPN protocol negotiation for QUIC connections

**Key Features:**
- Automatic peer discovery on local networks
- Direct peer-to-peer connections with NAT traversal
- Relay fallback for unreachable peers
- Vector clock for causal ordering

**API:**
```rust
pub struct Network {
    endpoint: Endpoint,
    node_id: NodeId,
    peers: HashMap<NodeId, Connection>,
    server_config: Arc<ServerConfig>, // ALPN: "murmur"
}

impl Network {
    pub async fn new(group_id: String) -> Result<Self>;
    pub async fn connect(&self, peer_addr: NodeAddr) -> Result<()>;
    pub async fn discover_and_connect_peers(&self) -> Result<usize>;
    pub async fn send(&self, peer_id: &str, message: Message) -> Result<()>;
    pub async fn broadcast(&self, message: Message) -> Result<()>;
}
```

### 2. Election Layer (`election.rs`)

**Responsibilities:**
- Bully algorithm implementation for leader election
- Heartbeat mechanism for leader health monitoring
- Automatic re-election on leader failure

**Algorithm:**
- Each node has a unique ID (iroh NodeId)
- Higher ID = higher priority
- Leader sends heartbeats every 2 seconds
- Followers timeout after 5 seconds without heartbeat
- Election triggered on timeout or new node join

**States:**
- `Follower`: Normal state, receives heartbeats
- `Candidate`: Initiating election
- `Leader`: Sending heartbeats, coordinating

**API:**
```rust
pub struct Election {
    node_id: String,
    role: Role,
    leader_id: Option<String>,
    term: u64,
}

impl Election {
    pub async fn start_election(&mut self, network: &Network) -> Result<()>;
    pub async fn handle_message(&mut self, peer_id: &str, message: &Message, network: &Network) -> Result<()>;
    pub fn is_leader(&self) -> bool;
}
```

### 3. Sync Layer (`sync.rs`)

**Responsibilities:**
- CRDT-based state synchronization using Automerge
- Conflict-free concurrent updates
- Change tracking and propagation

**CRDT Properties:**
- **Commutative**: Order of operations doesn't matter
- **Associative**: Grouping of operations doesn't matter
- **Idempotent**: Applying same change multiple times = applying once

**API:**
```rust
pub struct Sync {
    doc: Automerge,
}

impl Sync {
    pub fn put(&mut self, key: &str, value: &[u8]) -> Result<Vec<u8>>;
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    pub fn apply_changes(&mut self, changes: &[u8]) -> Result<()>;
    pub fn get_all_changes(&mut self) -> Result<Vec<u8>>;
}
```

### 4. Storage Layer (`storage.rs`)

**Responsibilities:**
- Local persistence of key-value data
- Multiple backend support (redb, SQLite, RocksDB)
- Atomic operations

**Backends:**
- **redb** (default): Pure Rust, embedded, ACID
- **SQLite**: Widely used, SQL interface
- **RocksDB**: High performance, LSM tree

**API:**
```rust
pub struct Storage {
    backend: Box<dyn StorageBackend>,
}

impl Storage {
    pub fn new(path: &Path) -> Result<Self>;
    pub fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    pub fn delete(&self, key: &str) -> Result<()>;
}
```

### 5. Swarm API (`lib.rs`)

**Responsibilities:**
- High-level API for applications
- Coordination of all layers
- Background task management

**API:**
```rust
pub struct Swarm {
    storage: Storage,
    network: Network,
    election: Election,
    sync: Sync,
}

impl Swarm {
    pub fn builder() -> SwarmBuilder;
    pub async fn start(&self) -> Result<()>;
    pub async fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    pub async fn discover_and_connect_local_peers(&self) -> Result<usize>;
    pub async fn is_leader(&self) -> bool;
    pub async fn connected_peers(&self) -> Vec<String>;
}
```

## Data Flow

### Write Operation

```
Application
    ↓ swarm.put("key", value)
Swarm
    ↓ sync.put() → CRDT changes
Sync (Automerge)
    ↓ storage.put()
Storage (Local DB)
    ↓ network.broadcast(CrdtUpdate)
Network
    ↓ Send to all peers
Peers
    ↓ Receive CrdtUpdate
    ↓ sync.apply_changes()
    ↓ storage.put()
```

### Read Operation

```
Application
    ↓ swarm.get("key")
Swarm
    ↓ storage.get()
Storage (Local DB)
    ↓ Return value
    ↓ (If not found, check CRDT)
Sync (Automerge)
    ↓ Return value
```

### Discovery and Connection

```
Node A starts
    ↓ Network::new() creates LocalSwarmDiscovery
    ↓ Broadcasts presence on local network
Node B receives broadcast
    ↓ Adds Node A to NodeMap
Application calls discover_and_connect_local_peers()
    ↓ network.discover_and_connect_peers()
    ↓ Iterates remote_info_iter()
    ↓ Connects to each discovered peer
Connection established
    ↓ ALPN negotiation ("murmur")
    ↓ Peer added to peers HashMap
    ↓ Election triggered
    ↓ Leader elected (highest NodeId)
```

## Message Types

```rust
pub enum Message {
    // Election messages
    Election { candidate_id: String },
    ElectionOk,
    Coordinator { leader_id: String, term: u64 },
    Heartbeat { leader_id: String, term: u64 },

    // Sync messages
    CrdtUpdate {
        key: String,
        operation: Vec<u8>,
        seq_num: u64,
        vector_clock: VectorClock,
    },
    SyncRequest,
    SyncResponse { data: Vec<u8> },
    Ack { seq_num: u64 },
}
```

## Concurrency Model

### Async Runtime
- Uses Tokio for async I/O
- All public APIs are async
- Background tasks for heartbeats and message handling

### Locking Strategy
- `RwLock` for shared state (allows multiple readers)
- Short critical sections to minimize contention
- Lock ordering to prevent deadlocks:
  1. Network
  2. Election
  3. Sync
  4. Storage

### Background Tasks

**Heartbeat Task:**
```rust
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if election.is_leader() {
            network.broadcast(Heartbeat { ... }).await;
        }
    }
});
```

**Message Handler Task:**
```rust
tokio::spawn(async move {
    loop {
        let (peer_id, message) = network.recv().await;
        swarm.handle_message(peer_id, message).await;
    }
});
```

## Error Handling

### Error Types
```rust
pub enum Error {
    Network(String),
    Storage(String),
    Serialization(String),
    Sync(String),
    Election(String),
}
```

### Error Propagation
- Use `?` operator for error propagation
- No `unwrap()` or `panic!()` in library code
- `expect()` only for logically impossible failures

### Retry Strategy
- Network errors: Automatic retry via iroh relay
- Storage errors: Fail fast, let application handle
- Sync conflicts: Automatic resolution via CRDT

## Performance Characteristics

### Latency
- Local network: 1-5ms (direct connection)
- Internet: 50-200ms (via relay)
- Write operation: O(1) local + network latency
- Read operation: O(1) local (no network)

### Throughput
- Limited by network bandwidth
- CRDT overhead: ~2x message size
- Batching possible for bulk operations

### Memory Usage
- Per-peer connection: ~100KB
- CRDT document: Grows with history
- Storage: Depends on backend

### Scalability
- Tested: 10-50 nodes
- Theoretical: 100-200 nodes (local network)
- Bottleneck: Broadcast messages (O(n) per write)

## Security Considerations

### Encryption
- All connections use TLS 1.3 via QUIC
- Peer authentication via public key cryptography
- No plaintext data on wire

### Authentication
- Each node has unique keypair (Ed25519)
- NodeId = public key
- Connections verified via TLS certificates

### Authorization
- No built-in authorization (application layer)
- Group isolation via group_id (honor system)
- Future: Cryptographic group membership

## Testing Strategy

### Unit Tests
- Each module has `#[cfg(test)]` tests
- Mock network for election tests
- In-memory storage for sync tests

### Integration Tests
- `examples/` directory contains integration tests
- `manual_test.rs`: Interactive testing
- `basic.rs`: Automated smoke test

### Network Tests
- Local network: Two terminals on same machine
- Cross-machine: Different computers on LAN
- Internet: Via relay servers

## Dependencies

### Core
- `iroh-net`: P2P networking, QUIC, NAT traversal
- `automerge`: CRDT implementation
- `tokio`: Async runtime

### Storage
- `redb`: Default embedded database
- `rusqlite`: Optional SQLite backend
- `rocksdb`: Optional RocksDB backend

### Utilities
- `serde`: Serialization
- `bincode`: Binary serialization
- `thiserror`: Error handling
- `tracing`: Logging

## Future Architecture

### Planned Improvements

1. **Streaming Sync**
   - Currently: Batch updates
   - Future: Stream changes as they happen

2. **Partial Replication**
   - Currently: Full document replication
   - Future: Subscribe to specific keys

3. **Conflict Callbacks**
   - Currently: Automatic CRDT resolution
   - Future: Application-defined conflict handlers

4. **Metrics and Monitoring**
   - Prometheus metrics
   - Health check endpoints
   - Performance profiling

5. **Security Enhancements**
   - Group membership verification
   - Access control lists
   - Audit logging
