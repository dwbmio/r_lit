# img_resize

> Local, offline game texture optimizer & image resizer (PNG / JPG / WebP). Zero network dependencies.

## Quick Start

### 1. Channel-Profile Texture Optimization (`optimize $engine_$channel.yml`)

Optimize a Godot mini-game asset folder (WeChat / QQ / Douyin / Alipay Mini Game) with perceptual PNG quantization (`imagequant` + PSNR quality guard + `oxipng`), fake-alpha stripping, transparent RGB cleanup + edge bleed, effective-pixel ratio auditing, and optional JPG/WebP conversion:

```bash
# List built-in $engine_$channel presets
img_resize profile list

# Scaffold a commented starter profile (e.g. godot_wx-minip.yml)
img_resize profile init godot_wx-minip

# Dry-run audit first (effective pixel ratios, hotspots & projected byte savings, zero writes)
img_resize optimize godot_wx-minip.yml ./assets -o ./assets_wx --dry-run --json

# Execute optimization and save a JSON audit report
img_resize optimize godot_wx-minip.yml ./assets -o ./assets_wx --report report.json --json
```

Ready-to-use profiles are also checked in under `profiles/`:
- `profiles/godot_wx-minip.yml` — Godot WASM → WeChat Mini Game
- `profiles/godot_qq-minip.yml` — Godot WASM → QQ Mini Game
- `profiles/godot_dy-minip.yml` — Godot WASM → Douyin Mini Game
- `profiles/godot_zfb-minip.yml` — Godot WASM → Alipay Mini Game
- `profiles/godot_mobile.yml` — Godot Native Mobile (iOS/Android; lossless source for ASTC/ETC2)
- `profiles/raw_png-jpg.yml` — Generic PNG/JPG channel

> **Convention**: Naming a config `$engine_$channel.yml` (e.g. `godot_wx-minip.yml`) automatically infers `engine` and `channel` and inherits the preset defaults — even a 2-line YAML overriding just `input`/`output` works out of the box.

### 2. Simple Resize (`r_resize`)

```bash
# Scale proportionally to max 800px
img_resize r_resize -m 800 image.jpg -o out/

# Exact resize to 1920x1080
img_resize r_resize --rw 1920 --rh 1080 image.jpg -o out/

# Batch process a directory
img_resize r_resize -m 1024 images/ -o out/

# Generate multiple sizes from a YAML spec
img_resize r_resize -c sizes.yaml icon.png
```

## Optimization Pipeline Details

1. **Effective Pixel & Alpha Analysis**
   - **Strip Fake Alpha** (`strip_fake_alpha: true`): RGBA PNGs with all pixels opaque (`alpha >= 252`) drop the useless alpha channel (RGB8) with zero visual or layout impact.
   - **Zero Transparent RGB + Edge Bleed** (`zero_transparent_rgb: true`, `edge_bleed_px: 1`): Clears dirty hidden RGB data in fully transparent pixels after dilating 1px of visible edge color (`alpha = 0`) to prevent dark fringes under GPU bilinear filtering.
   - **Low Effective Pixel Ratio Audit**: Flags textures where visible pixels or tight bbox cover too little of the canvas (e.g. `< 35%`), suggesting atlas packing (`mj_atlas`). `trim_transparent` is **off by default** to keep Godot pivots/anchors/9-patch/UVs safe, and can be enabled per directory via `rules:`.
2. **Perceptual Compression (TinyPNG-equivalent, 100% Offline)**
   - **PNG**: Local `imagequant` (`quality_min..quality_max`) + **PSNR Safety Guard** (`min_psnr_db`, default `37..39 dB`; falls back to lossless PNG if quantization would band) + `oxipng` lossless filter/deflate + metadata stripping.
   - **JPG**: Metadata stripped, configurable quality (`80..88`).
   - **WebP**: Lossy VP8 (with alpha) or lossless VP8L via `libwebp`.
   - **No-Regression Guard**: Never overwrites an unresized/unconverted image with a larger file.
3. **Built-in Godot Protection Rules (`inherit_default_rules: true`)**
   - Normal / ORM / mask / roughness / metallic / height / bump maps (`**/*_{normal,n,norm,orm,roughness,metallic,mask,height,bump,ao}.*`), `**/fonts/**`, and `**/*_pixel*.*` are automatically protected as lossless.

## Supported Formats

- **Input**: PNG, JPEG, WebP, BMP, GIF, TIFF
- **Output**: PNG, JPEG, WebP

## JSON Output & Machine Contract

```bash
img_resize --json optimize profiles/godot_wx-minip.yml ./assets -o ./out
img_resize __schema
```

Follows `r_lit` CLI contract v1 (single JSON envelope on stdout, exit codes `0..5`, full `artifacts` list).

## Build From Source

```bash
cd img_resize && cargo build --release
```

## Notes

- `tinyfy` is disabled (OpenSSL/musl conflict); `optimize` replaces it locally via `imagequant` + `oxipng`.
- License note: `img_resize` uses `imagequant` (GPL-3.0) for local PNG quantization (same as `mj_atlas`).
