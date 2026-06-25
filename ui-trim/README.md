# ui-trim

`ui-trim` cleans AI-generated UI asset PNGs into tight transparent PNGs.

It removes edge-connected pseudo-transparent matte backgrounds, white/gray checkerboard-like pixels, and optional red crop guide lines. It then trims the result to the final alpha bounding box with configurable padding.

## Why

Image models sometimes paint transparency as opaque pixels: checkerboards, white/gray canvas, warm matte, or red crop guides. A plain alpha trim cannot remove those pixels. `ui-trim` is deterministic and local, so it can run inside an asset pipeline without another AI call.

## Install

From source:

```bash
cargo build --release
```

From the repository root:

```bash
just build ui-trim release
```

## Usage

```bash
ui-trim \
  --input raw.png \
  --output clean.png \
  --padding 6 \
  --alpha-threshold 4 \
  --feather 2 \
  --max-bg-distance 48 \
  --remove-red-guides \
  --json
```

## Options

- `--input <PNG>`: input image path.
- `--output <PNG>`: output image path.
- `--padding <PX>`: padding around the final alpha bbox. Default: `6`.
- `--alpha-threshold <N>`: alpha values at or below this are background. Default: `4`.
- `--feather <PX>`: soften foreground alpha near removed background. Range: `0..3`, default: `2`.
- `--max-bg-distance <N>`: RGB distance threshold for sampled edge matte clusters. Default: `48`.
- `--remove-red-guides`: remove edge-connected red crop guide lines.
- `--json`: print machine-readable metadata.

## Output JSON

```json
{
  "ok": true,
  "input_width": 1408,
  "input_height": 480,
  "output_width": 1396,
  "output_height": 468,
  "trim_bbox": [4, 6, 1395, 467],
  "padding_px": 6,
  "removed_pixels": 236690,
  "alpha_ratio": 0.31,
  "throughput_mp_s": 120.5,
  "options": {
    "padding_px": 6,
    "alpha_threshold": 4,
    "feather_px": 2,
    "max_bg_distance": 48.0,
    "remove_red_guides": true,
    "implementation": "pure_rust_cpu",
    "acceleration": "specialized_u8_mask_kernel; png codec dependencies may use CPU intrinsics internally"
  },
  "timings_ms": {
    "decode": 2.1,
    "sample_matte": 0.03,
    "flood_fill": 4.2,
    "morphology": 3.8,
    "alpha_cleanup": 1.6,
    "bbox_crop": 1.1,
    "encode": 5.4,
    "total": 18.3
  },
  "warnings": []
}
```

The metadata is intentionally AI-friendly:

- `timings_ms` lets an agent identify whether decode/encode or mask processing is the bottleneck.
- `options` records normalized parameter values and the active implementation path.
- `alpha_ratio`, `removed_pixels`, and `warnings` are quality signals for automatic pipeline checks.

## Algorithm

1. Decode PNG to RGBA8 with the pure Rust `image` crate.
2. Sample matte color clusters from image edges.
3. Flood-fill only edge-connected matte pixels, so interior UI pixels with similar colors are preserved.
4. Apply 1px close/open morphology to stabilize the background mask.
5. Clear background alpha and optionally feather nearby foreground alpha.
6. Compute the final alpha bbox, expand it by padding, crop, and write PNG.

The default implementation intentionally avoids OpenCV. The hot path is linear RGBA scanning and small-radius mask operations, so OpenCV's C++ runtime and dynamic library deployment cost are not justified for the MVP.

SIMD/GPU policy:

- SIMD should be benchmark-driven and added behind a feature only when morphology/feather becomes a proven bottleneck.
- Candidate SIMD crates are `fast_morphology` for mask kernels and `fast_image_resize` for future resize work.
- GPU is not used by default because upload/synchronization and deployment cost outweigh the current per-image operations.

## Benchmark

Run:

```bash
cargo run --release --example benchmark
```

Latest local synthetic benchmark on this machine:

- `512x512`, 30 iterations: average algorithm time about `18.5 ms`, about `20.0 MP/s`.
- `1024x1024`, 15 iterations: average algorithm time about `56.7 ms`, about `20.7 MP/s`.
- `2048x2048`, 5 iterations: average algorithm time about `197.7 ms`, about `21.5 MP/s`.

The current bottleneck is mask processing (`flood_fill`, `morphology`, `alpha_cleanup`). If production traffic needs more headroom, the first acceleration target should be SIMD or a specialized mask-kernel implementation for morphology/feather.

## Development

```bash
cargo test
cargo run -- --input raw.png --output clean.png --json
cargo run --example smoke
cargo run --release --example benchmark
```
