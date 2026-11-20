# R_LIT

A collection of independent Rust CLI tools and companion desktop utilities maintained for medium- and long-term evolution. Each subdirectory is a standalone Cargo crate (with root `Cargo.toml` exposing `deskpet` only), and CLI binaries converge on the shared [`crates/rlit_cli`](crates/rlit_cli/) contract (`--json`, `__schema`, exit codes, `runlog`).

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)

## Directory

| Directory | Description |
|-----------|-------------|
| [artifact-inspect](artifact-inspect/README.md) | Offline directory/ZIP/TAR.GZ inventories; per-layer JSON and HTML metadata/image previews; validated on macOS arm64 and in a Linux x86_64 pilot |
| [audiogen](audiogen/) | Pure-Rust game audio pipeline (procedural SFX, MP3/WAV/FLAC encoding, async MiniMax jobs) |
| [bulk_upload](bulk_upload/) | Batch extract URLs from JSON and upload to S3-compatible storage |
| [deskpet](deskpet/) | Frameless transparent always-on-top 3D/Spine desktop mascot (Bevy + tray / menu-bar resident) |
| [gamereel](gamereel/) | Render game protocol replays into short videos (Cargo workspace; CUDA + FFmpeg) |
| [group_vibe_workbench](group_vibe_workbench/) | Desktop collaboration workbench (GPUI + Murmur P2P) |
| [img_resize](img_resize/) | Image resizing and compression |
| [looplog](looplog/) | Local log intake/query tool for AI-assisted debugging (see [WeChat AI debug guide](looplog/WECHAT_AI_DEBUG_CN.md)) |
| [mailctl](mailctl/) | Multi-mailbox IMAP/SMTP dispatcher — per-mailbox identity, allowlist, confirm-before-send, audit |
| [maquette](maquette/) | Kit-based low-poly modeling + glTF export (GUI + headless CLI) |
| [mini_reg](mini_reg/) | Persistent native-Rust helper for company-owned mini-app registration; enterprise Chrome + SQLite resume |
| [mj_atlas](mj_atlas/) | Sprite atlas / packing tool with incremental builds and polygon meshes |
| [mpreview](mpreview/) | Godot model preview & multi-angle camera screenshot tool |
| [phone_haul](phone_haul/) | Phone media migration — Android (adb + on-device agent) and iOS (AFC / Photos.sqlite), dedup + `yyyy/MM` archive |
| [textexture](textexture/) | Art-text image generation (shadow / stroke / gradient / glow / neon) |
| [ui-trim](ui-trim/) | Clean AI-generated UI asset PNGs into tight transparent PNGs |
| [crates/murmur](crates/murmur/) | P2P collaboration library (`iroh-net` + Automerge CRDT) |
| [crates/rlit_cli](crates/rlit_cli/) | Shared CLI machine-call contract (`--json` envelope, `__schema`, exit codes, `runlog`) |

See each subdirectory's README for detailed usage.

## Install (pre-built binaries)

Registered, released tools use the internal Jenkins cross-compile service to publish to the locations below. `artifact-inspect` can be [built from source](artifact-inspect/README.md#build). Its Linux x86_64 pilot artifact is not a registered release or installer.

Release locations for published tools:
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

GUI apps (`maquette`, `group_vibe_workbench`, `deskpet`) ship as a raw binary tarball per platform (Linux/macOS); install the same way.

## Build

```bash
cd <tool_dir> && cargo build --release
```

Or from the repo root using `just`:

```bash
just build <tool> release
just check-repo
```

### Binary layout

- `target/` contains disposable Cargo intermediates and is never release storage.
- Keep an intentionally retained local binary at `<tool>/out/<target-triple>/<binary>`.
- Neither location is committed. Jenkins publishes versioned release artifacts to R2/HFrog.

## Release

The only publish path is the internal **Jenkins cross-compile service** (`ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build`). There is no GitHub Actions release path. Trigger the job manually, pick `TOOL_NAME` and platforms, and it will:

1. Build per platform (Linux x86_64 native / macOS aarch64 native / Windows x86_64 via `cargo-xwin` to `x86_64-pc-windows-msvc`).
2. Package each as `<tool>-<target>.tar.gz` (Windows: `.zip`).
3. Upload to R2 (`s3://prod-hfrog/r_lit/<tool>/v<ver>/`) and refresh `r_lit/<tool>/install.sh`.
4. Sync `software / version / release / platform` records to HFrog with real `file_size`, `checksum_sha256`, `source_type`, and `install_script_url`.

The tool inventory of record is the Jenkinsfile `toolMap`. See [`docs/release.md`](docs/release.md) for the full pipeline diagram.

## License

See LICENSE file in each project.
