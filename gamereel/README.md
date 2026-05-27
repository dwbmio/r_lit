# gamereel

> Generate short-form videos from game protocol replays. Rust + FFmpeg + CUDA.

`gamereel` takes a binary game-protocol message blob (battle report, match result, replay frames) and renders it into a TikTok / Instagram Reels-shaped MP4. Each supported game lives in its own crate (`crates/proto-*`) and self-registers via `inventory::submit!` — adding a new game is one dependency line in the CLI, zero edits to the engine.

---

## What gamereel can do today (measured, not promised)

All numbers below are **measured on the actual hs-mvp scene** (720×1080 × 30 fps × 10 s, 6-node compositing with timeline animations) on a reference machine: **NVIDIA RTX 3060 + Intel i7-13700K + Ubuntu 24.04**. Reproduce via `cargo run --release -p hs-mvp --bin farm_bench -- --jobs 100 --workers 1,2,3,4,6,8`.

### ✅ Strengths

| Capability | Measurement |
|---|---|
| **Sub-300 ms per-video latency** (single worker) | p50 284 ms / p99 290 ms — within near-real-time SLA |
| **220 videos/minute throughput** (2 workers) | 100 hs-mvp videos rendered in **26.9 s wall** |
| **Throughput per dollar (consumer GPU)** | 1113 fps on a $300-class RTX 3060 |
| **Quality at platform-grade bitrates** | VMAF 97.5+ across `Fast/Balanced/TikTokHQ` profiles, 144-point grid search calibrated |
| **Zero VRAM growth across long runs** | 0 MB delta after 100 sequential renders ([cuda_vram_leak](crates/gamereel-core/tests/cuda_vram_leak.rs)) |
| **Deterministic output** | Same scene → byte-identical RGBA hashes across runs ([zorder_stable](crates/gamereel-core/tests/zorder_stable.rs)) |
| **Pluggable game protocols** | Drop a `crates/proto-<game>/` + one CLI dep line; `inventory::submit!` self-registers |
| **Cloud-ready** | `Worker` trait abstraction; `RemoteWorker` stub already shipped — adding gRPC dispatch needs no changes outside the new transport module |
| **Rust safety, no leaks** | 30 active tests + 10 CUDA-gated, no `unwrap()` / `panic!()` in production paths |

### ⚠️ Known limitations (also measured)

| Limit | Why | Mitigation |
|---|---|---|
| **Single-NVENC ceiling: workers > 2 doesn't help** | RTX 3060 has one NVENC engine; 2 concurrent streams saturate it | Hardware (4070 Ti Super for 2× NVENC) or M4 (lower per-stream cost) |
| **CPU compose ~40 % of frame budget** | `image_effect.rs` does per-pixel alpha blends on CPU | M4 wgpu compositor is the planned fix (predicted +50–80 % e2e) |
| **Per-frame `synchronize()` in CUDA path** | Kills NVENC async pipelining (~14 % overhead) | Stream-aware NVENC submit (planned with M4) |
| **284 ms one-shot CUDA init per worker process** | NVRTC kernel compile + ffmpeg hwframes pool init | Already amortized by M5 worker pool — only matters if you spawn fresh processes per job |
| **Single GPU only** | No multi-GPU dispatch | Trait abstraction in place; multi-GPU `LocalWorkerPool` is straightforward to add |
| **HDR profile is a stub** | `IgReelsHDR` falls back to `TikTokHQ` H.264 SDR | Real HDR (HEVC Main10 + BT.2020) is M4+ scope |
| **No AV1 encoder** | RTX 3060 NVENC Gen 7 has no AV1 path | Hardware (RTX 40 series Ada or Blackwell) |
| **Game protocol parsers are skeletons** | `proto-puzzle` / `proto-bubble` register but emit placeholder data | Real binary decoders are per-game work, deliberately separated |

---

## Performance trend (measured, M0 → M5)

Same hardware throughout.

| Milestone | What changed | e2e fps (perf_main) | Reference test |
|---|---|---:|---|
| **M0 baseline** | libx264 medium @ 6 Mbps (Linux fallback path) | 152 | M0 |
| **M1** | Encoder auto-pick (NVENC), scaler hoist, z-order BTreeMap | 377 (2.48×) | M1 |
| **M2** | 4 EncoderProfiles + 144-point VMAF grid | 381 (1.01×) | M2 |
| **M3** | Full GPU pipeline (cudarc kernel + ffmpeg CUDA hwframes) | 456 (1.23×) | M3 |
| **M5 (workers=1)** | Persistent CUDA + ffmpeg context across jobs | **1004 fps**, p99 290 ms | M5 |
| **M5 (workers=2)** | WorkerPool round-robin dispatch | **1113 fps**, p99 547 ms | M5 |
| **M4 (single, cold init)** | wgpu compositor + CUDA + h264_nvenc | **444 fps** (single video, includes 454 ms wgpu+cuda init); per-frame ceiling **1072 fps** | M4 |
| **M4.5 (heavy scene case)** | wgpu compose vs CPU image_effect | wgpu wins **37–136×** when per-frame composited area > ~150K px (= 20 % of 720×1080). Shipped as opt-in `GAMEREEL_WORKER_COMPOSITOR=wgpu`; default stays CPU because hs-mvp's dirty cache wins on its scene class. See the [decision matrix](#compositor-decision-matrix-cpu-vs-wgpu). | M4.5 |

**Total trajectory: 152 → 1113 fps = 7.3× over baseline**, with quality maintained (VMAF 97.5+).

The 100-job sweep:

| Workers | Wall (s) | Throughput (fps) | videos/min | p50 (ms) | p99 (ms) |
|---:|---:|---:|---:|---:|---:|
| 1 | 29.9 | 1004 | 201 | **284** | **290** |
| **2** | **26.9** | **1113** | **223** | 518 | 547 |
| 3 | 27.1 | 1107 | 221 | 781 | 810 |
| 4 | 27.4 | 1095 | 219 | 1047 | 1101 |
| 6 | 27.8 | 1078 | 216 | 1583 | 1695 |
| 8 | 28.3 | 1062 | 212 | 2112 | 2329 |

**workers=1 is the latency-best config** (sub-300 ms p99). **workers=2 is the throughput-best config** (+10 % over single, p99 doubles). Going past 2 just queues — same throughput, monotonic p99 explosion.

---

## Scaling roadmap and identified bottlenecks

What stops `gamereel` from going faster on the same machine, in priority order:

### Tier 1 — Software wins, single GPU (M4 territory)

| # | Bottleneck | Fix | Predicted impact |
|---|---|---|---|
| 1 | CPU `Scene::on_render` (~40 % of frame) | M4 wgpu compositor (eliminate image_effect.rs) | +50–80 % single-stream e2e |
| 2 | Per-frame `synchronize()` in CUDA path | Stream-aware NVENC submit (cudarc + ffmpeg share CUstream) | +10–15 % single-stream |
| 3 | `cuMemcpy2D` from cudarc-owned to ffmpeg pool buffer | Direct kernel writes to pool device pointer (cudarc 0.20+ API) | +1–2 % (cosmetic) |
| 4 | `to_rgba8()` per frame | Keep buffer as `Vec<u8>` from the compositor onward | +5 % |

### Tier 2 — Hardware ceiling (NVENC engine count)

The RTX 3060 has **1 NVENC engine** = our 1113 fps ceiling. Software cannot move this. Upgrade options:

| GPU | NVENC engines | Worker peak (estimated) | Notes |
|---|---:|---:|---|
| RTX 4070 Ti Super 16GB | 2 | ~4 | ~$800; 2× NVENC + 16GB VRAM (good for AI side-use) |
| RTX 4080 Super 16GB | 2 | ~4 | ~$1000; same NVENC count, faster cores, better AI |
| RTX 4090 24GB | 2 | ~4 | ~$1700; 24GB unlocks 30B LLM Q4 + same NVENC |
| RTX 5090 32GB | 3 | ~8 | ~$2000+; new Blackwell, 3rd NVENC engine, 32GB |
| **NVIDIA L4 24GB** | **4** | **~12** | ~$2000 server channel; 72 W single-slot; designed for transcoding farms |
| L40S 48GB | 3 | ~10 | ~$8000; datacenter-class; AI + video dual purpose |

Predicted scaling at 4× NVENC (L4): ~4400 fps (4× our current peak), 100 videos in ~7 s.

### Tier 3 — Multi-GPU

`Worker` trait already abstracts dispatch. Adding `MultiGpuWorkerPool` that round-robins across N local GPUs is mechanical. Linear scaling expected up to ~4 GPUs (PCIe and host RAM bandwidth become the next ceiling).

### Tier 4 — Cloud GPU (RemoteWorker via gRPC)

The `RemoteWorker` stub is shipped today. Filling it in:
1. gamereel-farm-server binary on the cloud node (reuses `LocalWorker` internally).
2. `RemoteWorker` impl in `crates/gamereel-farm/src/worker/remote.rs` issuing gRPC `Render(RenderJob) → RenderResult` calls.
3. Output transport: stream MP4 bytes back, OR have the cloud node upload to caller-supplied object storage URL (preferred for production — avoids round-tripping a video back to dispatcher).

Bottleneck shifts to: network bandwidth between dispatcher and cloud nodes. For 720×1080 H.264 at our default bitrates, output is ~60 KB / 10 s clip — rounding error.

### Tier 5 — Format premium tiers (HDR + AV1)

Hardware-gated:
- **HDR (HEVC Main10 + BT.2020)** requires Ada (RTX 40) or newer for clean HDR HW encode.
- **AV1 encode** requires Ada or newer (NVENC Gen 8+).

Both unlock TikTok / Instagram premium upload tiers — visible quality lift on platforms that re-compress aggressively.

---

## Compositor decision matrix: CPU vs wgpu

The wgpu compositor is **opt-in only** via `GAMEREEL_WORKER_COMPOSITOR=wgpu`. Default stays CPU because hs-mvp's dirty cache makes CPU compose nearly free on sparse-update scenes.

**The break-even rule (measured on RTX 3060)**:
- CPU image_effect cost ≈ 1 ns/pixel × pixels-touched-per-frame.
- wgpu compose+readback cost ≈ 0.5 ms/frame, constant.
- Cross-over at **~500 K pixels touched per frame** (with cache hits factored out). Below that, CPU wins. Above that, wgpu wins by 1.5–100×.
- Reference data: [`crates/gamereel-compositor/tests/wgpu_break_even_sweep.rs`](crates/gamereel-compositor/tests/wgpu_break_even_sweep.rs) shows wgpu winning **37–93× even at N=1 full-screen overlay** when CPU has no cache help.

### Stay on CPU (default)

| Scene class | Why | Example games |
|---|---|---|
| Sparse UI with small static background | dirty cache returns 70–90 % cached frames at near-zero cost | hs-mvp data card, idle-game leaderboard |
| Match-3 / puzzle replay (small grid cells) | per-cell sprites are 32–96 px; 64 cells × 4 KB = 250 K px / frame, half cache-hits | 方块游戏战报, candy-crush-style replays |
| Card animation (1 bg + ≤ 10 cards) | only the cards are dirty; bg stays cached | hs-mvp, MTG / Hearthstone clone replays |
| 1-shot result / score screen with rolling text | text is the only animation; bg cached | weekly summary, leaderboard intro |
| Talking-head replay overlays (face cam + chat scroll) | small overlay regions, big cached bg | streaming-style replays |

### Opt into wgpu (`GAMEREEL_WORKER_COMPOSITOR=wgpu`)

| Scene class | Why | Example games |
|---|---|---|
| Battle replay with full-screen VFX (explosions, screen shake, color flash) | every frame touches the full canvas, no cache hits | RPG / MOBA battle highlights, tower defense waves |
| Cinematic intro with parallax full-screen layers | 3–5 full-screen layers all moving every frame | 二次元 ARPG opening, season trailer |
| Dynamic background that re-renders per frame | `_clear_image` cache invalidates every frame | live-tiling background, moving sky / weather |
| Particle-heavy effects (50+ particles spanning the frame) | particles span the whole canvas | shoot-em-up, bullet-hell replay |
| Continuous full-screen filter (blur, color grade, bloom) | filter touches every pixel every frame | dream-sequence / flashback transitions |
| Game-board zoom or rotate transition | transform touches the entire canvas | board-game zoom-out, MOBA full-map sweep |

### Heuristic for unsure cases

```
expected_pixels_per_frame =
   sum(sprite_w * sprite_h * (1 if sprite_changes_every_frame else 0))

if expected_pixels_per_frame > 500_000:
    GAMEREEL_WORKER_COMPOSITOR=wgpu
else:
    leave default (CPU)
```

For 720×1080 frames, 500 K pixels ≈ **3 sprites of 400×400 all moving each frame**, OR **1 full-screen background that re-renders**. If a single full-screen layer is in play every frame, you're past break-even — switch.

---

## Workspace layout

```
gamereel/
├── Cargo.toml                         # workspace root
└── crates/
    ├── gamereel-core/                 # video generation engine + ProtocolParser trait + perf_main bin
    ├── gamereel-compositor/           # wgpu compositor (M4) — opt-in heavy-scene path
    ├── gamereel-farm/                 # worker pool, hardware probe, Worker trait
    ├── gamereel-output/               # OutputSink trait + LocalDiskSink + ObjectStorageSink
    ├── proto-puzzle/                  # 方块游戏 v0 JSON parser + Scene translator
    └── proto-bubble/                  # 泡泡龙 protocol parser (skeleton)
```

## Output delivery (S3-compatible, env-driven)

`gamereel-output::ObjectStorageSink` uploads rendered MP4s to any S3-compatible endpoint via env vars — same binary works on AWS, Aliyun OSS, Cloudflare R2, MinIO, GCP-via-S3:

```bash
export GAMEREEL_S3_REGION=cn-hangzhou
export GAMEREEL_S3_BUCKET=gamereel-replays
export GAMEREEL_S3_ACCESS_KEY_ID=LTAI…
export GAMEREEL_S3_SECRET_ACCESS_KEY=…
# Optional:
export GAMEREEL_S3_ENDPOINT=https://oss-cn-hangzhou.aliyuncs.com   # omit for AWS S3
export GAMEREEL_S3_PREFIX=prod/match3/
export GAMEREEL_S3_PUBLIC_URL_BASE=https://cdn.example.com/        # CDN URL for end-user share
export GAMEREEL_S3_PATH_STYLE=1                                    # set for MinIO/local
```

The receipt's `location` field is the public URL the player shares. CompositeSink fans out to multiple sinks in parallel for "upload + push notification" patterns.

## Build

```bash
cargo build --workspace --release       # all crates
cargo test  --workspace                 # 30 active + 10 CUDA-gated
cargo run   -p gamereel-core --bin perf_main --release
```

**Requires:** ffmpeg dev libraries (`libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev`), `clang`, `pkg-config`. For the CUDA pipeline: NVIDIA driver ≥ 535, `libnvrtc12`, `libnvrtc-builtins12.0`.

## Adding a new game protocol

1. `cp -r crates/proto-puzzle crates/proto-<gamename>` and rename in `Cargo.toml`.
2. Implement `ProtocolParser` for your type, register with `inventory::submit!`.
3. Add `proto-<gamename>` as a dependency in your consumer crate + `use proto_<gamename> as _;` (force-link so `inventory` constructors survive `lto = "fat"`).

## Forensic discipline

Every perf change is documented with *hypothesis*, *self-proof test*, *measured delta*, *retro* in commit messages. Read git log when revisiting a decision months from now.

## License

See LICENSE file.
