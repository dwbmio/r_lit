# R_LIT

A collection of independent, **short-running** Rust CLI tools and libraries. Each subdirectory is a standalone Cargo crate with no root workspace. This repo is designed for tools that run, do their job, and exit — not for long-running services or daemons.

Homepage: [https://www.snappywatt.com/](https://www.snappywatt.com/)

Pre-built binaries are hosted on [snappywatt.com](https://www.snappywatt.com/) and available for direct download.

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)
[![Release](https://github.com/dwbmio/r_lit/workflows/Release/badge.svg)](https://github.com/dwbmio/r_lit/actions)

## Directory

| Directory | Description |
|-----------|-------------|
| [bulk_upload](bulk_upload/) | Batch extract URLs from JSON and upload to S3-compatible storage |
| [img_resize](img_resize/) | Image resizing and compression |
| [textexture](textexture/) | Art-text image generation (shadow / stroke / gradient / glow / neon) |
| [mj_atlas](mj_atlas/) | Sprite atlas / packing tool with incremental builds |
| [maquette](maquette/) | Kit-based low-poly modeling + glTF export (GUI + headless CLI) |
| [group_vibe_workbench](group_vibe_workbench/) | Desktop collaboration workbench (GPUI + P2P) |
| [video-generator](video-generator/) | Video generation tools |
| [crates/murmur](crates/murmur/) | P2P collaboration library (iroh-net + Automerge CRDT) |

See each subdirectory's README for detailed usage.

## Install (pre-built binaries)

Every CLI tool is published to:
- **GitHub Releases** — `https://github.com/dwbmio/r_lit/releases`
- **Cloudflare R2 (mirror, stable URL)** — `https://gamesci-lite.com/r_lit/<tool>/`
- **HFrog tracker** — `https://hfrog.gamesci-lite.com/api/release/softwares/<tool>`

One-line install (auto-detects Linux / macOS / Windows-Git-Bash):

```bash
curl -fsSL https://gamesci-lite.com/r_lit/<tool>/install.sh | bash
```

Examples:

```bash
curl -fsSL https://gamesci-lite.com/r_lit/bulk_upload/install.sh | bash
curl -fsSL https://gamesci-lite.com/r_lit/img_resize/install.sh  | INSTALL_DIR=$HOME/.local/bin bash
```

GUI apps (`maquette`, `group_vibe_workbench`) ship as a notarized `.dmg`
on macOS; download from the GitHub Release page.

## Build

```bash
cd <tool_dir> && cargo build --release
```

## Release

Bump `version` in any `<tool>/Cargo.toml`, push to `main`, and the
`Release` workflow will:

1. Build the matrix of `targets` declared in `release-metadata.json` for that tool.
2. Create a GitHub Release with binaries + `SHA256SUMS` + per-asset checksum table.
3. Mirror everything to R2 (`s3://prod-gamesci-lite/r_lit/<tool>/v<ver>/`)
   and refresh `r_lit/<tool>/install.sh`.
4. Sync `software / version / release / platform` records to HFrog with
   real `file_size`, `checksum_sha256`, `source_type`, and `install_script_url`.

To add a tool, append it to `release-metadata.json` (`description`,
`category`, optional `gui`/`macos_app_name`/`targets`). Nothing else.

See [`docs/release.md`](docs/release.md) for the full pipeline diagram and
required GitHub secrets.

## License

See LICENSE file in each project.
