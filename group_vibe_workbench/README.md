# Group Vibe Workbench

A desktop collaboration workbench built with Rust, GPUI, and Wry.

## Architecture

- **GPUI** - Native UI framework for window management and controls
- **Wry** - Embedded browser component for interactive content
- **Rust** - System-level performance and memory safety

This hybrid architecture combines the performance of native UI with the flexibility of web technologies.

## Features

- Native desktop application with GPUI
- Embedded WebView for rich interactive content
- Cross-platform support (macOS, Linux, Windows)
- Modern UI with native performance
- Team collaboration tools

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
# Launch the workbench
group_vibe_workbench launch

# Launch with custom window size
group_vibe_workbench launch --width 1920 --height 1080

# Show help
group_vibe_workbench --help
```

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

## Requirements

- Rust 1.70+
- Platform-specific WebView dependencies

### macOS

No additional dependencies required.

### Linux

```bash
# Ubuntu/Debian
sudo apt-get install libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev \
                     libwebkit2gtk-4.0-dev

# Fedora
sudo dnf install libxcb-devel libxkbcommon-devel webkit2gtk3-devel
```

### Windows

Requires WebView2 Runtime (pre-installed on Windows 11):
- Download: https://developer.microsoft.com/microsoft-edge/webview2/

## License

See LICENSE file.
