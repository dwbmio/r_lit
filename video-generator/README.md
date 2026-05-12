# video-generator

> Rust video generation tools built on FFmpeg.

## Sub-projects

| Directory | Description |
|-----------|-------------|
| [movie-maker](movie-maker/) | Core library — programmatic video generation with FFmpeg, image compositing, and tween animations |
| [demo](demo/) | Demo application (hs-mvp) — example usage of movie-maker for scene rendering |

## movie-maker

A library for generating videos from code. Supports:

- FFmpeg-based video encoding
- Image compositing with `image` + `imageproc`
- Tween-based animation system
- Performance benchmark binary (`perf_main`)

## demo (hs-mvp)

A demo application that uses `movie-maker` to render scenes into video output.

## Build

This directory is a Cargo workspace (root `Cargo.toml` lists `movie-maker` and `demo` as members):

```bash
cargo build --workspace --release       # both crates
cargo test  --workspace                 # all tests
cargo bench -p movie-maker              # criterion benches
```

**Requires:** FFmpeg development libraries (`libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev`), `clang`, `pkg-config`. For quality benches, `vmaf` (Netflix libvmaf 3.x standalone) and `jq`.

## Performance Roadmap (linux-nvenc-refactor branch)

Active branch: `linux-nvenc-refactor`. Plan: `/root/.claude/plans/frolicking-wiggling-curry.md`.

Trend table — single-stream 720x1080 / 30 fps / 10 s on RTX 3060 + i7-13700K.
Two columns matter:
  * **e2e fps** — `perf_main` median across 5 runs (composition + scaling + encode).
  * **encoder fps** — pre-decoded source pumped straight at the encoder, so this is encoder+scaler only.

| Milestone | Encoder | e2e fps (perf_main) | encoder fps (shootout) | VMAF | SSIM | Notes |
|---|---|---:|---:|---:|---:|---|
| **M0** | libx264 medium @ 6 Mbps | **152** | 535 (shell) / 152 (criterion) | 99.34 | 0.999 | hardcoded videotoolbox crashed on Linux; libx264 used for measurement |
| **M1** | h264_nvenc p4 balanced (auto) | **377 (2.48× M0)** | NVENC p4: 475, NVENC p2: **619**, libx264: 520 | 98.42 (p4) / 99.05 (p2) | 0.998 | encoder auto-pick, scaler hoisted, z-order deterministic. CPU compositing now the bottleneck (~80% of e2e time). |
| M2 (target) | NVENC h264 TikTokHQ profile | — | (≥ M1 × 0.8) | ≥ 95 | — | quality-tuned via VMAF grid search |
| M3 (target) | NVENC + CUDA hwframes | ≥ 600 | ≥ 1500 | ≥ 95 | — | zero CPU↔GPU copies |
| M4 (target) | wgpu compositor + NVENC | ≥ 1500 | ≥ 3000 | ≥ 95 | — | replaces image_effect.rs |
| M5 (target) | actor pool, batch-100 | 100 × 10s ≤ 120s wall | — | ≥ 95 | — | hardware-bounded concurrency |

**Why no "5× M0" in M1**: the original 5× target assumed the M0 baseline would be ~30 fps (typical CPU-only path on a modest machine). On this 13700K with the hoisted scaler, libx264 medium itself hits 500+ fps on synthetic sources — there's no headroom for NVENC to be 5× faster on encoding alone. The real wins come in M3/M4 where the CPU-compositing bottleneck (currently ~80% of e2e wall time) gets moved to the GPU. M1 still delivered 2.48× e2e by virtue of the scaler hoist and clean reuse.

Numbers are refreshed each milestone via `benches/baseline.sh` (shell, end-to-end including quality eval) and `cargo bench` (Rust-side, criterion).

## License

See LICENSE file.
