# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

R_LIT is a monorepo containing cross-platform CLI tools and libraries written in Rust for image processing, file management, and distributed collaboration. The repository contains multiple independent tools that can be built and released separately.

**Main Tools:**
- **bulk_upload** (v0.2.1) - Batch download URLs from JSON and upload to S3-compatible storage
- **img_resize** (v0.2.0) - Image resizing and compression tool with pure Rust implementation
- **group_vibe_workbench** (v0.1.0) - Desktop collaboration workbench with GPUI + Murmur P2P sync
- **omniplan_covers_ding** (v0.1.0) - Internal tool for OmniPlan cover processing
- **video-generator** - Video generation tools (demo and movie-maker)

**Libraries:**
- **murmur** (v0.1.0) - Distributed P2P collaboration library with automatic leader election and CRDT synchronization (located in `crates/murmur`)

## Build and Development Commands

### Building Tools

```bash
# Build all tools (from root)
cargo build --release

# Build specific tool
cd bulk_upload && cargo build --release
cd img_resize && cargo build --release
cd group_vibe_workbench && cargo build --release

# Using just (from root)
just build bulk_upload release
just build img_resize release
just build group_vibe_workbench release

# Install tool locally (macOS: /usr/local/bin, Windows: D://dtool)
cd bulk_upload && just install_loc release
cd img_resize && just install_loc release
cd group_vibe_workbench && just install_loc release
```

### Testing

```bash
# Run tests for all tools
cargo test

# Run tests for specific tool
cd bulk_upload && cargo test
cd img_resize && cargo test
```

### Running Tools

```bash
# bulk_upload
cargo run --manifest-path bulk_upload/Cargo.toml -- jq --help
./bulk_upload/target/release/bulk_upload jq <json> -s ~/.s3config -p "prefix/" -c 10

# img_resize
cargo run --manifest-path img_resize/Cargo.toml -- r_resize --help
./img_resize/target/release/img_resize r_resize -m 800 image.jpg
```

## Release Process

This repository uses GitHub Actions for automated cross-platform releases. Each tool is versioned and released independently.

**To release a new version:**

1. Update the version in the tool's `Cargo.toml`:
   ```toml
   [package]
   version = "0.3.0"
   ```

2. Commit and push to main:
   ```bash
   git add bulk_upload/Cargo.toml
   git commit -m "chore(bulk_upload): bump version to 0.3.0"
   git push origin main
   ```

3. GitHub Actions automatically:
   - Detects version changes in `Cargo.toml` files
   - Builds binaries for all platforms (Linux musl, macOS, Windows)
   - Creates a GitHub Release with artifacts and SHA256 checksums

**Supported Platforms:**
- Linux (x86_64, i686, aarch64) - musl static builds
- macOS (x86_64, aarch64)
- Windows (x86_64, i686)

## Repository Architecture

### Monorepo Structure

Each tool is a separate Cargo workspace with its own dependencies and release cycle. The `murmur` library is located in `crates/` and is used as a local path dependency by `group_vibe_workbench`.

```
r_lit/
├── bulk_upload/          # S3 upload tool
│   ├── src/
│   │   ├── main.rs       # CLI definition with clap
│   │   ├── error.rs      # AppError enum with thiserror
│   │   └── subcmd/
│   │       └── jq.rs     # Core workflow: extract URLs, download, upload
│   └── Cargo.toml
├── img_resize/           # Image processing tool
│   ├── src/
│   │   ├── main.rs       # CLI definition
│   │   ├── error.rs      # Error handling
│   │   └── subcmd/
│   │       ├── r_tp.rs   # Pure Rust resize (config/proportional/exact modes)
│   │       └── tinify_tp.rs  # TinyPNG API integration
│   └── Cargo.toml
├── group_vibe_workbench/ # Desktop collaboration workbench
│   ├── src/
│   │   ├── main.rs       # CLI definition
│   │   ├── error.rs      # Error handling
│   │   ├── shared_file.rs # Murmur integration for P2P file sync
│   │   └── subcmd/
│   │       └── launch.rs # GPUI window + collaborative editing
│   └── Cargo.toml
├── crates/
│   └── murmur/           # P2P collaboration library
│       ├── src/
│       │   ├── lib.rs    # Swarm API
│       │   ├── network.rs # iroh P2P networking
│       │   ├── election.rs # Bully algorithm leader election
│       │   ├── sync.rs   # Automerge CRDT synchronization
│       │   └── storage.rs # Local persistence (redb/SQLite/RocksDB)
│       └── Cargo.toml
├── omniplan_covers_ding/ # Internal tool
├── video-generator/      # Video tools
└── .github/workflows/    # CI/CD automation
```
│   │   ├── main.rs       # CLI definition
│   │   ├── error.rs      # Error handling
│   │   └── subcmd/
│   │       └── launch.rs # GPUI window + WebView integration
│   └── Cargo.toml
├── omniplan_covers_ding/ # Internal tool
├── video-generator/      # Video tools
└── .github/workflows/    # CI/CD automation
```

### Error Handling Pattern

All tools follow the same error handling strategy:
- Define `AppError` enum in `error.rs` using `thiserror`
- No `unwrap()` or direct `panic!()` calls
- Use `?` operator for error propagation
- `expect()` only for logically impossible failures with clear explanations

### CLI Structure

All tools use `clap` derive macros with:
- Global `--json` flag for structured output (AI-friendly)
- Subcommands for different operations
- Detailed help text with examples in Chinese and English
- Support for stdin/stdout pipelines

### Logging

All tools use `fern` + `log` for structured logging:
- RFC3339 timestamps
- Debug level in dev builds, info in release
- Initialized in `main.rs` before command execution

## Tool-Specific Details

### bulk_upload

**Core workflow** ([subcmd/jq.rs](bulk_upload/src/subcmd/jq.rs)):
1. Parse `.s3` dotenv config (bucket, keys, endpoint, region)
2. Recursively extract all HTTP/HTTPS URLs from JSON
3. Deduplicate URLs while preserving order
4. Split into batches based on `--concurrency`
5. Concurrent download with `reqwest` + `futures::join_all`
6. Upload to S3 with `aws-sdk-s3` (path-style for MinIO compatibility)

**Key dependencies:** `tokio`, `reqwest`, `aws-sdk-s3`, `futures`

### img_resize

**Resize modes** ([subcmd/r_tp.rs](img_resize/src/subcmd/r_tp.rs)):
1. **Config mode**: YAML file with multiple output sizes
2. **Proportional mode**: `-m` flag for max dimension, preserves aspect ratio
3. **Exact mode**: `--rw` and `--rh` for specific dimensions

**Key dependencies:** `image`, `imageproc`, `walkdir` (batch processing)

**Note:** TinyPNG integration temporarily disabled for musl builds (see commented dependency in Cargo.toml)

### murmur

**Location**: `crates/murmur/`

**Architecture** ([crates/murmur/src/lib.rs](crates/murmur/src/lib.rs)):
- **P2P Networking**: Built on `iroh-net` for NAT traversal and relay selection
- **Local Discovery**: Uses iroh's native `LocalSwarmDiscovery` for zero-config peer discovery on local networks
- **CRDT Sync**: Uses `automerge` for conflict-free state synchronization
- **Local Storage**: Multiple backend options (redb, SQLite, RocksDB) with feature flags
- **Leader Election**: Bully algorithm with heartbeat mechanism (2s interval, 5s timeout)

**Core components:**
1. **Network** ([network.rs](crates/murmur/src/network.rs)): iroh endpoint, LocalSwarmDiscovery, ALPN configuration, peer management
2. **Election** ([election.rs](crates/murmur/src/election.rs)): Bully algorithm, role management, heartbeat
3. **Sync** ([sync.rs](crates/murmur/src/sync.rs)): Automerge document, CRDT operations
4. **Storage** ([storage.rs](crates/murmur/src/storage.rs)): Pluggable storage backends with trait abstraction

**Key dependencies:** `iroh-net` (with `discovery-local-network` feature), `automerge`, `redb` (default), `tokio`

**Discovery & Connection:**
```rust
// Automatic local network discovery
let swarm = Swarm::builder()
    .group_id("my-group")
    .build()
    .await?;

swarm.start().await?;

// Wait for discovery (5-10 seconds)
tokio::time::sleep(Duration::from_secs(5)).await;

// Connect to discovered peers
let count = swarm.discover_and_connect_local_peers().await?;
println!("Connected to {} peers", count);
```

**Basic Usage:**
```rust
swarm.put("key", b"value").await?;
let value = swarm.get("key").await?;
```

**Documentation:** See [crates/murmur/docs/](crates/murmur/docs/) for detailed documentation:
- [DISCOVERY.md](crates/murmur/docs/DISCOVERY.md) - Local network discovery, mDNS issues, ALPN configuration
- [ARCHITECTURE.md](crates/murmur/docs/ARCHITECTURE.md) - System architecture and design decisions

### group_vibe_workbench

**Core workflow** ([group_vibe_workbench/src/subcmd/launch.rs](group_vibe_workbench/src/subcmd/launch.rs)):
1. Initialize GPUI application and window
2. Create SharedFile instance with Murmur integration
3. Start P2P swarm for collaborative editing
4. Render UI with menu bar and content area
5. Sync file changes across all connected peers

**Murmur integration** ([group_vibe_workbench/src/shared_file.rs](group_vibe_workbench/src/shared_file.rs)):
- `SharedFile` manages a file synchronized via Murmur's CRDT
- Automatic local persistence + distributed sync
- Node information tracking (leader status, connected peers)

**Key dependencies:** `gpui`, `gpui-component`, `murmur` (local path), `tokio`

## CI/CD Workflows

### CI Workflow ([.github/workflows/ci.yml](.github/workflows/ci.yml))
- Triggers on push/PR to main/master/develop
- Builds bulk_upload and img_resize on Linux only
- Runs `--help` to verify binaries work

### Release Workflow ([.github/workflows/release.yml](.github/workflows/release.yml))
- Triggers on Cargo.toml changes in main branch
- Detects which tools changed by diffing Cargo.toml files
- Builds changed tools for all platforms using matrix strategy
- Uses `cross` for cross-compilation (i686, aarch64)
- Packages binaries (tar.gz for Unix, zip for Windows)
- Creates GitHub Release with version from Cargo.toml
- Generates SHA256SUMS for all artifacts

## Development Notes

### Release Profile

All tools use aggressive size optimization in `Cargo.toml`:
```toml
[profile.release]
lto = true
panic = "abort"
strip = true
opt-level = "z"
```

### Just Commands

Each tool has a `.justfile` with Python-based build scripts:
- `just install_loc release` - Build and install to system path
- `just gen_doc` - Generate changelog with git-cliff (if available)

Root `.justfile` provides:
- `just build <tool> <method>` - Build specific tool
- `just install_loc <tool> <method>` - Install specific tool

### Platform-Specific Paths

Binary install paths are platform-dependent:
- macOS: `/usr/local/bin`
- Windows: `D://dtool`
