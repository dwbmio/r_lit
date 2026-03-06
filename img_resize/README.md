# img_resize

> Pure Rust image resizing and compression tool. No network dependencies.

## Quick Start

```bash
# Scale proportionally to max 800px
img_resize r_resize -m 800 image.jpg

# Exact resize to 1920x1080
img_resize r_resize --rw 1920 --rh 1080 image.jpg

# Batch process a directory
img_resize r_resize -m 1024 images/

# Force convert to JPG
img_resize r_resize -m 800 -j image.png
```

## Resize Modes

### 1. Proportional (`-m`)

Scales the image so neither dimension exceeds the given pixel value. Preserves aspect ratio.

```bash
img_resize r_resize -m 800 photo.jpg
```

### 2. Exact (`--rw` + `--rh`)

Resizes to an exact width and height. Does not preserve aspect ratio.

```bash
img_resize r_resize --rw 1920 --rh 1080 photo.jpg
```

### 3. Config File (`-c`)

Uses a YAML config to generate multiple output sizes at once.

```yaml
vec_size:
  - [1920, 1080]
  - [800, 600]
vec_f:
  - "output/large.png"
  - "output/small.png"
base_f: "/output/base/path"
```

```bash
img_resize r_resize -c config.yaml input.png
```

## Supported Formats

- PNG
- JPG / JPEG

## JSON Output

```bash
img_resize --json r_resize -m 800 image.jpg
```

Returns structured JSON for programmatic parsing.

## Build From Source

```bash
cargo build --release
```

## Note

The `tinyfy` subcommand is currently disabled due to OpenSSL/musl compatibility issues.

## License

MIT
