# R_LIT

A collection of independent Rust tools and libraries. Each subdirectory is a standalone Cargo crate with no root workspace.

Homepage: [https://www.snappywatt.com/](https://www.snappywatt.com/)

Pre-built binaries are hosted on [snappywatt.com](https://www.snappywatt.com/) and available for direct download.

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)
[![Release](https://github.com/dwbmio/r_lit/workflows/Release/badge.svg)](https://github.com/dwbmio/r_lit/actions)

## Directory

| Directory | Description |
|-----------|-------------|
| [bulk_upload](bulk_upload/) | Batch extract URLs from JSON and upload to S3-compatible storage |
| [img_resize](img_resize/) | Image resizing and compression |
| [group_vibe_workbench](group_vibe_workbench/) | Desktop collaboration workbench (GPUI + P2P) |
| [pgpour](pgpour/) | Postgres CDC → Kafka data pipeline |
| [omniplan_covers_ding](omniplan_covers_ding/) | Internal tool (has external path dependency) |
| [video-generator](video-generator/) | Video generation tools |
| [crates/murmur](crates/murmur/) | P2P collaboration library (iroh-net + Automerge CRDT) |

See each subdirectory's README for detailed usage.

## Build

```bash
cd <tool_dir> && cargo build --release
```

## License

See LICENSE file in each project.
