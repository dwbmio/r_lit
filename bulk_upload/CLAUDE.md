# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`bulk_upload` is a Rust CLI tool for batch downloading files from URLs and uploading them to S3-compatible object storage (including MinIO). It extracts URLs from JSON input, downloads files concurrently in batches, and uploads them to S3.

## Build and Development Commands

```bash
# Build debug version
cargo build

# Build release version (optimized with LTO, stripped)
cargo build --release

# Run the tool
cargo run -- jq <json_text> --s3 /path/to/.s3 --prefix assets/ --concurrency 10

# Install locally (macOS: /usr/local/bin, Windows: D://dtool)
just install_loc release

# Generate changelog (requires git-cliff)
just gen_doc
```

## Architecture

### Entry Point and CLI Structure
- [main.rs](src/main.rs) defines the CLI using `clap` derive macros with a single `Jq` subcommand
- Supports JSON input via direct argument or stdin (for piping)
- Initializes `fern` logger with RFC3339 timestamps (debug level in dev, info in release)

### Error Handling Strategy
All errors flow through [error.rs](src/error.rs)'s `AppError` enum using `thiserror`:
- **Strict rules**: No `unwrap()`, no direct `panic!()`
- Use `?` operator for error propagation
- `expect()` only for logically impossible failures with clear explanations
- AWS SDK errors manually converted via custom `From` impl (generic SDK errors can't use `#[from]`)

### Subcommand: `jq`
[subcmd/jq.rs](src/subcmd/jq.rs) implements the core workflow:

1. **Config Loading**: Parses `.s3` dotenv file for S3 credentials (bucket, access_key, secret_key, endpoint, region)
2. **URL Extraction**: Recursively traverses JSON structure to find all `http://` or `https://` strings, deduplicates while preserving order
3. **Batch Processing**: Splits URLs into chunks based on `--concurrency` parameter
4. **Concurrent Download**: Uses `reqwest` client with `futures::join_all` for parallel downloads within each batch
5. **Concurrent Upload**: Uploads successful downloads to S3 using `aws-sdk-s3` with path-style addressing (MinIO compatible)
6. **S3 Key Generation**: Extracts filename from URL (strips query params), combines with `--prefix`

### S3 Configuration Format
The `.s3` file uses dotenv format:
```
S3_BUCKET=my-bucket
S3_ACCESS_KEY=xxxxx
S3_SECRET_KEY=xxxxx
S3_ENDPOINT=http://minio.example.com:9000
S3_REGION=us-east-1  # optional, defaults to us-east-1
```

### Key Dependencies
- `clap` (derive): CLI argument parsing
- `tokio` (rt-multi-thread): Async runtime
- `reqwest` (stream): HTTP downloads
- `aws-sdk-s3` + `aws-config`: S3 uploads with custom endpoint support
- `futures`: Concurrent task orchestration (`join_all`)
- `thiserror`: Error enum derivation
- `fern` + `log`: Structured logging

## Release Profile
Cargo.toml configures aggressive size optimization:
- LTO enabled
- Panic = abort
- Stripped symbols
- opt-level = "z" (optimize for size)
