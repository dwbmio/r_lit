# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`group_vibe_workbench` is a desktop collaboration workbench built with GPUI (GPU-accelerated UI framework) and integrated with Murmur for P2P collaboration. It provides a native desktop application for multi-user collaborative editing of shared Prompt context files.

**Version**: 0.1.0

**Architecture**:
- **GPUI 0.2.2** - Native UI framework for window management and controls
- **gpui-component 0.5.1** - Component library with WebView support
- **Murmur 0.1.0** - P2P collaboration library with CRDT synchronization
- **Rust** - System-level performance and memory safety

**Core Features**:
- Multi-user collaborative editing of shared files
- **Zero-configuration local network discovery** using mDNS
- **Dynamic group discovery and creation** - no hardcoded group IDs
- P2P synchronization using Murmur's CRDT-based approach
- Automatic leader election for coordination
- Local file persistence with distributed sync

**Monorepo Context**: This project is part of the r_lit monorepo containing multiple independent Rust CLI tools. Murmur is located in `crates/murmur` and provides the P2P collaboration infrastructure. See [../../CLAUDE.md](../../CLAUDE.md) for monorepo-level documentation.

## Build and Development Commands

```bash
# Build debug version
cargo build

# Build release version (optimized with LTO, stripped)
cargo build --release

# Run the application
cargo run -- launch

# Run with custom window size
cargo run -- launch --width 1920 --height 1080

# Install locally (macOS: /usr/local/bin, Windows: D://dtool)
just install_loc release

# Build from monorepo root
cd ../.. && just build group_vibe_workbench release

# Generate changelog (requires git-cliff)
just gen_doc
```

## Release Process

This project follows the same release process as other tools in the r_lit monorepo. To release a new version:

1. Update the version in `Cargo.toml`:
   ```toml
   version = "0.2.0"
   ```

2. Commit and push:
   ```bash
   git add Cargo.toml
   git commit -m "chore(group_vibe_workbench): bump version to 0.2.0"
   git push origin main
   ```

3. GitHub Actions will automatically:
   - Detect the version change in `Cargo.toml`
   - Build binaries for all platforms (Linux musl, macOS, Windows)
   - Create a GitHub Release with all artifacts

**Supported Platforms**:
- Linux (x86_64, i686, aarch64) - musl static builds
- macOS (x86_64, aarch64)
- Windows (x86_64, i686)

## Architecture

### Entry Point and CLI Structure
[main.rs](src/main.rs) defines the CLI using `clap` derive macros:
- Single `launch` subcommand to start the GUI
- `--width` and `--height` flags for window size configuration
- Global `--json` flag for structured output (currently unused, reserved for future features)
- Initializes `fern` logger with RFC3339 timestamps (debug level in dev, info in release)

### Shared File Module
[shared_file.rs](src/shared_file.rs) implements collaborative file editing:
- `SharedFile` struct manages a file synchronized across peers using Murmur
- Automatic initialization of Murmur swarm with P2P networking
- Local file persistence combined with distributed CRDT synchronization
- Node information tracking (node ID, leader status, connected peers)

**Key operations**:
- `new()` - Initialize shared file with storage path, group ID, and file key
- `get_content()` - Retrieve current file content
- `update_content()` - Update content and sync to all peers
- `node_info()` - Get current node's status in the swarm
- `shutdown()` - Gracefully shutdown the swarm

### Error Handling Strategy
All errors flow through [error.rs](src/error.rs)'s `AppError` enum using `thiserror`:
- **Strict rules**: No `unwrap()`, no direct `panic!()`
- Use `?` operator for error propagation
- `expect()` only for logically impossible failures with clear explanations
- Error variants: `Io`, `Gpui`, `Config`, `Other`

### Subcommand: `launch`
[subcmd/launch.rs](src/subcmd/launch.rs) implements the application window:

1. **Application Initialization**: Creates GPUI `Application` instance with `Application::new().run()`
2. **Component Setup**: Initializes `gpui-component` library via `gpui_component::init(cx)`
3. **Window Creation**: Opens main window with `cx.open_window()` using centered bounds
4. **UI Flow**:
   - **Login Page**: User enters nickname (stored in local database)
   - **Group Discovery Page**: Discovers groups on local network via iroh's LocalSwarmDiscovery
   - **Group Lobby Page**: Shows members and collaboration interface
5. **Shared File Integration**: Initializes `SharedFile` for collaborative editing when joining a group

**Current Implementation**: The app provides a complete flow from login → group discovery → group lobby. Users can:
- Automatically discover groups on the local network (zero configuration)
- Create new groups with auto-generated IDs
- Join discovered groups and see member information
- The SharedFile is initialized when joining a group, with full Murmur integration

### UI Layout
The application uses a vertical flex layout:
- **Top Menu Bar** (40px): File, Edit, View, Help menus with hover effects
- **Main Content Area**: Centered WebView container (800x500px) with placeholder text

### Logging System
The project uses `fern` + `log` for application logging:
- Configured in [main.rs](src/main.rs) `init_logger()` function
- RFC3339 timestamp format
- Debug level in debug builds, Info level in release builds
- Outputs to stdout

**Note**: `tracing` and `tracing-subscriber` are also included as dependencies but not currently used. The active logging system is `fern` + `log`.

### Key Dependencies
- `gpui` (0.2.2): Core UI framework
- `gpui-component` (0.5.1): Component library with WebView feature
- `murmur` (0.1.0): P2P collaboration library (local path dependency)
- `clap` (derive): CLI argument parsing
- `thiserror`: Error enum derivation
- `anyhow`: Additional error handling utilities
- `tokio` (rt-multi-thread, macros, full): Async runtime for Murmur integration
- `log` + `fern` + `humantime`: Logging infrastructure
- `tracing` + `tracing-subscriber`: Structured logging (included but not used)
- `dotenv`: Environment variable management
- `serde` + `serde_json` + `serde_yaml`: Serialization

## Platform-Specific Requirements

### macOS
- Metal Toolchain (installed via `xcodebuild -downloadComponent MetalToolchain`)
- No additional runtime dependencies

### Linux
```bash
# Ubuntu/Debian
sudo apt-get install libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev \
                     libwebkit2gtk-4.0-dev

# Fedora
sudo dnf install libxcb-devel libxkbcommon-devel webkit2gtk3-devel
```

### Windows
- WebView2 Runtime (pre-installed on Windows 11)
- Download: https://developer.microsoft.com/microsoft-edge/webview2/

## Murmur Integration

### Overview
Group Vibe Workbench uses Murmur (located in `crates/murmur`) for P2P collaborative editing. Murmur provides:
- **Local network discovery** via iroh's native `LocalSwarmDiscovery` (zero configuration)
- **CRDT-based synchronization** using Automerge for conflict-free merges
- **Automatic leader election** using the Bully algorithm
- **NAT traversal** via iroh-net networking stack
- **Local persistence** with multiple storage backend options (redb, SQLite, RocksDB)

### Discovery Process
1. **Automatic Broadcasting**: Each node broadcasts its presence on the local network via LocalSwarmDiscovery
2. **Peer Detection**: iroh maintains a NodeMap of discovered peers
3. **Connection**: Application calls `discover_and_connect_local_peers()` to connect to discovered nodes
4. **ALPN Negotiation**: QUIC connections use "murmur" ALPN protocol for authentication

### Architecture
The integration follows this flow:
1. **Initialization**: `SharedFile::new()` creates a Murmur `Swarm` instance
2. **Swarm Start**: Begins P2P networking, LocalSwarmDiscovery, and leader election
3. **Discovery**: Waits 5-10 seconds for peer discovery, then connects
4. **Content Sync**: File changes are propagated via `swarm.put()` which broadcasts to all peers
5. **Local Persistence**: Changes are saved to local file system and Murmur's storage backend
6. **Conflict Resolution**: CRDT ensures all peers converge to the same state

### File Structure
```
src/
├── main.rs           # CLI entry point
├── error.rs          # Error types
├── shared_file.rs    # Murmur integration for collaborative files
├── user_db.rs        # User database (redb)
├── gui/              # GUI components
│   ├── mod.rs
│   ├── theme.rs      # Theme system (Catppuccin Mocha)
│   ├── toast.rs      # Toast notifications
│   └── pages/        # Page components
│       ├── mod.rs
│       ├── login_popview.rs      # Login page
│       ├── group_discovery.rs    # Group discovery page
│       └── group_lobby.rs        # Group lobby page
└── subcmd/
    ├── mod.rs
    └── launch.rs     # GPUI window and UI flow
```

### Usage Pattern
```rust
// Initialize shared file
let shared_file = SharedFile::new(
    PathBuf::from("./workbench_data"),  // Storage path
    "default_group".to_string(),         // Group ID
    "prompt_context".to_string(),        // File key
    PathBuf::from("./prompt_context.txt") // Local file path
).await?;

// Get content
let content = shared_file.get_content().await;

// Update content (syncs to all peers)
shared_file.update_content("New content".to_string()).await?;

// Get node info
let info = shared_file.node_info().await;
println!("Node ID: {}", info.node_id);
println!("Is Leader: {}", info.is_leader);
println!("Connected Peers: {:?}", info.connected_peers);
```

## Development Notes

### GPUI API Version
This project uses `gpui 0.2.2` (via `gpui-component 0.5.1`), which has different APIs than the latest GPUI from Zed's repository. Always refer to:
- GPUI 0.2.2 examples in `~/.cargo/registry/src/.../gpui-0.2.2/examples/`
- gpui-component source code for correct API usage

**Key API differences from latest GPUI**:
- `Application::new().run()` instead of `App::new()`
- `cx.open_window()` with `WindowOptions` struct
- `cx.new(|_| View {})` for creating views
- `Render` trait with `render(&mut self, window: &mut Window, cx: &mut Context<Self>)` signature

### Color Scheme
The application uses Catppuccin Mocha color palette:
- Background: `#1e1e2e`
- Surface: `#313244`
- Border: `#45475a`
- Text: `#cdd6f4`
- Subtext: `#bac2de`
- Success: `#a6e3a1`

### Styling
GPUI uses a Tailwind-like API:
```rust
div()
    .flex()              // display: flex
    .flex_col()          // flex-direction: column
    .items_center()      // align-items: center
    .justify_center()    // justify-content: center
    .size_full()         // width: 100%, height: 100%
    .bg(rgb(0x1e1e2e))   // background-color
    .px_4()              // padding-x: 1rem
    .hover(|style| style.bg(rgb(0x45475a)))  // hover state
```

### Testing
Currently, there are no tests in this project. When adding tests:
- Place unit tests in the same file as the code using `#[cfg(test)]` modules
- Place integration tests in a `tests/` directory
- Use `cargo test` to run all tests
- Use `cargo test --test <name>` to run a specific integration test

## Release Profile
Cargo.toml configures aggressive size optimization:
- LTO enabled
- Panic = abort
- Stripped symbols
- opt-level = "z" (optimize for size)

## Troubleshooting

### Build Errors

**"Metal Toolchain not found"** (macOS):
```bash
xcodebuild -downloadComponent MetalToolchain
```

**"cannot find -lwebkit2gtk-4.0"** (Linux):
```bash
# Ubuntu/Debian
sudo apt-get install libwebkit2gtk-4.0-dev

# Fedora
sudo dnf install webkit2gtk3-devel
```

**GPUI API mismatch errors**:
- Ensure you're using `gpui 0.2.2` APIs, not the latest Zed GPUI APIs
- Check `gpui-component` source code for correct usage patterns
- Refer to GPUI 0.2.2 examples in `~/.cargo/registry/src/.../gpui-0.2.2/examples/`

### Runtime Issues

**Window doesn't appear**:
- Check logs for GPUI initialization errors
- Verify platform-specific dependencies are installed
- Try running with `RUST_LOG=debug` for more verbose output

**Application crashes on startup**:
- Ensure Metal Toolchain is installed (macOS)
- Verify WebView2 Runtime is installed (Windows)
- Check that all Linux dependencies are present

## Related Documentation
- [README.md](README.md) - User-facing documentation
- [WEBVIEW_INTEGRATION.md](WEBVIEW_INTEGRATION.md) - WebView integration guide
- [PROJECT_SUMMARY.md](PROJECT_SUMMARY.md) - Complete project summary (in Chinese)
- [../../CLAUDE.md](../../CLAUDE.md) - Monorepo-level documentation
