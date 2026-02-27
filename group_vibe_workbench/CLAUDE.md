# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`group_vibe_workbench` is a desktop collaboration workbench built with GPUI (GPU-accelerated UI framework) and Wry (WebView integration). It provides a native desktop application with embedded web content capabilities.

**Version**: 0.1.0

**Architecture**:
- **GPUI 0.2.2** - Native UI framework for window management and controls
- **gpui-component 0.5.1** - Component library with WebView support
- **Wry** - Cross-platform WebView engine (via gpui-component)
- **Rust** - System-level performance and memory safety

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
- [main.rs](src/main.rs) defines the CLI using `clap` derive macros with a single `launch` subcommand
- Supports window size configuration via `--width` and `--height` flags
- Initializes `fern` logger with RFC3339 timestamps (debug level in dev, info in release)

### Error Handling Strategy
All errors flow through [error.rs](src/error.rs)'s `AppError` enum using `thiserror`:
- **Strict rules**: No `unwrap()`, no direct `panic!()`
- Use `?` operator for error propagation
- `expect()` only for logically impossible failures with clear explanations

### Subcommand: `launch`
[subcmd/launch.rs](src/subcmd/launch.rs) implements the application window:

1. **Application Initialization**: Creates GPUI `Application` instance
2. **Component Setup**: Initializes `gpui-component` library
3. **Window Creation**: Opens main window with specified dimensions
4. **UI Rendering**: Renders menu bar and content area
5. **WebView Integration**: Placeholder for Wry WebView (to be completed)

### UI Layout
The application uses a vertical flex layout:
- **Top Menu Bar** (40px): File, Edit, View, Help menus
- **Main Content Area**: Centered WebView container (800x500px)

### Key Dependencies
- `gpui` (0.2.2): Core UI framework
- `gpui-component` (0.5.1): Component library with WebView feature
- `clap` (derive): CLI argument parsing
- `thiserror`: Error enum derivation
- `tokio` (rt-multi-thread): Async runtime
- `tracing` + `tracing-subscriber`: Structured logging
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

## WebView Integration

### Current Status
- ✅ Window creation with GPUI
- ✅ Menu bar implementation
- ✅ Layout structure (top menu + centered content)
- ⏳ WebView integration (placeholder implemented)

### Next Steps for WebView
1. Add `raw-window-handle` dependency
2. Get window handle from GPUI window
3. Create Wry WebView with `WebViewBuilder`
4. Embed WebView in GPUI layout
5. Implement Rust ↔ JavaScript communication

See [WEBVIEW_INTEGRATION.md](WEBVIEW_INTEGRATION.md) for detailed integration guide.

## Development Notes

### GPUI API Version
This project uses `gpui 0.2.2` (via `gpui-component 0.5.1`), which has different APIs than the latest GPUI from Zed's repository. Always refer to:
- GPUI 0.2.2 examples in `~/.cargo/registry/src/.../gpui-0.2.2/examples/`
- gpui-component source code for correct API usage

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
```

## Release Profile
Cargo.toml configures aggressive size optimization:
- LTO enabled
- Panic = abort
- Stripped symbols
- opt-level = "z" (optimize for size)

## Related Documentation
- [README.md](README.md) - User-facing documentation
- [WEBVIEW_INTEGRATION.md](WEBVIEW_INTEGRATION.md) - WebView integration guide
- [PROJECT_SUMMARY.md](PROJECT_SUMMARY.md) - Complete project summary
- [../../ci-all-in-one/_ai/rlit-dev/group_vibe_workbench.md](../../ci-all-in-one/_ai/rlit-dev/group_vibe_workbench.md) - Development guide
