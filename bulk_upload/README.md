# bulk_upload

High-speed batch file upload/download CLI for S3-compatible object storage.

Built with Rust for maximum throughput and minimal resource usage. Single static binary, zero dependencies.

## Supported Storage Providers

| Provider | Status |
|----------|--------|
| AWS S3 | ✅ |
| Cloudflare R2 | ✅ |
| Aliyun OSS | ✅ |
| iDrive E2 | ✅ |
| MinIO | ✅ |
| DigitalOcean Spaces | ✅ |
| Any S3-compatible endpoint | ✅ |

## Features

- **Multi-threaded parallel transfers** — saturates available bandwidth
- **Resume interrupted transfers** — no re-upload on failure
- **Checksum verification** — SHA256/MD5 for data integrity
- **Bandwidth throttling** — configurable speed limits
- **Progress tracking** — real-time transfer progress
- **Glob pattern matching** — batch select files with wildcards
- **JSON URL extraction** — extract and download URLs from JSON input
- **Zero dependencies** — single static binary, no runtime required

## Installation

### One-line install (Linux / macOS)

```bash
curl -fsSL https://nexus.gamesci-lite.com/repository/raw-prod/r_lit/bulk_upload/install.sh | TOOL_NAME=bulk_upload bash
```

### Build from source

```bash
cargo install --path .
```

## Quick Start

### 1. Configure S3 credentials

Create a `.s3` config file:

```env
S3_BUCKET=my-bucket
S3_ACCESS_KEY=your-access-key
S3_SECRET_KEY=your-secret-key
S3_ENDPOINT=https://your-account.r2.cloudflarestorage.com
S3_REGION=auto
```

### 2. Upload files from JSON URL list

```bash
# Extract URLs from JSON and batch upload to S3
bulk_upload jq '{"urls":["https://example.com/a.png","https://example.com/b.png"]}' \
  --s3 .s3 --prefix assets/ --concurrency 10

# Pipe JSON from stdin
cat urls.json | bulk_upload jq - --s3 .s3 --prefix textures/v2/
```

## Use Cases

- **Game asset pipelines** — batch upload textures, models, audio to cloud storage
- **CI/CD artifact publishing** — push build outputs to S3/R2/OSS
- **Storage migration** — move files between S3-compatible providers
- **Backup** — sync local directories to cloud with resume support
- **Content delivery** — bulk upload static assets for CDN distribution

## Platforms

| Platform | Architecture | Size |
|----------|-------------|------|
| Linux | x86_64 | ~4MB |
| macOS | ARM64 (Apple Silicon) | ~3MB |

## Performance

- Tokio async I/O runtime for maximum throughput
- Zero-copy file streaming — minimal memory footprint
- Handles thousands of files without memory spikes
- Configurable concurrency level for optimal resource usage

## Why CLI over MCP

`bulk_upload` is a pure CLI tool. AI agents (Claude Code, Cursor, etc.) can invoke it directly via shell — no MCP server needed.

- 30%+ token savings compared to MCP approach
- Zero context window pollution
- LLMs are natively trained on CLI commands

## AI-Friendly Documentation

- **For humans**: This README.md
- **For AI agents**: [llms.txt](https://gamesci-lite.com/llms-txt/en/bulk_upload.txt)
- **Tool page**: [snappywatt.com/tools/bulk-upload](https://snappywatt.com/tools/bulk-upload)

## License

MIT
