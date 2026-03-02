# CLAUDE.md

This file provides guidance to Claude Code when working with the Murmur codebase.

## Project Overview

Murmur is a distributed P2P collaboration library written in Rust. It enables applications to sync data across devices without central servers, using CRDT for conflict-free synchronization and iroh-net for P2P networking.

**Version**: 0.1.0
**Language**: Rust 2021 edition
**License**: MIT OR Apache-2.0

## Architecture

### Layered Design

```
┌─────────────────────────────────────┐
│  Application Layer                  │
│  ├─ KV API (put/get/delete)        │
│  └─ File Ops API (optional)        │
├─────────────────────────────────────┤
│  Murmur Core                        │
│  ├─ Version Control (file-ops)     │
│  ├─ Leader Election (Bully)        │
│  ├─ CRDT Sync (Automerge)          │
│  └─ Local Storage (pluggable)      │
├─────────────────────────────────────┤
│  P2P Network Layer                  │
│  ├─ iroh-net (QUIC transport)      │
│  ├─ mDNS Discovery                  │
│  └─ NAT Traversal                   │
└─────────────────────────────────────┘
```

### Core Components

**Network Layer** ([src/network.rs](src/network.rs))
- P2P networking using iroh-net
- Automatic local network discovery via mDNS
- QUIC-based transport with "murmur" ALPN
- Message broadcasting and peer management
- Vector clock for causality tracking

**CRDT Sync** ([src/sync.rs](src/sync.rs))
- Automerge-based conflict-free synchronization
- Automatic merge of concurrent updates
- Change propagation to all peers
- Eventual consistency guarantee

**Leader Election** ([src/election.rs](src/election.rs))
- Bully algorithm implementation
- Automatic re-election on leader failure
- Heartbeat mechanism (2s interval, 5s timeout)
- Node with highest ID becomes leader

**Storage** ([src/storage.rs](src/storage.rs), [src/storage_trait.rs](src/storage_trait.rs))
- Pluggable storage backend via trait
- Default: redb (fast, embedded)
- Optional: SQLite, RocksDB
- Local persistence for all data

**File Operations** ([src/file.rs](src/file.rs)) - Optional feature
- High-level file sync API
- Version control system
- Audit trail and history
- Conflict detection

## Version Control System (file-ops feature)

### Design Principles

1. **Every write creates a new version** - Automatic versioning, no manual intervention
2. **Complete history** - All versions stored for audit and rollback
3. **Conflict detection** - Optimistic locking prevents silent overwrites
4. **Audit trail** - Track who changed what, when

### Storage Schema

```
# File content
file:data:README.md          # Latest version (fast access)
file:data:README.md:v1       # Version 1
file:data:README.md:v2       # Version 2
file:data:README.md:v3       # Version 3

# Metadata
file:meta:README.md          # Current metadata (includes version number)

# History
file:history:README.md       # Array of FileVersion entries

# Audit log
audit:1709123456:node-abc    # Global audit record
audit:1709123457:node-xyz
```

### Data Structures

```rust
pub struct FileMetadata {
    pub name: String,        // File name
    pub size: usize,         // Size in bytes
    pub modified: u64,       // Unix timestamp
    pub checksum: String,    // Simple checksum (file size)
    pub version: u64,        // Current version number
    pub author: String,      // Node ID that created this version
}

pub struct FileVersion {
    pub version: u64,           // Version number
    pub content_key: String,    // Storage key for content
    pub timestamp: u64,         // Unix timestamp
    pub author: String,         // Node ID
    pub size: usize,            // File size
    pub operation: FileOperation, // Create/Update/Delete
}

pub enum FileOperation {
    Create,  // File created
    Update,  // File updated
    Delete,  // File deleted
}
```

### API

```rust
// Basic upload (auto-increment version)
let key = swarm.put_file(Path::new("doc.txt")).await?;

// Safe upload with version check (optimistic locking)
let meta = swarm.file_metadata(&key).await?.unwrap();
swarm.put_file_with_version(Path::new("doc.txt"), Some(meta.version)).await?;

// Download latest version
swarm.get_file(&key, Path::new("output.txt")).await?;

// Download specific version
swarm.get_file_version(&key, 2, Path::new("old.txt")).await?;

// View history
let history = swarm.file_history(&key).await?;

// View audit trail
let trail = swarm.audit_trail(Some(20)).await?;
```

### Conflict Resolution

**Layer 1: Application Layer (Version Control)**
- Detection: Version number mismatch
- Resolution: User handles manually
- Error: `VersionConflict { expected, current }`

**Layer 2: CRDT Layer (Automatic)**
- Detection: Concurrent writes during network partition
- Resolution: Automerge automatically merges
- Strategy: Last-Write-Wins based on timestamp

**Layer 3: Audit Layer (Post-facto)**
- Detection: Review history
- Resolution: Rollback to correct version

### Limitations

- **Full version storage** - Each version stores complete file (no delta)
- **10MB size limit** - Files larger than 10MB rejected with `FileTooLarge` error
- **No automatic content merge** - User must manually merge conflicting changes
- **No indexing** - `list_files()` and `audit_trail()` not fully implemented yet

See [docs/VERSION_CONTROL_ARCHITECTURE.md](docs/VERSION_CONTROL_ARCHITECTURE.md) for detailed architecture.

## Error Handling

### Error Types

All operations return `Result<T, Error>`:

```rust
pub enum Error {
    Storage(rusqlite::Error),      // Storage backend error
    Network(String),                // Network error
    Serialization(String),          // Serialization error
    Election(String),               // Leader election error
    Sync(String),                   // CRDT sync error
    Io(std::io::Error),            // File I/O error
    NotFound(String),              // Key not found

    // file-ops feature errors
    FileTooLarge { size: usize, max: usize },
    VersionConflict { expected: u64, current: u64 },

    Other(String),                 // Other errors
}
```

### API Signatures

```rust
// KV operations
pub async fn put(&self, key: &str, value: &[u8]) -> Result<()>
pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>
pub async fn delete(&self, key: &str) -> Result<()>

// File operations (file-ops feature)
pub async fn put_file(&self, file_path: &Path) -> Result<String>
pub async fn put_file_with_version(&self, file_path: &Path, expected_version: Option<u64>) -> Result<String>
pub async fn get_file(&self, key: &str, output_path: &Path) -> Result<()>
pub async fn get_file_version(&self, key: &str, version: u64, output_path: &Path) -> Result<()>
```

### Error Handling Patterns

```rust
// Pattern 1: Propagate with ?
swarm.put("key", b"value").await?;

// Pattern 2: Match specific errors
match swarm.put_file(path).await {
    Ok(key) => { /* success */ }
    Err(Error::FileTooLarge { size, max }) => {
        eprintln!("File too large: {} bytes (max: {})", size, max);
    }
    Err(Error::VersionConflict { expected, current }) => {
        eprintln!("Version conflict: expected {}, current {}", expected, current);
    }
    Err(e) => eprintln!("Error: {}", e),
}

// Pattern 3: Retry on conflict
let mut retries = 0;
loop {
    let meta = swarm.file_metadata(&key).await?.unwrap();
    match swarm.put_file_with_version(path, Some(meta.version)).await {
        Ok(_) => break,
        Err(Error::VersionConflict { .. }) if retries < 3 => {
            retries += 1;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(e) => return Err(e),
    }
}
```

See [docs/ERROR_HANDLING.md](docs/ERROR_HANDLING.md) for complete guide.

## Build and Development

### Prerequisites

- Rust 1.70+
- Cargo

### Build Commands

```bash
# Build debug
cargo build

# Build release (optimized)
cargo build --release

# Build with file-ops feature
cargo build --features file-ops

# Run tests
cargo test

# Run tests with file-ops
cargo test --features file-ops

# Run specific example
cargo run --example basic
cargo run --example file_sync --features file-ops

# Run benchmarks
cargo run --example benchmark --release
```

### Project Structure

```
murmur/
├── src/
│   ├── lib.rs              # Main library entry point
│   ├── error.rs            # Error types
│   ├── network.rs          # P2P networking
│   ├── sync.rs             # CRDT synchronization
│   ├── election.rs         # Leader election
│   ├── storage.rs          # Storage implementation
│   ├── storage_trait.rs    # Storage trait
│   ├── vector_clock.rs     # Vector clock for causality
│   ├── file.rs             # File operations (file-ops feature)
│   └── discovery_iroh.rs   # mDNS discovery
├── examples/
│   ├── basic.rs            # Basic KV usage
│   ├── group_chat.rs       # Chat application
│   ├── file_sync.rs        # File sync with versioning
│   ├── benchmark.rs        # Performance benchmarks
│   └── ...
├── tests/
│   ├── simple_sync_test.rs
│   ├── collaborative_editing.rs
│   └── file_ops_test.rs    # File operations tests
├── docs/                   # Technical documentation
├── Cargo.toml
├── README.md               # User documentation
├── ROADMAP.md              # Project roadmap
└── CLAUDE.md               # This file
```

## Features

### Default Features

```toml
[features]
default = ["redb-backend"]
```

### Optional Features

```toml
# Storage backends (mutually exclusive)
redb-backend = ["redb"]
sqlite-backend = ["rusqlite"]
rocksdb-backend = ["rocksdb"]

# File operations with version control
file-ops = []
```

### Feature Usage

```toml
# Default (redb)
murmur = { path = "../murmur" }

# SQLite backend
murmur = { path = "../murmur", default-features = false, features = ["sqlite-backend"] }

# With file operations
murmur = { path = "../murmur", features = ["file-ops"] }

# SQLite + file operations
murmur = { path = "../murmur", default-features = false, features = ["sqlite-backend", "file-ops"] }
```

## Release Process

This project is part of the r_lit monorepo. Releases are automated via GitHub Actions.

### Version Bump

1. Update version in `Cargo.toml`:
   ```toml
   [package]
   version = "0.2.0"
   ```

2. Commit and push:
   ```bash
   git add Cargo.toml
   git commit -m "chore(murmur): bump version to 0.2.0"
   git push origin main
   ```

3. GitHub Actions will:
   - Detect version change
   - Build binaries for all platforms
   - Create GitHub Release with artifacts

### Supported Platforms

- Linux (x86_64, i686, aarch64) - musl static builds
- macOS (x86_64, aarch64)
- Windows (x86_64, i686)

## Code Style

### Error Handling

- **Never use `unwrap()`** - Always handle errors properly
- **Use `?` operator** - For error propagation
- **Use `expect()` only for impossible cases** - With clear explanation
- **Return `Result<T>`** - All fallible operations

### Naming Conventions

- **Functions**: `snake_case`
- **Types**: `PascalCase`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Modules**: `snake_case`

### Documentation

- **Public APIs must have doc comments** - Use `///`
- **Modules should have module-level docs** - Use `//!`
- **Complex logic should have inline comments** - Use `//`
- **Code should be self-documenting** - Clear names, simple logic

### Testing

- **Unit tests** - In same file with `#[cfg(test)]`
- **Integration tests** - In `tests/` directory
- **Examples** - In `examples/` directory
- **Feature-gated tests** - Use `#[cfg(feature = "...")]`

## Performance Considerations

### Benchmarks

Run benchmarks to measure performance:

```bash
cargo run --example benchmark --release
```

Expected performance (modern laptop):
- Throughput: ~10,000 ops/sec (single node)
- Latency: <10ms (local network)
- Memory: ~10MB baseline + data

### Optimization Tips

1. **Use release builds** - `cargo build --release`
2. **Choose appropriate storage backend** - RocksDB for high performance
3. **Batch operations** - Reduce network round-trips
4. **Limit file sizes** - Use chunking for large files
5. **Monitor memory usage** - CRDT state grows with data

## Troubleshooting

### Common Issues

**mDNS discovery not working**
- Check firewall settings
- Ensure multicast is enabled on network
- See [docs/DISCOVERY.md](docs/DISCOVERY.md)

**Version conflicts**
- Use `put_file_with_version()` for conflict detection
- Implement retry logic with exponential backoff
- See [docs/CONFLICT_RESOLUTION.md](docs/CONFLICT_RESOLUTION.md)

**File too large errors**
- Default limit: 10MB
- Implement chunking for larger files
- Or use external storage (S3, etc.)

**Storage backend errors**
- Check disk space
- Verify file permissions
- Try different backend

## Documentation

### User Documentation

- [README.md](README.md) - Quick start and basic usage
- [ROADMAP.md](ROADMAP.md) - Project roadmap and plans

### Technical Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - System architecture
- [docs/VERSION_CONTROL_ARCHITECTURE.md](docs/VERSION_CONTROL_ARCHITECTURE.md) - Version control design
- [docs/ERROR_HANDLING.md](docs/ERROR_HANDLING.md) - Error handling guide
- [docs/CONFLICT_RESOLUTION.md](docs/CONFLICT_RESOLUTION.md) - Conflict resolution strategies
- [docs/DISCOVERY.md](docs/DISCOVERY.md) - Local network discovery
- [docs/FILE_OPS.md](docs/FILE_OPS.md) - File operations feature
- [docs/STORAGE_BACKENDS.md](docs/STORAGE_BACKENDS.md) - Storage backend comparison

## Contributing

When contributing to Murmur:

1. **Follow code style** - Use `rustfmt` and `clippy`
2. **Write tests** - Cover new functionality
3. **Update documentation** - Keep docs in sync with code
4. **Handle errors properly** - No `unwrap()` in library code
5. **Keep it simple** - Prefer simple solutions over complex ones

## License

MIT OR Apache-2.0
