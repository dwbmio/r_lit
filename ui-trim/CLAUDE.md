# CLAUDE.md

This file guides AI agents working on `ui-trim`.

## Purpose

`ui-trim` is a short-running Rust CLI for cleaning AI-generated UI asset PNGs. It removes edge-connected pseudo-transparent backgrounds, checkerboard-like white/gray matte pixels, and red crop guide lines, then writes a tight RGBA PNG.

This is a deterministic pixel tool, not a semantic image editing tool. Do not call AI services for trimming.

## Current Implementation

- Default path: pure Rust CPU.
- PNG decode/encode: `image` crate with PNG feature.
- Hot algorithm steps: edge matte sampling, edge-connected flood-fill, 1px close/open morphology, alpha cleanup, optional small-radius feather, bbox crop.
- The JSON report includes option semantics and timing breakdowns so callers and agents can reason about cost and quality.

## OpenCV / SIMD / GPU Policy

- Do not add OpenCV by default. The C++ runtime and dynamic library deployment cost is too high for this CLI's current operations.
- SIMD should be added only when benchmark data shows a real hot path. Candidate areas: morphology mask kernels, distance/feather transform, future resize. Candidate crates: `fast_morphology`, `fast_image_resize`.
- GPU is not an MVP path. It only makes sense for very large batches or an already GPU-resident asset pipeline.
- If acceleration is added, keep it behind a Cargo feature and preserve the pure Rust CPU path.

## AI-Friendly CLI Contract

Important parameters:

- `--padding`: keeps visual breathing room around the alpha bbox. Icons/buttons usually use 4-6; glow/bubbles may need 8-16.
- `--alpha-threshold`: pixels with alpha <= this are background. Keep low (default 4) to avoid deleting faint glow.
- `--feather`: softens foreground alpha near removed background. Range 0-3. Higher values may shrink thin highlights.
- `--max-bg-distance`: RGB threshold for sampled edge matte clusters. Raise for uneven warm matte; lower if pale UI edges are being removed.
- `--remove-red-guides`: removes red guide lines only when edge-connected.

JSON metadata fields:

- `trim_bbox`: `[x0, y0, x1, y1]` in input coordinates after padding.
- `alpha_ratio`: fraction of output pixels whose alpha is above threshold. Near 1 may mean background was not removed; extremely low may mean over-trim.
- `removed_pixels`: count of pixels marked as edge-connected background.
- `timings_ms`: decode/sample/flood/morphology/alpha/bbox/encode/total breakdown.
- `throughput_mp_s`: input megapixels processed per second for the algorithm path.
- `options`: normalized parameter values plus implementation/acceleration labels.
- `warnings`: machine-readable quality warnings.

## Verification

Run:

```bash
cargo test
cargo run --example smoke
cargo run --release --example benchmark
```

Expected: unit tests cover matte trim and interior matte preservation; CLI smoke validates JSON/output; benchmark prints JSON timing cases for 512, 1024, and 2048 square synthetic UI assets.

## Current Benchmark Snapshot

Latest local release benchmark:

- `512x512`: about 18.5 ms, about 20.0 MP/s.
- `1024x1024`: about 56.7 ms, about 20.7 MP/s.
- `2048x2048`: about 197.7 ms, about 21.5 MP/s.

The current bottleneck is mask processing. If the user asks for more throughput, optimize morphology/feather first; do not spend time on PNG codec or CLI argument parsing unless `timings_ms` proves they dominate real inputs.
