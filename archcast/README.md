# archcast

**AI-first deterministic architecture & flow report compiler (SIMD + Rayon + WebGPU).**

`archcast` separates the **AI-friendly single source of truth** (compact YAML/JSON) from **human-facing visual deliverables** (fixed-style HTML with built-in WebGPU/Canvas2D motion, standalone root-level `.svg` diagrams, optional raster `.png` and animated flow-pulse `.gif`) and **engineering wiki documentation** (`Markdown + Mermaid`). It also reverse-extracts existing `HTML + SVG` overview pages back into canonical YAML and Markdown.

## Features

- **1:1 Fixed Visual Style (`render`)**: Compiles YAML/JSON specs into deterministic 4-column swimlane reports (before/after dual diagrams, action subcards, auto-sized pill badges, orthogonal bus routing, payload/gap callouts, warning banners, grouped API/callback tables).
- **Root-Level Standalone SVG + Rich HTML + Markdown**: Default `--format all` writes `<stem>.html`, standalone `<stem>-<diagram_id>.svg` (or `<stem>.svg` for single-diagram specs), and `<stem>.md` directly into `--output-dir`.
- **HTML-First Browser WebGPU Motion Layer**: Generated HTML embeds a zero-dependency WebGPU WGSL instanced particle overlay (automatically falling back to 60fps Canvas2D) for animated data-flow pulses, click-to-trace upstream/downstream card highlighting, and one-click SVG download. Pass `--static-html` for pure zero-JS HTML.
- **Raster PNG & Animated Flow-Pulse GIF (`--png`, `--gif`, `--format media`)**:
  - Pure-Rust SIMD SVG rasterization (`resvg` + `tiny-skia`) with system CJK font loading.
  - Animated looping GIF export with glowing packets traveling along routed edges and breathing status badges.
  - Default build runs frame compositing in parallel on CPU via Rayon (100% headless CI safe); optional `--features gpu` executes frame glow compositing on a native `wgpu` WGSL compute shader (Metal / Vulkan / DX12).
- **Reverse Extraction (`extract`)**: Parses existing `HTML + SVG` overviews via SIMD scanning (`memchr`), zero-copy XML DOM (`roxmltree`), and 4-wide SIMD spatial containment (`wide::f64x4`) into clean YAML + Markdown.
- **Agent Skill**: See `SKILL.md` for end-to-end AI agent authoring workflows.
- **`rlit_cli` Machine Call Contract v1**: Global `--json` envelopes, deterministic exit codes (`0-5`), SHA-256 `artifacts` receipts, hidden `__schema` export, and `<output_dir>/archcast.log` sidecar logs.

## Build

```bash
cd archcast
cargo build --release
cargo test

# Enable native WebGPU WGSL compute shader backend for animated GIF compositing
cargo build --release --features gpu
```

## Quick Start

```bash
# 1. Generate a starter YAML spec
archcast init starter.yaml --title "Release Pipeline Overview"

# 2. Validate topology and schema
archcast validate starter.yaml --json

# 3. Compile YAML -> HTML (WebGPU/Canvas2D motion) + standalone root SVG(s) + Markdown
archcast render starter.yaml -o out/ --strict --json

# 4. Also export static PNG + animated flow-pulse GIF
archcast render starter.yaml -o out/ --format media --strict --json

# 5. Reverse-extract an existing HTML+SVG overview into YAML + Markdown
archcast extract legacy-overview.html -o extracted/ --json

# 6. Inspect runtime SIMD / Rayon / WebGPU capabilities
archcast info --json
```

## Machine Call Contract

- `--json` outputs exactly one `rlit_cli` envelope on stdout; human logs go to stderr and `<output_dir>/archcast.log`.
- `archcast __schema` prints the machine-readable CLI JSON Schema.
- Exit codes: `0` ok, `1` runtime, `2` usage, `3` input/validation, `5` partial batch failure.
