# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

R_LIT is a monorepo containing cross-platform CLI tools written in Rust for image processing and file management. The repository contains multiple independent tools that can be built and released separately.

**Main Tools:**
- **bulk_upload** (v0.2.1) - Batch download URLs from JSON and upload to S3-compatible storage
- **img_resize** (v0.2.0) - Image resizing and compression tool with pure Rust implementation
- **group_vibe_workbench** (v0.1.0) - Desktop collaboration workbench with GPUI + Wry WebView
- **omniplan_covers_ding** (v0.1.0) - Internal tool for OmniPlan cover processing
- **video-generator** - Video generation tools (demo and movie-maker)

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

Each tool is a separate Cargo workspace with its own dependencies and release cycle. Tools are independent and do not share code (except `omniplan_covers_ding` which depends on an external `cli-common` crate).

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
