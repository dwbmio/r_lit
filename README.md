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
| [ui-trim](ui-trim/) | Clean AI-generated UI asset PNGs into tight transparent PNGs |
| [textexture](textexture/) | Art-text image generation (shadow / stroke / gradient / glow / neon) |
| [mj_atlas](mj_atlas/) | Sprite atlas / packing tool with incremental builds |
| [looplog](looplog/) | Local log intake/query tool for AI-assisted debugging (WeChat Mini Program MVP; see [WeChat AI debug guide](looplog/WECHAT_AI_DEBUG_CN.md)) |
| [maquette](maquette/) | Kit-based low-poly modeling + glTF export (GUI + headless CLI) |
| [group_vibe_workbench](group_vibe_workbench/) | Desktop collaboration workbench (GPUI + P2P) |
| [deskpet](deskpet/) | Frameless transparent always-on-top 3D desktop mascot (Bevy + tray / menu-bar resident) |
| [video-generator](video-generator/) | Video generation tools |
| [crates/murmur](crates/murmur/) | P2P collaboration library (iroh-net + Automerge CRDT) |

See each subdirectory's README for detailed usage.

## Install (pre-built binaries)

Every CLI tool is published (by the internal Jenkins cross-compile service) to:
- **Cloudflare R2 (stable URL, primary)** — `https://r2.gamesci-lite.com/r_lit/<tool>/`
- **HFrog tracker** — `https://hfrog.gamesci-lite.com/api/release/softwares/<tool>`

One-line install (auto-detects Linux / macOS / Windows-Git-Bash):

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/<tool>/install.sh | bash
```

Examples:

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/bulk_upload/install.sh | bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/img_resize/install.sh  | INSTALL_DIR=$HOME/.local/bin bash
```

GUI apps (`maquette`, `group_vibe_workbench`, `deskpet`) ship as a raw binary
tarball per platform (Linux/macOS); install the same way.

## Build

```bash
cd <tool_dir> && cargo build --release
```

## Release

The only publish path is the internal **Jenkins cross-compile service**
(`ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build`). There is no
GitHub Actions release path. Trigger the job manually, pick `TOOL_NAME` and
platforms, and it will:

1. Build per platform (Linux x86_64 native / macOS aarch64 native / Windows
   x86_64 via cargo-xwin to `x86_64-pc-windows-msvc`).
2. Package each as `<tool>-<target>.tar.gz` (Windows: `.zip`).
3. Upload to R2 (`s3://prod-hfrog/r_lit/<tool>/v<ver>/`) and refresh
   `r_lit/<tool>/install.sh`.
4. Sync `software / version / release / platform` records to HFrog with real
   `file_size`, `checksum_sha256`, `source_type`, and `install_script_url`.

The tool inventory of record is the Jenkinsfile `toolMap`.

See [`docs/release.md`](docs/release.md) for the full pipeline diagram.

## License

See LICENSE file in each project.
