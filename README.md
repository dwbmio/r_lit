# R_LIT Toolkit

Cross-platform CLI tools for image processing and file uploading.

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)
[![Release](https://github.com/dwbmio/r_lit/workflows/Release/badge.svg)](https://github.com/dwbmio/r_lit/actions)

[中文文档](README_CN.md)

## Tools

- **bulk_upload** - Batch download URLs and upload to S3 object storage
- **img_resize** - Image resizing and compression tool

## Quick Install

### One-line Install (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/dwbmio/r_lit/main/install.sh | sh
```

### Using Cargo

```bash
cargo install bulk_upload
cargo install img_resize
```

### Download Pre-built Binaries

Visit the [Releases](https://github.com/dwbmio/r_lit/releases) page to download binaries for your platform.

**Supported Platforms:**
- Linux (x86_64, i686, ARM64) - musl static builds
- macOS (x86_64, ARM64)
- Windows (x86_64, i686)

## Usage

### bulk_upload

Extract URLs from JSON and batch upload to S3:

```bash
# Basic usage
cat data.json | bulk_upload jq -s ~/.s3config -p "images/"

# JSON output mode
bulk_upload --json jq -s ~/.s3config < data.json

# Show help
bulk_upload jq --help
```

**Config file format** (`.s3`):
```env
S3_BUCKET=my-bucket
S3_ACCESS_KEY=your-access-key
S3_SECRET_KEY=your-secret-key
S3_ENDPOINT=https://s3.example.com
S3_REGION=us-east-1
```

### img_resize

Resize or compress images:

```bash
# Scale proportionally to max 800px
img_resize r_resize -m 800 image.jpg

# Exact resize to 1920x1080
img_resize r_resize --rw 1920 --rh 1080 image.jpg

# Batch process directory
img_resize r_resize -m 1024 images/

# Compress with TinyPNG
img_resize tinyfy images/

# JSON output mode
img_resize --json r_resize -m 800 image.jpg

# Show help
img_resize --help
```

## Features

### bulk_upload
- ✅ Automatically extract all URLs from JSON recursively
- ✅ Automatic URL deduplication
- ✅ Concurrent batch download and upload
- ✅ S3-compatible storage support (MinIO, AWS S3, Aliyun OSS)
- ✅ JSON output mode for programmatic parsing
- ✅ Detailed progress and error reporting

### img_resize
- ✅ Pure Rust implementation, no network dependencies
- ✅ Support PNG and JPG formats
- ✅ Three resize modes: config file, proportional, exact
- ✅ Batch process directories
- ✅ TinyPNG API integration
- ✅ JSON output mode
- ✅ Preserve image quality

## AI-Friendly

These tools are optimized for AI invocation:

- **Clear --help output**: Detailed parameter descriptions and usage examples
- **JSON output mode**: Structured data for easy parsing
- **Standardized error handling**: Clear error messages and exit codes
- **Pipeline-friendly**: Support stdin/stdout data streams

## Development

### Build

```bash
# Build all tools
cargo build --release

# Build individual tool
cd bulk_upload && cargo build --release
cd img_resize && cargo build --release
```

### Test

```bash
# Run all tests
cargo test

# Run tests for individual tool
cd bulk_upload && cargo test
cd img_resize && cargo test
```

### Release New Version

To release a new version, update the version in the tool's `Cargo.toml`:

```bash
# Update version in Cargo.toml
cd bulk_upload
# Edit Cargo.toml: version = "0.3.0"

# Commit and push
git add .
git commit -m "chore(bulk_upload): bump version to 0.3.0"
git push origin main
```

GitHub Actions will automatically detect the version change and build binaries for all platforms.

## System Requirements

- **OS**: macOS, Linux, Windows
- **Architecture**: x86_64, i686, ARM64
- **Dependencies**: None (statically linked)

## License

See LICENSE file in each project.

## Contributing

Issues and Pull Requests are welcome!
