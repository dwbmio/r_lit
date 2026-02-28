# Group Vibe Workbench

A desktop collaboration workbench built with Rust, GPUI, and Murmur for P2P collaborative editing.

## Architecture

- **GPUI** - Native UI framework for window management and controls
- **Murmur** - P2P collaboration library with CRDT synchronization
- **Rust** - System-level performance and memory safety

This architecture combines native UI performance with distributed P2P collaboration, enabling multiple users to edit shared Prompt context files in real-time without a central server.

## Features

- Native desktop application with GPUI
- **Zero-configuration P2P collaboration** - Automatic local network discovery
- **Dynamic group discovery** - Find and join existing groups on your network
- **Create or join groups** - Start a new collaboration or join an existing one
- P2P collaborative file editing powered by Murmur
- Automatic leader election for coordination
- CRDT-based conflict resolution (using Automerge)
- Local file persistence with distributed sync
- Cross-platform support (macOS, Linux, Windows)
- NAT traversal via iroh networking

## Installation

### Using Cargo

```bash
cargo install group_vibe_workbench
```

### From Source

```bash
cd group_vibe_workbench
cargo build --release
```

## Usage

```bash
# Launch the workbench with your nickname
group_vibe_workbench launch --nickname "Alice"

# Or use short form
group_vibe_workbench launch -n "Alice"

# Launch with custom window size
group_vibe_workbench launch -n "Alice" --width 1920 --height 1080

# Show help
group_vibe_workbench --help
```

## How It Works

### Zero-Configuration Discovery

Group Vibe Workbench uses mDNS (Multicast DNS) for automatic local network discovery:

1. **Launch Application**: Start the workbench and log in with your nickname
2. **Discover Groups**: Click "搜索群组" to scan for active groups on your local network
3. **Join or Create**:
   - Join an existing group by clicking on it
   - Create a new group with "创建新群组"
4. **Automatic Connection**: The app automatically connects to peers in the same group
5. **Start Collaborating**: Edit shared files with real-time synchronization

### P2P Collaboration

Group Vibe Workbench uses Murmur for distributed collaboration:

1. **Swarm Initialization**: Each instance creates a Murmur swarm with a unique node ID
2. **Local Discovery**: Nodes advertise themselves on the local network using mDNS
3. **Leader Election**: Nodes automatically elect a leader using the Bully algorithm
4. **File Synchronization**: Changes to the shared Prompt context file are broadcast to all peers
5. **CRDT Merging**: Automerge ensures all peers converge to the same state without conflicts
6. **Local Persistence**: Changes are saved both locally and in Murmur's distributed storage

### Shared Files

The workbench manages a shared "Prompt context" file that multiple users can edit simultaneously:

- **File Key**: `prompt_context` (used for P2P synchronization)
- **Local Path**: `./prompt_context.txt` (local file system)
- **Storage Path**: `./workbench_data/swarm/<user_id>` (Murmur's storage backend)
- **Group ID**: Dynamically discovered or created (e.g., `group_1234567890`)

### Network Discovery

The app uses mDNS for zero-configuration networking:

- **Service Type**: `_murmur._udp.local`
- **Discovery Timeout**: 5 seconds for group discovery, 3 seconds for member discovery
- **Automatic Advertising**: Each node advertises its presence with nickname and group ID
- **No Configuration Required**: Works out of the box on local networks

## Development

### Build

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Install locally (macOS: /usr/local/bin, Windows: D://dtool)
just install_loc release
```

### Run

```bash
# Run debug version
cargo run -- launch

# Run with custom dimensions
cargo run -- launch --width 1600 --height 900
```

### Project Structure

```
group_vibe_workbench/
├── src/
│   ├── main.rs           # CLI entry point
│   ├── error.rs          # Error types
│   ├── shared_file.rs    # Murmur integration
│   └── subcmd/
│       ├── mod.rs
│       └── launch.rs     # GPUI window and UI
├── Cargo.toml
└── CLAUDE.md             # Development guide
```

## Requirements

- Rust 1.70+
- Platform-specific dependencies

### macOS

No additional dependencies required (Metal Toolchain installed via Xcode).

### Linux

```bash
# Ubuntu/Debian
sudo apt-get install libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev

# Fedora
sudo dnf install libxcb-devel libxkbcommon-devel
```

### Windows

No additional dependencies required.

## Technical Details

### Murmur Integration

Murmur provides the P2P collaboration infrastructure:

- **Networking**: Built on iroh for NAT traversal and relay selection
- **CRDT**: Uses Automerge for conflict-free state synchronization
- **Storage**: Supports multiple backends (redb, SQLite, RocksDB)
- **Election**: Bully algorithm for automatic leader selection

### GPUI UI Framework

GPUI provides GPU-accelerated native UI:

- **Version**: 0.2.2 (via gpui-component 0.5.1)
- **Styling**: Tailwind-like API
- **Color Scheme**: Catppuccin Mocha palette
- **Performance**: Native rendering with Metal (macOS), Vulkan (Linux), DirectX (Windows)

## Roadmap

- [ ] Text editor integration in UI
- [ ] Real-time cursor positions
- [ ] User presence indicators
- [ ] Chat/comments system
- [ ] File history and versioning
- [ ] Multiple file support
- [ ] Custom themes

## License

See LICENSE file.

## Contributing

Issues and Pull Requests are welcome!
