# gamereel

> Generate short-form videos from game protocol replays. Rust + FFmpeg + CUDA.

`gamereel` takes a binary game-protocol message blob (battle report, match result, replay frames) and renders it into a TikTok / Instagram Reels-shaped MP4. Each supported game lives in its own crate (`crates/proto-*`) and self-registers via `inventory::submit!` — adding a new game is one dependency line in the CLI, zero edits to the engine.

## Workspace layout

```
gamereel/
├── Cargo.toml                         # workspace root
├── crates/
│   ├── gamereel-core/                 # video generation engine + ProtocolParser trait
│   ├── proto-puzzle/                  # 方块游戏 protocol parser (skeleton)
│   └── proto-bubble/                  # 泡泡龙 protocol parser (skeleton)
├── apps/
│   ├── gamereel-cli/                  # CLI entry: `gamereel render --protocol …`
│   └── hs-mvp/                        # original demo (Hearthstone-style recap)
├── benches/                           # mN.sh + results/mN.json trend artifacts
├── tools/quality-eval/                # VMAF + grid_search + scale_path_bench
└── docs/                              # optimization-log.md, design notes
```

## Build

```bash
cargo build --workspace --release       # all crates
cargo test  --workspace                 # all tests (24 active + 8 CUDA-gated)
cargo bench -p gamereel-core            # criterion benches
cargo run   -p gamereel-cli -- list-protocols
```

**Requires:** ffmpeg dev libraries (`libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev`), `clang`, `pkg-config`. For the CUDA pipeline (M3+): NVIDIA driver ≥ 535 + `libnvrtc12` + `libnvrtc-builtins12.0`. For quality benches: `vmaf` (Netflix libvmaf 3.x) + `jq`.

## Adding a new game protocol

1. Create `crates/proto-<gamename>/` (use `proto-puzzle` as template).
2. Implement `ProtocolParser` for your type, register with `inventory::submit!`.
3. Add the new crate to `apps/gamereel-cli/Cargo.toml` `[dependencies]` and to `src/main.rs` as `use proto_<gamename> as _;` (force-link so `inventory` constructors aren't stripped).

`gamereel list-protocols` should now show your new parser.

## Performance trend (linux-nvenc-refactor branch)

Single-stream 720x1080 / 30 fps / 10 s on RTX 3060 + i7-13700K. Two columns matter:

  * **e2e fps** — `perf_main` median across 5 runs (composition + scaling + encode).
  * **encoder fps** — pre-decoded source pumped straight at the encoder, so this is encoder+scaler only.

| Milestone | Encoder | e2e fps (perf_main) | encoder fps (shootout) | VMAF | Notes |
|---|---|---:|---:|---:|---|
| **M0** | libx264 medium @ 6 Mbps | **152** | 535 (shell) / 152 (criterion) | 99.34 | hardcoded videotoolbox crashed on Linux; libx264 used for measurement |
| **M1** | h264_nvenc p4 balanced (auto) | **377 (2.48× M0)** | NVENC p4: 475, NVENC p2: **619**, libx264: 520 | 98.42 / 99.05 | encoder auto-pick, scaler hoisted, z-order deterministic |
| **M2** | EncoderProfile::Balanced (default) | **381 (1.01× M1)** | Fast 474 / Balanced 462 / TikTokHQ 400 / IgReelsHDR 416 | Fast 97.87 / Balanced 97.73 / **TikTokHQ 97.48** / HDR 97.48 | 4 named profiles, 144-point VMAF grid; e2e flat — see [O-011](docs/optimization-log.md#o-011) |
| **M3** | CUDA hwframes + h264_nvenc (cudarc kernel) | **456 (1.23× M2)** | (CUDA-only path) | unchanged from M2 | full GPU pipeline; cudarc RGBA→NV12 kernel; ffmpeg CUDA hwframes pool; **0 MB VRAM leak / 100 cycles** ([O-012..014](docs/optimization-log.md)) |
| **M5 (1 worker)** | LocalWorker (persistent CUDA) | **1004 fps** (3.3× M3) | — | unchanged | **p99 290 ms** per video; latency-best ([O-018..020](docs/optimization-log.md)) |
| **M5 (2 workers)** | WorkerPool, throughput-tuned | **1113 fps** (3.7× M3) | — | unchanged | 100 hs-mvp videos in **26.9 s** wall; p99 547 ms |
| M4 (target) | wgpu compositor + CUDA + NVENC | (target ≥ 1500 single, multi unknown) | — | ≥ 95 | replaces image_effect.rs CPU compositor |

**Where the CPU time really goes** (measured in M2's [`cpu_breakdown`](crates/gamereel-core/tests/cpu_breakdown.rs) test): for the perf_main scene, RGBA→YUV color conversion (`sws_scale`, CPU SIMD) was **46%** of phase time; NVENC submit+wait **41%**; CPU compositing only **13%**. M3 eliminated the sws_scale slug; M4's wgpu compositor will eliminate the compositing slug; M5's actor pool will multiply the throughput across the GPU's NVENC ceiling.

**Forensic discipline**: every perf change has an entry in [`docs/optimization-log.md`](docs/optimization-log.md) recording the *hypothesis*, the *self-proof test*, the *measured delta*, and a *retro* explaining any gap between projection and reality. Read it back when revisiting a decision months from now.

## License

See LICENSE file.
