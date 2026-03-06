# bulk_upload

> Batch upload & download files to S3-compatible storage. Fast.

## Quick Start

```bash
# Install
curl -fsSL https://nexus.gamesci-lite.com/repository/raw-prod/r_lit/bulk_upload/install.sh | TOOL_NAME=bulk_upload bash

# Create config
cat > .s3 << EOF
S3_BUCKET=my-bucket
S3_ACCESS_KEY=your-key
S3_SECRET_KEY=your-secret
S3_ENDPOINT=https://your-account.r2.cloudflarestorage.com
S3_REGION=auto
EOF

# Upload
cat urls.json | bulk_upload jq - --s3 .s3 --prefix assets/ --concurrency 10
```

## What It Does

Takes a JSON blob, extracts all URLs from it, downloads them in parallel, and uploads them to your S3 bucket. That's it.

```bash
# Direct JSON input
bulk_upload jq '{"files":["https://example.com/a.png","https://example.com/b.png"]}' \
  --s3 .s3 --prefix textures/

# From stdin
curl -s https://api.example.com/assets | bulk_upload jq - --s3 .s3
```

## Supported Storage

AWS S3 · Cloudflare R2 · Aliyun OSS · iDrive E2 · MinIO · anything S3-compatible

## Platforms

| Platform | Arch | Size |
|----------|------|------|
| Linux | x86_64 | ~4MB |
| macOS | Apple Silicon | ~3MB |

## Build From Source

```bash
cargo build --release
```

## For AI Agents

See [llms.txt](./llms.txt) for structured tool description.

## License

MIT
