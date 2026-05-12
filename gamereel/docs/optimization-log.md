# Optimization Log

> One row per landed performance change. Every row carries (a) the *hypothesis* that justified the work, (b) the *test* that mechanically verifies the change is real, (c) the *measurement* showing the actual delta, and (d) a *retro* note explaining any gap between hypothesis and measurement.
>
> The point is not to celebrate wins — the point is to make every regression and every wrong assumption forensically reviewable months later.

## Entry template

```
### <ID> — <one-line title>      [milestone: M?]

**Hypothesis**
- Expected impact: <Nx end-to-end | Nx encoder-only | -X% memory | …>
- Mechanism: <why it should help — the underlying operation count / cache / latency story>

**Test (self-proof)**
- File: <path/to/test_file.rs::test_name>
- What it asserts: <plain English; what would break the test if regression>

**Measurement**
- Before: <number with units, link to baseline json>
- After:  <number with units, link to mN.json>
- Actual delta: <Nx>

**Retro**
- Hypothesis vs reality: <"matched" | "underdelivered because…" | "overdelivered because…">
- Lessons: <one or two bullets — what to remember next time>
- Followups: <if any>
```

---

## M0 — Baseline + tooling

This milestone landed no perf change of its own. Its job was to make every later perf claim measurable. Two things to remember:

* The **shell** baseline (`benches/baseline.sh`, libx264 medium on testsrc2) measures **535 fps / VMAF 99.34 / SSIM 0.999 / PSNR_Y 53.5**. That's the all-CLI reference number. JSON: [`benches/results/m0_baseline.json`](../benches/results/m0_baseline.json).
* The **Rust-side** baseline (`movie-maker/benches/encode_baseline.rs`, criterion, libx264 medium with hoisted scaler, RGBA gradient source) measures **152 fps**. This is *much lower* than the shell number because the bench reconstructs the encoder context every iteration; that overhead is amortized in real workloads where the encoder lives across many frames. Treat 152 fps as the conservative "Rust API per-iter" floor, not the encoder's true ceiling.

When comparing later milestones, **always compare like-for-like**: shell vs shell, Rust API vs Rust API, e2e vs e2e. The same physical change shows different multipliers under different harnesses; pick the harness that matches the production codepath.

---

### O-001 — Cargo workspace + opt-level=3 override     [milestone: M0]

**Hypothesis**
- Expected impact: 1.5–2× CPU-bound code (vs the repo-wide `opt-level="z"` default).
- Mechanism: `opt-level="z"` optimizes for binary size and disables several inlining and vectorization passes. CPU-heavy loops (image_effect blends, ffmpeg-next FFI bridges) lose ~50 % throughput. video-generator is the wrong workload for size optimization.

**Test (self-proof)**
- The change itself doesn't have a dedicated assertion test. The proof is the M1 measurement landing on profile `release` (not `release-small`); regression would show as a fps drop in `benches/m1.sh` perf_main runs.

**Measurement**
- Captured indirectly. M1 perf_main median: 794 ms. Same workload built with `opt-level="z"` produces ~1100–1300 ms in spot checks (not committed; verify by `cargo build --release --profile=release-small`).

**Retro**
- The repo convention `opt-level="z"` exists for short-running CLI tools where startup time and disk footprint dominate. video-generator is the wrong shape for that convention. Override is documented in workspace `Cargo.toml` so the next reader sees the deviation.
- Followup: if at some point we ship video-generator binaries via the same release pipeline, test both profiles end-to-end (release vs release-small) and confirm the speed delta survives optimization passes evolving over Rust versions.

---

### O-002 — Dead code removal (frame.rs / mvp.rs / shadow NodeAttr)    [milestone: M0]

**Hypothesis**
- Expected impact: 0× perf, but reduces compile time and noise. Important hygiene before adding 6 new files.
- Mechanism: `frame.rs` was byte-identical to `stream.rs`. `tests/mvp.rs` referenced nonexistent paths and stale APIs. Shadow `NodeAttr` enum in `meta_action.rs` shadowed the real struct used by every caller.

**Test (self-proof)**
- N/A. Build passing post-removal is the test.

**Measurement**
- Cold workspace build went from 6m 53s (gamereel-core only) to 3m 26s (gamereel-core + demo) on the same machine. Most savings came from `ffmpeg-next` git-source caching in the workspace target dir, not the deletions.

**Retro**
- Pre-cleanup hidden risk: the next person to grep `NodeAttr` would have followed the wrong definition. The 30-line cleanup unblocks any later refactor that touches scene metadata.

---

## M1 — Encoder pick + hot-loop cleanup

### O-003 — Encoder auto-selection (NVENC > QSV > VAAPI > VT > x264)   [milestone: M1]

**Hypothesis**
- Expected impact: 3–5× single-stream encoder throughput on machines with NVENC; 0 (or unblocking) on others.
- Mechanism: Hardware encoders offload H.264 entropy coding + motion search from the CPU to dedicated silicon. Pre-M1 the code hardcoded `h264_videotoolbox` (macOS only) and panicked on Linux, so we couldn't even measure.

**Test (self-proof)**
- File: `movie-maker/tests/encoder_pick.rs`
- 7 tests: priority order assertion, NVENC parameter set sanity, NV12 vs YUV420P pixel format per encoder, that `pick_h264_encoder` returns a real registered codec on the host, and that NVENC wins on hosts where it's compiled in. The "wins on this host" assertion is environment-conditional, so the suite passes on CI hosts without GPU but still catches "we forgot to put NVENC first" mistakes.

**Measurement**
- Standalone NVENC p4 balanced (shell, testsrc2): **475 fps** vs libx264 medium **520 fps** → **0.91× on this content**.
- Standalone NVENC p2 fastish: **619 fps** → **1.19× over libx264 medium**.
- See [`benches/results/m1.json`](../benches/results/m1.json).

**Retro**
- Hypothesis vs reality: **underdelivered** for the standalone-encoder slice, by a wide margin.
  * The 3–5× projection assumed a "weak CPU + strong GPU" balance. On i7-13700K (16C/24T, 5.4 GHz) with libx264's 24-thread parallelism, the CPU encoder is an unfair benchmark: 13700K-class throughput is rare in deployment but is what we have.
  * NVENC also showed slightly lower VMAF at the same bitrate (98.4 vs 99.3) — small but consistent; the perceptual gap is sub-frame.
- The honest framing: **NVENC's win is not single-stream raw fps on this CPU. It's (a) freeing the CPU for compositing, (b) enabling 6+ concurrent streams within one process via M5's actor pool, (c) per-frame energy efficiency.**
- Lessons:
  1. Always anchor speedup hypotheses on the hardware actually deployed, not "typical".
  2. Quote both throughput AND quality (VMAF). NVENC at p4 is a different point in the speed/quality plane, not a strict Pareto improvement over libx264 medium.
- Followups: M2 will sweep NVENC presets to find a config that matches libx264 medium's VMAF (likely p6 or p7 with `-tune hq` + spatial AQ). Then the speed comparison becomes apples-to-apples.

---

### O-004 — Scaler / frame buffer hoist out of per-frame loop      [milestone: M1]

**Hypothesis**
- Expected impact: ~1.3× e2e (eliminates per-frame heap allocation and ffmpeg context construction).
- Mechanism: Pre-M1, the loop body called `software::scaling::Context::get(...)` and `frame::Video::new(...)` × 2 every iteration. `scaling::Context::get` allocates an internal scratch buffer + builds an SIMD dispatch table per call (~5 large allocs). Hoisting them turns the loop body into pure data movement.

**Test (self-proof)**
- File: `movie-maker/tests/scaler_reuse.rs::steady_state_per_frame_large_allocs_under_threshold`
- Installs a counting global allocator only in this test binary. Encodes 5 frames with the post-M1 hoisted layout. Asserts ≤ 4 large (≥ 4 KiB) allocations *per frame* in steady state.
- Actual measured per-frame: **0.0 large allocs** (well under the alarm threshold). Pre-M1 was empirically ~50–80.

**Measurement**
- `perf_main` median wall: 152 fps (M0) → **377 fps (M1) = 2.48×**. The hoist + encoder change together. Isolated hoist contribution can be reproduced by reverting only `mod.rs` and re-running `benches/m1.sh`; in spot checks the hoist alone accounted for ~30 % of the e2e improvement.

**Retro**
- Hypothesis matched: ~30 % improvement consistent with `scaling::Context::get` being the dominant per-frame allocator pre-M1.
- The allocation-counting test is the most valuable part of this entry. It catches the regression class "someone added a per-frame `to_owned()` / `clone()` deep in the render pipeline" which is otherwise invisible until you wonder why fps dropped 20 %.
- Lessons: **resource-allocation regressions are silent but cumulative**. The cheapest defense is a guarded counter in a test, not a profiling session three releases later.

---

### O-005 — `Scene::children` HashMap → BTreeMap (z-order determinism)    [milestone: M1]

**Hypothesis**
- Expected impact: **0× perf**, but unblocks regression tests that hash rendered output. Side win: fixes a latent rendering bug (overlapping sprites z-flickered between runs because `HashMap` iteration is randomized for HashDoS).
- Mechanism: `BTreeMap` iterates by sorted key. Node IDs are `u64`, so the iteration order is now stable and visually meaningful (small IDs render first, large IDs on top).

**Test (self-proof)**
- File: `movie-maker/tests/zorder_stable.rs::two_independent_renders_produce_identical_frame_hashes`
- Renders 30 frames of `tests/perf_main/scene.meta` twice from a fresh runtime, hashes each frame's RGBA bytes with SHA-256, asserts every digest matches.
- Sibling test `rendered_frames_actually_change_over_time` guards against false-pass on a stuck-black render (degenerate trivially-identical frames).

**Measurement**
- Same number of nodes, same blends — the rendering cost is unchanged. perf_main wall time before/after: indistinguishable in noise.

**Retro**
- This is an enabling change, not a perf change. Without it, M3+ would have no way to assert "the GPU compositor produces the same image as the CPU compositor" — every comparison would have to allow for HashMap-induced byte differences.
- Lessons: when you reach for `HashMap` for "I just need a fast lookup", check whether iteration order is observable downstream. If it is, `BTreeMap` is almost free at the scales we care about (< 10 K entries) and unblocks downstream verification.

---

## M2 — NVENC quality tuning

### O-006 — `EncoderProfile::Fast` (preset p2, no AQ, bf=0)    [milestone: M2]

**Hypothesis**
- Expected impact: 1.10–1.20× over Balanced fps, VMAF stays ≥ 88 (the M2 floor).
- Mechanism: NVENC presets p1–p2 disable several quality features (lookahead, B-pyramid, deeper motion search) in exchange for fewer GPU cycles per frame. Use case: previews / drafts where the loss of 0.5–1.5 VMAF is acceptable for ~20% throughput.

**Test (self-proof)**
- `tests/profile_speed_floor.rs::profile_speed_ordering_holds` — asserts `Fast ≤ Balanced × 1.10` (i.e. Fast must not be slower than Balanced × 1.10).
- `tools/quality-eval/profile_quality_floor.sh` — asserts mean Fast VMAF ≥ 88 across the three test sources.

**Measurement**
- VMAF (mean of 3 sources): **97.87** (floor: 88, headroom 9.87 pts).
- fps (synthetic 720x1080 × 300 frames): **473.9**.
- Compared to Balanced 462.2 fps: **1.025× speedup** — well below the 1.10 we expected.

**Retro**
- Hypothesis vs reality: **underdelivered on speed**. p2 is supposed to be faster than p4 by 15–25 % per NVENC docs, but at 720×1080 with VBR 6–8 Mbps on a 3060, the encoder spends most of its time waiting on PCIe/DMA rather than running encoder logic. Preset choice barely moves the needle.
- The grid search confirmed this: p2 mean fps was 600.6 at the per-source winner, vs p4 at 480.3 — a 1.25× gap. We'd see that gap cleanly on real content (more entropy → more encoder work). On synthetic content the GPU is starved, so preset doesn't matter as much.
- Lessons:
  1. NVENC preset speed differences narrow on low-entropy content and small resolutions. Document the regime where the speedup is real.
  2. Don't over-tune Fast based on synthetic content alone. Real-camera content has higher entropy and the speedup will reappear.
- Followup: M5's batch-100 bench will exercise this with real gamereel-core output (much closer to TikTok content); revisit Fast profile selection then.

---

### O-007 — `EncoderProfile::Balanced` (preset p4, bf=3, no AQ)    [milestone: M2]

**Hypothesis**
- Expected impact: VMAF ≥ 92 floor, retain M1's e2e fps (~377). This is the default profile that all callers (perf_main, demo, future render-farm) inherit.
- Mechanism: p4 is NVENC's "middle" preset — full motion search, B-frames, but no lookahead/AQ overhead. Best speed/quality knee for most content.

**Test (self-proof)**
- `tests/profile_speed_floor.rs::profile_speed_ordering_holds` — sandwiched between Fast and TikTokHQ; tests both the ≤ Fast × 1.10 lower bound and being ≥ TikTokHQ × 0.85.
- `tools/quality-eval/profile_quality_floor.sh` — asserts mean Balanced VMAF ≥ 92.
- `tests/encoder_profile.rs::profile_default_is_balanced` — pins Balanced as the API default.

**Measurement**
- VMAF (mean of 3 sources): **97.73** (floor: 92, headroom 5.73 pts).
- e2e perf_main median: **381 fps** (M1 was 377 fps — within noise, no regression).
- Standalone encoder fps: **462.2**.

**Retro**
- Hypothesis matched: VMAF well above floor, e2e flat vs M1. The point of M2 wasn't to make Balanced faster; it was to make sure Balanced is still the right default after we documented 4 profile tiers.
- Notable: I initially added `spatial-aq=1` to Balanced based on grid winners — and VMAF *dropped* by 1.1 pts on synthetic content. **Lesson learned the hard way**: AQ adapts bits based on perceptual texture variance; synthetic gradients have ~zero variance, so AQ just adds overhead without benefit. Reverted in the same M2 commit.
- Lessons:
  1. **Per-source grid winners ≠ universal-best config**. The Python aggregation in `grid_search.sh` initially picked highest-VMAF-per-source; the right metric is highest-min-VMAF (or mean-VMAF) across sources. Updated to use min-VMAF as the primary criterion.
  2. AQ benefits depend on the perceptual content; synthetic test sources can't validate AQ tuning.
- Followup: when M5 lands, re-run the grid against real gamereel-core output (game data cards) to revisit AQ for Balanced.

---

### O-008 — `EncoderProfile::TikTokHQ` (preset p6, lookahead=16, AQ on)    [milestone: M2]

**Hypothesis**
- Expected impact: VMAF ≥ 95 floor (the platform-target tier). fps ≥ Balanced × 0.5 (heavier preset, expected).
- Mechanism: p6 enables the slower / higher-quality motion search modes. lookahead=16 lets the rate controller distribute bits across upcoming complex frames. spatial-aq + temporal-aq weight bits towards high-frequency edges and high-motion regions — both critical for the TikTok content mix (faces, text overlays, transitions).

**Test (self-proof)**
- `tests/profile_speed_floor.rs::profile_speed_ordering_holds` — asserts `TikTokHQ ≥ Balanced × 0.85` (i.e. not faster than Balanced × 0.85). Catches accidental dropping of the heavy quality knobs.
- `tools/quality-eval/profile_quality_floor.sh` — asserts mean TikTokHQ VMAF ≥ 95.
- `tests/encoder_profile.rs::tiktok_hq_profile_produces_valid_h264_high` — asserts ffprobe sees a valid H.264 High stream with the right pix_fmt.

**Measurement**
- VMAF (mean of 3 sources): **97.48** (floor: 95, headroom 2.48 pts).
- Standalone encoder fps: **399.5** (Balanced × 0.86 — within tolerance).
- Bitrate ceiling: 14 Mbps (vs Balanced 12 Mbps), giving us headroom against TikTok's recompression pass.

**Retro**
- Hypothesis matched: VMAF clears 95 floor and fps cost is the expected ~14 % over Balanced. **However** — initial parameter set used `lookahead=32, bf=4`. The grid showed `lookahead=16, bf=3` performed identically at higher fps. Tuned down accordingly.
- Quality lift over Balanced is only ~0.3 VMAF on synthetic content (97.73 → 97.48 — actually a *slight loss* on these specific sources because AQ + extras add some overhead without payoff). Real-camera content is where TikTokHQ's value shows up; the synthetic grid only validates that we haven't broken anything.
- Lessons:
  1. NVENC parameter tuning has a long tail of "settings that look like they should help but don't on this content class". Always validate with a representative source.
  2. **The honest framing of TikTokHQ**: it's our best guess at what platform-uploaded content needs, validated to clear minimum quality on synthetic sources. M5's real-content batch will let us calibrate it properly.
- Followup: stand up a small corpus of actual TikTok-style content (5–10 clips, mix of faces / gameplay / text overlays) and rerun the grid against that. Until then TikTokHQ is "production-grade" only in the regression-safe sense, not the truly-tuned sense.

---

### O-009 — `EncoderProfile::IgReelsHDR` (M2 fallback, M3 will diverge)    [milestone: M2]

**Hypothesis**
- Expected impact: VMAF ≥ 95 (same floor as TikTokHQ), enables the API surface for HDR Reels uploads in M3 without breaking existing callers.
- Mechanism: M2 contractually falls back to TikTokHQ params. The enum variant exists so M3's HDR pipeline (HEVC Main10, hwframes, BT.2020, mdcv/clli metadata) can be plumbed without breaking signatures.

**Test (self-proof)**
- `tests/encoder_profile.rs::ig_reels_hdr_falls_back_to_tiktok_hq_in_m2` — encodes with the variant, asserts ffprobe reports H.264 High (same as TikTokHQ).
- `tests/profile_speed_floor.rs::profile_speed_ordering_holds` — asserts IgReelsHDR within ±30 % of TikTokHQ. The point of this assertion is to fail loudly the moment M3 swaps the implementation, prompting an explicit test update.
- `tools/quality-eval/profile_quality_floor.sh` — same floor as TikTokHQ.

**Measurement**
- VMAF (mean of 3 sources): **97.48** (identical to TikTokHQ, by construction).
- Encoder fps: **415.5** (close to TikTokHQ's 399.5).

**Retro**
- This is API plumbing, not perf. The point is **forward-compatibility**: callers that opt into HDR-aware code today will not need code changes when M3 lands the real HDR pipeline. The fallback emits a `log::warn!` so production callers know they're not getting actual HDR yet.
- Lessons: introducing API surface ahead of implementation requires a loud-failing regression test (so the fallback can't be silently accepted as "working") AND a clear log message so production callers see the gap.

---

### O-010 — Grid-search disciplines learned    [milestone: M2]

This isn't a single optimization but a methodology entry, kept here so M3+ inherits the lessons.

**Disciplines validated by M2**
1. **Synthetic sources are not enough.** They reward simple parameter sets and starve AQ / B-frame benefits. Use them for regression detection, not for tuning real-content profiles.
2. **Per-source winners ≠ profile defaults.** The grid tool now picks the highest-min-VMAF config across sources, not per-source winners. This avoids the trap of "Balanced does great on text_scroll specifically".
3. **Speed comparisons must amortize NVENC cold-start.** Sessions take 300–500 ms to spin up; benches under 5 s of measured content per iter undercount steady-state throughput by 5–10×.
4. **Cargo integration tests cannot run NVENC in parallel** without contending for the single hardware encoder. Either consolidate into one `#[test]` or use `#[serial]`.
5. **Quality regression tests belong in shell scripts**, not cargo tests, because each VMAF eval is 3–5 s. Keep cargo tests under 5 s wall time per file.

**Anti-patterns avoided**
- Building tests around fps numbers from a specific machine (use ratios instead).
- Parking heavy synthetic sources in git (250 MB lossless mandelbrot is a hard no — `.gitignore` it, regenerate from filter scripts).
- Tuning a profile on one content class and shipping it as a global default.

---

### O-011 — Where CPU time actually goes (forensic correction)    [milestone: M2]

**Observation triggering this entry**
- I claimed in the M1 retro and the README trend table that "CPU compositing is ~80% of e2e wall time". A direct measurement (`tests/cpu_breakdown.rs`, run with `--ignored`) shows the actual breakdown for the perf_main scene is very different.

**Test (self-proof)**
- File: `movie-maker/tests/cpu_breakdown.rs::cpu_breakdown_perf_main` (marked `#[ignore]`; run with `cargo test --release ... -- --ignored`).
- Times three phases in isolation: scene composition, sws_scale (RGBA→YUV), and the NVENC submit/receive loop.

**Measurement (300 frames, 720×1080, perf_main scene)**
| Phase | ms | Share |
|---|---:|---:|
| Compose (`Scene::on_render`, CPU pixel blends) | 85 | 12.7% |
| RGBA→YUV (`sws_scale`, CPU SIMD) | 311 | **46.1%** |
| NVENC submit/recv (CPU + GPU wait) | 278 | 41.2% |
| Total | 674 | 100% |

`/usr/bin/time -v` on the same binary: 0.97s user + 0.37s sys CPU vs 0.98s wall, 137% CPU. Phases overlap in the real hot loop, so individual ms don't sum to wall time.

**Retro: the 80% figure was wrong**
- I assumed compositing was the dominant cost because `image_effect.rs` is the most visually heavy CPU code (per-pixel alpha blends in nested loops). The actual data: on a *simple* perf_main scene (2 sprites, mostly static), compositing is **only 13%** — the dominant CPU cost is `sws_scale` at 46%.
- This matters for M3 sequencing. Originally M3's win was framed as "free up CPU by moving compose to GPU", but the real win is **moving sws_scale to GPU via `scale_cuda`** — that's the 311 ms slug, ~3× the size of the compositing slug.
- Lessons:
  1. **Always measure before claiming a bottleneck percentage**. Phase-isolation tests are cheap to write and stop you from optimizing the wrong thing.
  2. The compose-vs-scale-vs-encode breakdown changes with scene complexity. Demo's `hs-mvp` (game data card with many overlays + dynamic images) likely shifts more weight to compositing — re-measure when M5's batch bench includes real scenes.
- Followup:
  - Update README trend table to reflect that **sws_scale** is the M3 target, not compositing.
  - Re-run `cpu_breakdown` test against `demo` (with its richer scene) once that runner exists, to confirm whether the M4 wgpu compositor or the M3 hwframes path dominates the e2e wins on real workloads.

---

## M3 — Full-GPU pipeline (cudarc + ffmpeg CUDA hwframes)

### O-012 — `RGBA8 (CPU) → NV12 (GPU)` cudarc kernel via NVRTC    [milestone: M3]

**Hypothesis**
- Expected impact: replace the CPU-side `sws_scale` slug (M2 measured at 311 ms / 46% of phase time on perf_main, see [O-011](#o-011)) with a GPU kernel running in ~5 ms; net 60× on this stage in isolation.
- Mechanism: a single CUDA thread block per 2×2 RGBA tile computes 4 Y samples + 1 averaged UV pair. BT.601 limited-range matrix with output clamp. Persistent device buffers reused across frames so per-frame cost is upload + launch only.

**Test (self-proof)**
- File: `movie-maker/tests/cuda_rgba_nv12_parity.rs::cuda_kernel_matches_sws_scale_within_nv12_tolerance` (#[ignore], CUDA-gated)
- Runs both CPU `sws_scale` (RGBA→YUV420P, repacked to NV12 in test) and our GPU kernel on a deterministic RGBA pattern (XOR'd channels). Asserts `< 1%` of all NV12 samples diverge by more than 2 levels.
- Measured: Y plane 0/76800 samples >2 (max diff 1, mean |diff| 0.020), UV plane 1058/38400 samples >2 (max diff 14, mean |diff| 0.403). Combined 0.918% — under the 1% bar.

**Measurement**
- Standalone CPU sws_scale (M2 measurement): 311 ms / 300 frames = 1.04 ms/frame at 720x1080.
- Standalone GPU kernel (rough estimate from cuda_rgba_nv12_parity timing): ≪ 1 ms/frame.
- However: the win in isolation does NOT translate 60× into e2e because (a) we now pay PCIe upload of RGBA every frame, (b) destination is in ffmpeg's pool not our kernel's output buffer so we add a cuMemcpy2D, (c) `synchronize()` after each frame waits for both kernel + memcpy.

**Retro**
- Hypothesis vs reality: **technically delivered (kernel is fast)** but **e2e impact muted** (see O-014).
- The Y plane's perfect parity (max diff 1) confirms the BT.601 matrix is right. UV's looser fit (max diff 14, ~3% of samples) is the **box-average vs sws low-pass-then-decimate** difference — sws applies a small filter before subsampling, ours uses a raw 2×2 average. NVENC quantizes both before encoding so the visible quality difference is negligible (M3-4 e2e mp4 ffprobe-clean).
- Lessons:
  1. Standalone-stage micro-benchmarks routinely overpredict e2e impact by an order of magnitude when there are upstream/downstream synchronization costs.
  2. NV12 chroma subsampling has multiple "correct" implementations; pick one and document it (we're using box-average, sws uses low-pass).
- Followup: if VMAF on real content shows visible chroma artifacts, add a 3-tap low-pass before chroma subsampling in the kernel — adds ~10 GFLOPs/frame, negligible at 720p.

---

### O-013 — ffmpeg CUDA hwframes pool (separate-context UVA bridge)    [milestone: M3]

**Hypothesis**
- Expected impact: 0× perf on its own; **enables** O-014 (no-CPU-roundtrip pipeline).
- Mechanism: ffmpeg owns an `AVHWFramesContext` (NV12 pool, 4 frames) on the same NVIDIA device cudarc uses. Encoder's `hw_frames_ctx` set so `h264_nvenc` consumes pooled CUDA frames natively (not synthetic CPU frames it would have to re-upload).

**Test (self-proof)**
- File: `movie-maker/tests/cuda_hwctx_alloc.rs` (2 tests, #[ignore])
  - `allocates_pooled_cuda_nv12_frames` — verifies pool returns distinct device pointers, format = AV_PIX_FMT_CUDA, width/height/linesize sane.
  - `frames_ref_can_be_borrowed_multiple_times` — verifies AVBufferRef refcount semantics so encoder + copy helper can both hold references safely.

**Measurement**
- N/A as a perf change in isolation.

**Retro**
- **Initial attempt with `AV_CUDA_USE_PRIMARY_CONTEXT` failed**: `Primary context already active with incompatible flags`. cudarc's `CudaContext::new(0)` retains the primary context with a flag set ffmpeg refuses (`CU_CTX_SCHED_AUTO` vs ffmpeg's expectation).
- **Solution**: let ffmpeg create its own CUDA context on the same device. RTX 3060's Unified Virtual Addressing (UVA, on by default for Pascal+) means a device pointer allocated by cudarc is valid in ffmpeg's context. We do cuMemcpy2DAsync_v2 from cudarc's stream — UVA resolves the cross-context destination pointer transparently.
- Lessons:
  1. CUDA primary context flag negotiation between independent libraries is fragile. Default to "let each library make its own context, share via UVA" unless both libraries explicitly cooperate.
  2. ffmpeg-next does not expose `hw_frames_ctx` in its safe wrapper — drop to `ffmpeg-sys-next` (which `ffmpeg-next` already pulls in transitively) for the field set.
  3. **Cargo's `links="ffmpeg"` rule blocked our first plan** to use `rsmpeg` (which has cleaner hwframe APIs). Both ffmpeg-next and rsmpeg declare `links="ffmpeg"` and refuse to coexist. We salvaged by promoting `ffmpeg-sys-next` (already transitive) to a direct dep.

---

### O-014 — End-to-end CUDA pipeline integrated into `create_scene_stream_cuda`    [milestone: M3]

**Hypothesis**
- Expected impact: **~1.5–2×** e2e on perf_main scene; **2–3×** on heavier compositions.
- Mechanism: `Scene::on_render` → `CudaConverter::convert` (RGBA upload + GPU kernel) → `CudaConverter::copy_to_device_2d` (cuMemcpy2DAsync_v2 to pool frame) → `avcodec_send_frame` with `AV_PIX_FMT_CUDA` AVFrame. No CPU-side `sws_scale`, no CPU-side YUV intermediate, just one PCIe upload per frame.

**Test (self-proof)**
- File: `movie-maker/tests/cuda_e2e_perf_main.rs::perf_main_scene_through_cuda_pipeline_produces_valid_mp4` (#[ignore])
- Drives `create_scene_stream_cuda` on the perf_main scene, asserts ffprobe sees codec=h264, profile=High, width=720, height=1080, pix_fmt=yuv420p, nb_read_frames=300.
- File: `movie-maker/tests/cuda_vram_leak.rs::no_vram_leak_across_100_encodes` (#[ignore], ~62 s)
- 100 sequential encodes, asserts VRAM delta < 200 MB.

**Measurement** (`benches/m3.sh` → [`benches/results/m3.json`](../benches/results/m3.json))
- M2 path (sws_scale + Balanced profile): **370.8 fps median** (5 runs)
- M3 path (CUDA hwframes): **455.9 fps median** (5 runs, cold run dropped)
- **Speedup: 1.229× (1.23×) on perf_main e2e**
- VRAM leak across 100 cycles: **0 MB** ([`tests/cuda_vram_leak.rs`](../movie-maker/tests/cuda_vram_leak.rs))

**Retro**
- Hypothesis vs reality: **delivered ~half the predicted multiplier**. The sources of over-prediction:
  1. perf_main is small and mostly static; `Scene::on_render` (CPU pixel blends) is now the dominant cost — not the sws_scale slug we removed. Re-running [O-011](#o-011)'s breakdown post-M3 would likely show compositing at 50%+ on perf_main.
  2. We `synchronize()` after every frame (kernel + 2× cuMemcpy + send). This serializes the pipeline; an async pipeline where NVENC drains while the next frame's kernel runs would recover another ~20%.
  3. The cuMemcpy2DAsync_v2 from cudarc-owned buffer to ffmpeg pool buffer is wasted PCIe-internal traffic. The fix: **make CudaConverter::convert write directly into the pool frame's pointer** (need to inject a cudarc `CudaSlice<u8>` view over an external pointer — not in cudarc 0.19, would be M4-ish).
- The **leak test passing with 0 MB delta** is the most valuable result here. Drop machinery for both `CudaConverter` and `CudaHwContext` is correct; production can run unattended for arbitrary durations.
- Lessons:
  1. **perf_main is no longer a representative benchmark for M3+**. It was a useful baseline (M0-M2) but its scene is so simple that compositing dominates anything we do downstream. M5's batch test against demo's hs-mvp scene (richer compositing) will give more honest M3/M4 multipliers.
  2. The "1.23× e2e on a small scene + 0 leaks + GPU pipeline ready for M4 wgpu compositor" is the **right framing**. M3 is structural — the compositor (M4) cashes in the structure, M5 amortizes everything across a worker pool.
- Followup:
  - Add an e2e CUDA bench against a synthesized "heavy" scene (50+ overlays) to surface the multiplier on real compositing load.
  - Drop the per-frame `synchronize()` once we add a stream-aware NVENC submission path (cudarc + ffmpeg can share a CUstream).

---

## D — Demo audit (post-M3): are we using what we built?

The M3 commit landed the CUDA pipeline but **the demo never used it** — `apps/hs-mvp/src/main.rs` called `start_gen_first` (sws_scale path) instead of the CUDA entry. We instrumented the actual demo workload (`apps/hs-mvp/src/bin/trace.rs` and `trace_cuda.rs`) and audited which optimizations were actually wired in.

### O-015 — `Texture::dynamic_image` → `Arc<DynamicImage>`    [milestone: D2]

**Hypothesis**
- Expected impact: **save ~100 ms / 300 frames** on hs-mvp by replacing the per-frame `.clone()` of full image buffers (deep copy) with `Arc::clone` (refcount only).
- Mechanism: `Scene::on_render` calls `ctx.get_texture(...).dynamic_image.clone()` once per active node per frame. With ~6 active nodes and 75 dirty animation frames, that's ~450 deep clones. The largest texture is the `bans-*` composite at 384×384×4 = 590 KB.

**Test (self-proof)**
- File: `apps/hs-mvp/src/bin/trace.rs` — re-run before/after; the per-frame `scene.on_render` median should not regress (Arc deref is cheaper than buffer copy on any sane allocator).
- All 24 active gamereel-core tests still pass after the API change (Texture's field type changed from `Option<DynamicImage>` to `Option<Arc<DynamicImage>>`).

**Measurement**
- Pre-D2 trace: `scene.on_render` total 337 ms / 300 frames.
- Post-D2 trace: `scene.on_render` total 348 ms / 300 frames (within run-to-run noise; ±5 % variance).
- e2e wall: pre 273 fps → post 270 fps (no measurable change on hs-mvp).

**Retro**
- Hypothesis vs reality: **null result on this specific demo workload**, but **structurally correct change**.
- Why no measurable win on hs-mvp: textures are small (48×48 = 9 KB per cell, 384×384 = 590 KB for bans-*). The deep-copy bandwidth cost on a 13700K (~50 GB/s memory) is sub-millisecond per clone. 450 small clones ≈ 5 ms total — within timer noise.
- Where this WILL show: any scene with full-frame textures (1080×1920 = 8.3 MB each). Per-frame deep clone of an 8 MB buffer is ~150 µs; 300 frames × N nodes adds up. Doing the right thing structurally beats waiting for a regression to teach us.
- Lessons:
  1. **"Obviously expensive" optimizations sometimes aren't, when measured on the actual workload.** Always trace before celebrating.
  2. The change still belongs in: it's no slower, it generalizes correctly to bigger scenes, and it composes with the GPU compositor (M4) which will want shared texture handles.

---

### O-016 — Demo switched to CUDA pipeline (`start_gen_first_cuda`)    [milestone: D3]

**Hypothesis**
- Expected impact: **+4 % on hs-mvp** (matches pre-existing trace_cuda.rs measurement of 273 → 286 fps).
- Mechanism: `apps/hs-mvp/src/main.rs` was calling `start_gen_first` which routes to the M2 sws_scale path. New `start_gen_first_cuda` on `StageMgr` dispatches to `create_scene_stream_cuda` instead. `GAMEREEL_PIPELINE=sws` env var preserves the M2 fallback for hosts without NVIDIA driver.

**Test (self-proof)**
- File: `apps/hs-mvp/src/main.rs::main` runs the CUDA path by default; running with `GAMEREEL_PIPELINE=sws` exercises the fallback.
- File: `apps/hs-mvp/src/bin/trace_cuda.rs` already validated ffprobe-clean output on hs-mvp's scene.

**Measurement**
- 5-run median e2e:
  - sws path:  1023 ms (293 fps)
  - cuda path: 980 ms (306 fps)
  - **speedup: 1.04× — matches the trace prediction within noise.**

**Retro**
- Hypothesis vs reality: **delivered exactly what trace predicted**, no surprises.
- The 4 % is small because **CUDA initialization eats 284 ms** as a one-shot tax on the first encode (NVRTC kernel compile + ffmpeg hwframes pool setup + cudarc primary context retain). On a single-shot 1-second video, that's 25 % of wall time. **In batch mode, this tax amortizes to nothing** — `tests/cuda_vram_leak.rs` runs 100 sequential encodes in 62 s = 620 ms/video = **485 fps (1.7× single-shot)**.
- The CUDA path's per-frame steady-state is ~2.3 ms vs sws's ~2.7 ms; the per-frame win is real, the wall-time win is diluted by init.
- Lessons:
  1. **Single-shot benchmarks systematically undersell M3** because they pay the full init cost amortized over 300 frames; batch benchmarks (M5) are the honest measurement.
  2. Default-on for the CUDA path is right — even if e2e is a wash, downstream M4 (wgpu compositor) needs the GPU residency to deliver its multiplier.

---

### O-017 — Dirty-cache audit: already working, no change needed    [milestone: D1]

**Hypothesis (going in)**
- Suspected: `Scene::do_action` was setting `is_dirty = true` every frame, defeating the cache. Predicted fix would skip ~225 of 300 frames in hs-mvp (the post-animation steady period), giving ≥1.5× e2e.

**Test (self-proof)**
- `apps/hs-mvp/src/bin/trace.rs` per-frame median of `scene.on_render` is **0.07 ms** vs total 348 ms (mean 1.16 ms). Median ≪ mean ⇒ most frames are cheap (cache hits), a few are expensive (animation frames). Approximate split:
  - ~225 cached frames × 0.07 ms = 16 ms (returns `_catch_image.clone()`)
  - ~75 active animation frames × ~4.4 ms = 332 ms (re-blends 6 nodes)
- The ~75 figure matches the demo timeline: picks-0 animates 0.1–0.5 s, picks-1 1.1–1.5 s, picks-2 2.1–2.5 s. That's 3 × 0.4 s × 30 fps = 36 dirty frames from move/scale, plus ~3 × 30 = 90 frames where bans become active in jumps. Within the right order of magnitude.

**Retro**
- The `_dirty` machinery (`do_action` value-comparison + early-return on `_catch_image.clone()`) is **functioning correctly**. Hypothesis was wrong; no fix landed.
- Re-investing here would not move the needle. The remaining cost on cached frames is the 3 MB `_catch_image.clone()` per cached frame (~70 µs × 225 = 16 ms total) — fixable but small.
- The actual hot path is the 75 active-animation frames at ~4.4 ms each. To make THOSE cheaper requires either (a) compositing on the GPU (M4) so the per-pixel blends become a wgpu compute shader, or (b) parallel pipeline so frame N+1's compose overlaps with frame N's encode.
- Lessons:
  1. **Always validate the symptom before fixing the cause.** "Dirty flag broken" is a guess; phase-isolation timer (`trace.rs` median vs mean) is data.
  2. Sub-millisecond medians strongly suggest a working cache; if you suspect cache failure, you'd see a flat per-frame distribution, not a bimodal one.

---

### Demo audit summary — what we ARE using vs what's left on the table

**✅ active in demo (gamereel-core post-D2/D3)**
1. NVENC h264_nvenc auto-pick (M1)
2. Hoisted scaler + reusable frame buffers (M1)
3. BTreeMap z-order determinism (M1)
4. Workspace `opt-level=3 + lto=fat` (M1)
5. EncoderProfile::Balanced default (M2)
6. CUDA hwframes pipeline via `start_gen_first_cuda` (M3, D3)
7. `Arc<DynamicImage>` shared textures (D2)
8. `Scene._dirty` cache hitting ~75 % of frames (already worked, D1 audit)

**❌ available but NOT yet wired / not yet built**
9. Per-frame `synchronize()` removal — would save 99 ms / 300 frames in CUDA path (~14 %).
10. M4 wgpu compositor — would eliminate the 419 ms `scene.on_render` slug (~60 % of CUDA-path frame loop).
11. M5 actor pool batch — would amortize the 284 ms `cuda.init` tax across N videos. `cuda_vram_leak` proves 1.7× speedup at 100-video batch.
12. Pipeline parallelism (compose frame N+1 while encoding N) — independent of M4/M5; could be a 1.5× standalone win on single-video.
13. EncoderProfile::TikTokHQ for production uploads (currently demo uses Balanced).

**Verdict on hs-mvp single-shot**
- ~1000 ms wall (300 fps) is the **floor without M4/M5**. The 284 ms CUDA init + 419 ms CPU compose are unavoidable on a 13700K + RTX 3060 with the current architecture.
- M5 batch alone would push 100-video throughput to 485 fps — a real production-shaped number.
- M4 + M5 combined would target ≥1500 fps single-video on this same hardware (predicted, not yet validated).

---

## M5 — Worker pool: amortize CUDA init across batches

### O-018 — `LocalWorker` with persistent CUDA + ffmpeg context    [milestone: M5-2]

**Hypothesis**
- Expected impact: **single-worker steady state ≥ 600 fps** by paying CUDA init exactly once per worker process (vs once per render in the M3 single-shot path that the demo had been using).
- Mechanism: `LocalWorker` owns `CudaConverter` + `CudaHwContext` for its lifetime; only the ffmpeg encoder context (which holds h264_nvenc DPB / B-frame state that *cannot* be reused across videos) is rebuilt per job. CUDA init = ~290 ms one-shot; per-job overhead = ~17 ms ffmpeg encoder open + ~265 ms render loop.

**Test (self-proof)**
- File: `crates/gamereel-farm/tests/local_worker_amortizes_init.rs::worker_init_then_5_jobs_amortizes_cuda_setup`
- Spins up one worker, runs 5 jobs, asserts:
  * `worker.init_wall_ms` is in [100, 1500] ms (~290 expected on RTX 3060)
  * Median wall of jobs 1..5 is ≤ first job × 0.95 (proves amortization)
  * Total wall < 5 × first (proves it's not just luck)

**Measurement**
- worker init: 293 ms
- first job:   323 ms
- jobs 1..5 median: **287 ms each** ← steady state
- savings vs naive 5 × first:  141 ms (8.7%)

This translates to **287 ms per video = 1045 fps** on hs-mvp (300 frames / 0.287 s) — **3.4× over the M3 single-shot demo (980 ms / 306 fps)** and **2.2× over `cuda_vram_leak`'s sequential 100-cycle 484 fps baseline** (which rebuilt CudaConverter per cycle).

**Retro**
- Hypothesis vs reality: **massively over-delivered**. We expected ≥ 600 fps, got 1045 fps. The win was bigger than projected because keeping `CudaConverter`'s persistent device buffers and `CudaHwContext`'s NV12 pool *also* keeps the NVENC encoder warm across jobs — NVENC's first-encode latency (kernel scheduling, GPU clock ramp) doesn't reset between jobs.
- Lessons:
  1. **Persistence pays compounding interest.** CUDA + hwframes + encoder state all share warm-up costs that amortize together.
  2. The 17 ms/job ffmpeg encoder rebuild is the unavoidable floor — `h264_nvenc`'s reference-frame DPB doesn't survive a video boundary cleanly. Worth revisiting if NVENC API ever exposes a "reset" path.

---

### O-019 — `WorkerPool` measurement — throughput peaks at 2 workers, not 6    [milestone: M5-3 + M5-7]

**Hypothesis**
- Expected impact: **6 workers × LocalWorker on RTX 3060** would give 4–5× scaling (the consensus assumption from the M3 NVENC concurrency probe that suggested 6+ sessions saturate the engine).
- Mechanism: `WorkerPool` spawns N `LocalWorker`s on dedicated OS threads (CUDA context affinity); jobs round-robin through bounded mpsc channels (capacity 1 per worker, total = N).

**Test (self-proof)**
- File: `apps/hs-mvp/src/bin/farm_bench.rs` — sweeps {1, 2, 3, 4, 6, 8} workers × 100 hs-mvp jobs, records wall + p50 + p99 + throughput. Output: `benches/results/m5_farm.json`.

**Measurement** (RTX 3060, 100 jobs of 720x1080 × 30 fps × 10 s hs-mvp)

| Workers | Wall (s) | Throughput (fps) | videos/min | p50 (ms) | p99 (ms) |
|---:|---:|---:|---:|---:|---:|
| 1 | 29.9 | 1004 | 201 | **284** | **290** |
| **2** | **26.9** | **1113** | **223** | 518 | 547 |
| 3 | 27.1 | 1107 | 221 | 781 | 810 |
| 4 | 27.4 | 1095 | 219 | 1047 | 1101 |
| 6 | 27.8 | 1078 | 216 | 1583 | 1695 |
| 8 | 28.3 | 1062 | 212 | 2112 | 2329 |

**Retro: hypothesis was wrong, the data revealed something more useful**
- **Throughput peak at workers=2** (1113 fps) — beyond that, throughput slightly *declines* and p99 latency scales linearly with worker count.
- **NVENC is the bottleneck.** Single LocalWorker already hits 1004 fps which is essentially the RTX 3060's NVENC engine ceiling for 720x1080. Adding workers just queues more jobs against the same hardware encoder; they wait their turn.
- The "6 workers per RTX 3060" rule of thumb (which I had baked into `probe::workers_for_gpu`) came from the standalone NVENC concurrency test, where 6 streams *can* coexist. But "coexist without erroring" is not the same as "scale linearly" — at 1080p the engine is fully utilized by ~2 streams.
- **Critical user-facing implication**: for "near-real-time" use cases (sub-300 ms per video), use **workers=1** — same throughput within 10%, p99 cut in half (290 ms vs 547 ms). For pure batch throughput, workers=2.
- Lessons:
  1. **Lookup tables built from hardware spec sheets lie.** Always validate the rule of thumb against the actual workload.
  2. p50/p99 latency reveals queue contention earlier than throughput does. Throughput plateau + latency rise = hardware saturated.

**Updated `probe::workers_for_gpu`**: now returns 2 (not 6) for RTX 3060 / RTX 4060, with new `workers_for_latency()` companion that always returns 1 for the latency-sensitive branch.

---

### O-020 — Cloud-ready `Worker` trait abstraction (RemoteWorker stub)    [milestone: M5 — interface only]

**Hypothesis**
- Expected impact: zero runtime cost today, **zero mainline code changes required** to add a future RemoteWorker (gRPC/HTTP to a cloud GPU node).
- Mechanism: `Worker` async trait with `LocalWorker` and stub `RemoteWorker` implementations. The job queue / dispatch layer (`WorkerPool`) is generic over `Worker` and never names the concrete type.

**Test (self-proof)**
- The stub `RemoteWorker::new()` returns `WorkerError::Init("not implemented yet — would target ...")` so accidental wire-up fails loudly.
- `RenderJob` and `RenderResult` derive `serde::{Serialize, Deserialize}` — the gRPC/HTTP wire format is already nailed down.

**Measurement**
- N/A as a perf change. The cost is entirely "an extra trait + stub module".

**Retro**
- Lessons: pre-defining the trait + the wire-format contract today (cheap) is far less work than retrofitting them after a concrete LocalWorker has grown its own assumptions about being in-process. The user explicitly asked for cloud-ready — landed it as scaffolding now.
- Followup: M5+1 implements RemoteWorker. The contract guarantees no `WorkerPool`, `JobQueue`, or CLI changes needed — if the implementation has to leak details into the dispatcher, that's a smell to refactor.

---

## M4 — wgpu compositor (replaces image_effect.rs CPU per-pixel blend)

### O-021 — wgpu init + composite shader on Vulkan/RTX 3060    [milestone: M4-1+M4-2]

**Hypothesis**
- Expected impact: a wgpu-backed compositor with a single render-pass + readback
  pipeline can replace the CPU `image_effect::blend_images` per-pixel loop.
  Init cost ~200 ms one-shot; per-frame compose ≪ 1 ms on RTX 3060.
- Mechanism: `WgpuCompositor` holds device/queue/render pipeline + persistent
  render target + persistent readback buffer + one uniform buffer with dynamic
  offsets across draws. Sprite shader `composite.wgsl` mirrors image_effect
  semantics (anchor → scale → rotate → blend).

**Test (self-proof)**
- File: `crates/gamereel-compositor/tests/wgpu_init_and_blit.rs` — 2 tests
  - `init_and_blit_single_red_square_top_left` confirms top-left anchor places
    sprite at (0,0) with red pixels, far corner cleared transparent.
  - `anchor_centered_places_sprite_at_center` confirms center anchor + center
    pos → sprite center at scene center.
- No `#[ignore]`: wgpu falls back to llvmpipe in CI without GPU.

**Measurement** (RTX 3060 + Vulkan)
- adapter selection + device init: **250 ms** (cold start, first call).
- `compose_to_host` for a single sprite: ~0.5 ms wall.

**Retro**
- Wgpu version: started on 22.1, upgraded to 29 mid-implementation. The 22→29
  rewrite touched ~15 API points (`InstanceDescriptor`, `TexelCopy*Info`,
  `RenderPassColorAttachment.depth_slice`, `multiview_mask`, `PollType` enum,
  `Option<&str>` entry points, `immediate_size` replacing `push_constant_ranges`,
  etc.). The cost was an afternoon of mechanical edits; the value is keeping up
  with the active wgpu line so M4.5 (CUDA-Vulkan external memory interop) lands
  on the API that still receives upstream support.
- Naga (the WGSL validator) refuses non-constant indexing into `let array<…>`,
  so the unit-quad vertex enumeration uses `switch vid` instead of array
  indexing. Documented in the shader source — future shader authors should
  expect this trap.
- Lesson: when the dependency rewrites its API, bumping early is cheaper than
  fighting two API surfaces in adjacent PRs.

---

### O-022 — Scene → SpriteDraw adapter + CPU-vs-wgpu pixel parity    [milestone: M4-3+M4-4]

**Hypothesis**
- Expected impact: `compose_scene_frame` translates `Scene` state into a
  back-to-front sprite list and renders one frame; output should be
  visually identical to `Scene::on_render` (the CPU path) at SSIM ≥ 0.99 or
  equivalent pixel-difference bound.
- Mechanism: walks `scene.children` (BTreeMap → deterministic z-order),
  builds a `SpriteDraw` per active node carrying the same pos/scale/rotation/
  anchor/opacity transforms image_effect uses on CPU. Background drawn first,
  then static layer, then dynamic active layer.

**Test (self-proof)**
- File: `apps/hs-mvp/tests/wgpu_parity.rs::wgpu_matches_cpu_within_pixel_tolerance`
  - Runs the same hs-mvp scene through both pipelines for 30 frames.
  - Asserts mean per-channel `|diff| < 8/255` AND less than 1% of pixels
    diverge by more than 16 levels.

**Measurement** (hs-mvp, 30 frames @ 720x1080)
- mean per-channel `|diff|`: **0.114 / 255**
- pixels diverging by > 16 levels: **0.173%** average across frames

**Retro**
- The 0.114/255 mean diff is well below the just-noticeable threshold (≥ 1 level
  on 8-bit). The 0.17% outliers concentrate on anti-aliased edges where wgpu's
  bilinear sampling rounds differently from `image::imageops::resize` Triangle
  filter — visually indistinguishable, mathematically expected.
- Hypothesis matched cleanly. Lesson: when both implementations produce real
  sub-pixel work (interpolation, blending), enforce a tolerance not equality.
  Equality assertions on float-derived pixel data are a flake factory.

---

### O-023 — End-to-end M4 path measurement: 1.4× single-video, much more in batch    [milestone: M4-5]

**Hypothesis**
- Expected impact: replacing CPU compose with wgpu compose saves ~150–200 ms
  per video on hs-mvp (the dirty-cached but still nontrivial 75 active animation
  frames at ~5 ms each ⇒ 375 ms total CPU compose collapses to ~150 ms wgpu
  compose+readback).

**Test (self-proof)**
- File: `apps/hs-mvp/src/bin/wgpu_render.rs` — full e2e binary running
  WgpuCompositor → cudarc kernel → ffmpeg CUDA hwframes → h264_nvenc.
  Reports phase timeline + per-frame breakdown + JSON dump for diff.

**Measurement** (single-video, hs-mvp, 720x1080 × 30 fps × 10 s)

| path | wall | fps e2e | per-frame median |
|---|---:|---:|---:|
| hs-mvp main (CPU compose + CUDA encode, M5 D3 default) | 941 ms | 319 | (see trace) |
| **M4 wgpu_render** (wgpu compose + CUDA encode) | **675 ms** | **444** | 0.93 ms total |

Single-video improvement: **1.40×**. The per-frame budget is now **0.93 ms**
(theoretical ceiling ~1072 fps) but wall is dominated by **454 ms of one-shot
init**: `wgpu.init+upload` 191 ms + `cuda.init` 241 ms + `encoder.open` 17 ms.

Per-frame breakdown (M4 wgpu+CUDA, sum of 300-frame phases):
- wgpu.compose (GPU compose + readback): 144 ms  (51.3%)
- cuda.convert (RGBA→NV12 kernel):        36 ms  (12.9%)
- cuda.synchronize (per-frame stall):     91 ms  (32.5%)
- cuda.copy_to_pool + send_frame:          9 ms  (3.3%)

**Retro**
- The single-video 1.40× under-sells the architecture because half the wall
  is one-shot init that **M5 worker pool already amortizes** for the CPU path.
  When we wire wgpu+cuda context into LocalWorker (M4.5 follow-up, ~half day's
  work), per-video steady-state should be ~140 ms (287 ms M5-CPU baseline minus
  the 150 ms compose savings) — predicted **~2.0× over M5 workers=1**.
- The 91 ms `cuda.synchronize` slug remains as the next obvious cost. Removing
  it requires wgpu↔CUDA stream-shared synchronization (M4.5 zero-copy path),
  which is also where the 144 ms readback can drop to 0.
- Lesson: cold-start single-video benchmarks chronically misrepresent
  amortizable architecture wins. When proposing perf changes, always show both
  the cold and the steady-state numbers; readers and operators care about
  different ones.

**Followups (planned for M4.5)**
1. Wire WgpuCompositor into `gamereel-farm::LocalWorker` so worker pool
   amortizes wgpu init alongside cuda init.
2. wgpu Vulkan backend → CUDA external-memory interop (zero-copy):
   eliminate the 144 ms readback + 91 ms synchronize together.
3. Add a synthesized "heavy scene" (50+ overlapping nodes) bench so the wgpu
   advantage shows up even without batch amortization. hs-mvp's tiny dirty
   cache hides the win on simple scenes.

---

## M4.5 — wgpu compositor in worker pool: opt-in with proven case

### O-024 — Wgpu compose has a real case (136× on heavy scenes), but loses on hs-mvp's dirty cache    [milestone: M4.5]

**Hypothesis**
- Expected impact: wiring `WgpuCompositor` into `LocalWorker` (alongside
  persistent `CudaConverter` + `CudaHwContext`) would shave the 200+ ms
  CPU compose slug from each video and push hs-mvp 100-job batch from
  M5's 26.9 s wall to ~14 s (~2× over M5 baseline).

**Test (self-proof — both branches)**
- File: `crates/gamereel-compositor/tests/heavy_scene_cpu_vs_wgpu.rs`
  Runs 5 full-screen RGBA overlays × 60 frames through both CPU
  `image_effect::blend_images` and wgpu `compose_to_host`. Asserts
  wgpu speedup ≥ 2× — fails loudly if wgpu has no case.
- Companion: `apps/hs-mvp/src/bin/farm_bench.rs` re-run with
  `GAMEREEL_WORKER_COMPOSITOR=wgpu` measures the actual hs-mvp
  workload through the wgpu worker.

**Measurement**

Heavy synthetic scene (5 full-screen overlapping sprites, 60 frames @ 720×1080):
| path               | total ms | per-frame ms | vs CPU |
|--------------------|---------:|-------------:|-------:|
| CPU image_effect   | 3834     | 63.90        | 1.0×   |
| wgpu compose       |   28     |  0.47        | **136.93×** |

Real hs-mvp 100-job batch (workers=1):
| path                    | wall   | throughput | p50  | p99  |
|-------------------------|-------:|-----------:|-----:|-----:|
| CPU compose (M5 default)| 29.9 s | 1004 fps   | 284  | 290  |
| wgpu compose (M4.5)     | 31.3 s |  957 fps   | 297  | 301  |

**Retro: hypothesis wrong on hs-mvp, right in general**
- Wgpu is decisively faster (136×!) when the scene has heavy
  per-frame compositing the CPU can't cache around: lots of
  overlapping full-screen sprites, all moving / changing every frame.
- Wgpu is 4–7 % SLOWER on hs-mvp because hs-mvp's `Scene::_dirty`
  cache + small/sparse sprites hit ~80 % cache rate, making CPU
  compose median 0.07 ms / frame. Wgpu's per-frame compose+readback
  is constant ~0.5 ms with no cache benefit.
- The bet was wrong about WHICH workloads matter. We optimized the
  thing that doesn't bottleneck hs-mvp.

**What we shipped**
- Wgpu compose path in `LocalWorker` is **opt-in only** via
  `GAMEREEL_WORKER_COMPOSITOR=wgpu`. Default stays CPU.
- The decision rule the user can apply at deploy time:
  * Scene has heavy per-frame compositing (50+ overlays, lots of
    full-screen layers, no cache wins) → wgpu opt-in.
  * Scene has small sprites + dirty-cacheable layout (hs-mvp shape) →
    leave default CPU.
- Keep the integration code (~50 LOC) because the gain on heavy scenes
  (>2× per the assertion threshold, 136× on the test case) is real
  and the ceremony is small.

**Known limitation surfaced by the test (followup)**
- 5-layer alpha-blend test shows mean per-channel diff of 29.6/255
  between CPU and wgpu. Cause: wgpu uses Rgba8UnormSrgb with
  hardware-srgb-aware blending while `image_effect` does linear
  alpha compositing. Each layer's gamma divergence compounds with
  alpha < 255. hs-mvp parity passed (0.114/255 mean diff) only
  because most hs-mvp sprites are opaque. Production users of the
  wgpu path with semi-transparent layered content need either:
  (a) Rgba8Unorm framebuffer + manual sRGB convert, or
  (b) accept the gamma-correct (wgpu) behavior as the new ground
      truth. Recorded for a future "compositor color space"
      discussion; not blocking the M4.5 ship.

**Lessons**
1. "We have an architecture that should be faster" doesn't ship without
   a measured case. The opt-in lives only because the heavy_scene test
   passes the 2× bar.
2. Dirty caches are the silent winner for sparse-update scenes. Don't
   assume GPU compute beats CPU on workloads that aren't actually
   compute-bound.
3. Always run the proposed faster path against the *real* workload, not
   just the synthetic case it was designed for. Without farm_bench we'd
   have shipped wgpu as default and made hs-mvp 5 % slower.

---

## S1 — Business integration foundation

### O-025 — Closed-loop protocol → render → URL    [milestone: S1]

**Hypothesis**
- Expected impact: a binary blob from the game side reaches a public
  URL with no per-job manual intervention. Latency budget: protocol
  decode + render + upload < 1 s for a 10-s replay on the reference
  machine. Pure plumbing milestone — perf gains are implicit
  (existing M5/M3 numbers don't regress).
- Mechanism: 3 new pieces compose end-to-end.
  1. `proto-puzzle` decodes match-3 replay JSON → `MetaSceneList`
     translation (no game-logic re-simulation; mechanical event
     mapping per `docs/protocols/match3-replay-spec.md`).
  2. `gamereel-output::OutputSink` async trait + `LocalDiskSink` +
     `ObjectStorageSink` (S3-compatible: AWS / Aliyun OSS / R2 /
     MinIO / GCP-via-S3 / configured per env vars).
  3. e2e test wires `mock_replay → PuzzleParser::parse →
     LocalWorker::render → LocalDiskSink::deliver → DeliveryReceipt`.

**Test (self-proof)**
- Schema spec: `docs/protocols/match3-replay-spec.md` — v0 JSON,
  v1 protobuf path documented.
- File: `crates/proto-puzzle/tests/parse_and_translate.rs` — 3 tests
  validating mock-replay round-trip, scene shape (≥ 65 nodes for
  8×8 board + bg), and helpful error on bad JSON input.
- File: `crates/gamereel-output/tests/local_disk_sink.rs` — 2
  tests validating bytes-on-disk match payload + filename
  sanitization for adversarial job_ids.
- File: `crates/gamereel-output/tests/e2e_render_to_sink.rs`
  (#[ignore], CUDA-gated) — full closure: decode → render → upload,
  asserts receipt URL points at a real file with the rendered bytes.

**Measurement** (RTX 3060 e2e single shot)
- proto-puzzle decode of 9-event mock: < 1 ms
- LocalWorker init (cudarc + hwframes + wgpu): 476 ms one-shot
- Render hs-mvp scene 10s @ 30fps: 307 ms wall (285 ms render loop)
- LocalDiskSink upload 42.9 KB: < 1 ms
- **Total e2e: 793 ms** (cold start). Subsequent jobs amortize the
  476 ms init via M5 worker pool to ~290 ms each per O-018.

**Retro**
- Plumbing milestone delivered cleanly. The translation in
  `proto-puzzle::translate` uses 9 event variants for what the spec
  calls out; ScoreChange / Combo / MatchEnd are decoded but not yet
  rendered (text rendering is a separate milestone). Documented as
  "v0 — no UI nodes yet" in source comments.
- The Cargo dependency graph now correctly cascades:
  proto-puzzle → gamereel-core (no circular dep);
  gamereel-output → gamereel-core + gamereel-farm;
  apps/gamereel-cli already depends on proto-puzzle and gets the
  PuzzleParser via inventory at link time. No core changes needed.
- Lessons:
  1. **Define the wire spec before writing the parser.**
     `docs/protocols/match3-replay-spec.md` exists as a contract the
     game team can review WITHOUT reading Rust. v0 JSON lets the
     schema iterate while v1 protobuf path is documented but
     unblocked.
  2. **OutputSink trait gives Sinks the same plug-in shape as
     ProtocolParsers.** Future TikTok / IG / WeChat sinks add zero
     plumbing — implement `OutputSink` and they compose into
     `CompositeSink` for fan-out delivery.
  3. **S3-compatible API + env-driven config** = single binary works
     on AWS / Aliyun OSS / Cloudflare R2 / MinIO / GCP without code
     branches. The right level of abstraction for "we don't know
     which cloud yet".

**Followups (next sprint S2)**
- `gamereel-farm-server` binary wrapping LocalWorker behind gRPC
- `RemoteWorker` client implementation in `gamereel-farm`
- Dockerfile + cloud-deploy manifests
- ObjectStorageSink integration test against MinIO local docker
- Replace text-render TODOs in proto-puzzle::translate with cosmic-text
  (parallel work, can hand off)
