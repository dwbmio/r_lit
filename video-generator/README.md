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

Trend table — single-stream 720x1080 / 30 fps / 10 s synthetic source on RTX 3060 + i7-13700K:

| Milestone | Encoder | Throughput (fps) | Realtime × | VMAF | SSIM | PSNR_Y | Notes |
|---|---|---:|---:|---:|---:|---:|---|
| **M0 (baseline)** | libx264 medium @ 6 Mbps | **152** | 5.07× | 99.34* | 0.999* | 53.5* | * shell baseline; criterion bench measures 152 fps Rust-side |
| M1 (target) | NVENC h264 p4 vbr | ≥ 760 | ≥ 25× | ≥ 92 | — | — | 5x over M0 |
| M2 (target) | NVENC h264 TikTokHQ | (≥ M1 × 0.8) | — | ≥ 95 | — | — | quality-tuned |
| M3 (target) | NVENC + CUDA hwframes | ≥ 1500 | ≥ 50× | ≥ 95 | — | — | zero CPU↔GPU copies |
| M4 (target) | wgpu compositor + NVENC | ≥ 3000 | ≥ 100× | ≥ 95 | — | — | replaces image_effect.rs |
| M5 (target) | actor pool, batch-100 | (100 × 10s ≤ 120s wall) | — | ≥ 95 | — | — | hardware-bounded concurrency |

Numbers are refreshed each milestone via `benches/baseline.sh` (shell, end-to-end including quality eval) and `cargo bench` (Rust-side, criterion).

## License

See LICENSE file.
