# mj_atlas

[中文文档 / Chinese →](README_CN.md)

**mj_atlas** is a fast, single-binary texture-atlas packer with an opinionated workflow for game engines. It takes a directory of sprite images and produces a packed atlas PNG plus the metadata sidecar your engine can consume directly — TexturePacker-compatible JSON, Godot 4 native `.tres` resources, or whatever else you wire up next.

It is also one of the very few open-source packers that ships with **UV-stable incremental packing**: re-running the tool after editing or adding a sprite reuses the existing layout for unchanged sprites, so already-deployed game clients can drop in the new atlas without rebaking UVs.

```
sprite_dir/  ──────►   atlas.png  +  atlas.json     (TexturePacker-compatible)
                        atlas.manifest.json         (incremental cache)
```

## Highlights

- **MaxRects bin packing** with optional 90° rotation, edge extrude, configurable spacing/padding, and POT atlas sizing
- **Multi-atlas auto-split** when sprites exceed `--max-size`
- **Transparent-pixel trimming** with a configurable alpha threshold (`--trim-threshold`)
- **Polygon-mesh output** — `--polygon` extracts per-component contours, simplifies (Douglas-Peucker), triangulates (earcut), and writes `vertices` / `verticesUV` / `triangles` so engines can skip transparent overdraw
- **Multi-component meshing** — disjoint blobs in a single sprite each get their own contour and triangulation, joined into one mesh
- **Polygon shape modes** — `--polygon-shape concave|convex|auto` to trade fidelity for vertex count, plus `--max-vertices N` as a hard budget
- **Lossy PNG quantization** via imagequant (`--quantize`, ~60-70% size reduction)
- **Duplicate-sprite detection** — SHA256 pixel hashing with a fast cheap-key pre-reject; aliases reuse one canonical position
- **Animation auto-detection** — files matching `name_NN.ext` are grouped into animation sequences (TexturePacker `animations` field, Godot `SpriteFrames`)
- **Incremental packing with UV stability** (see below)
- **Optional GUI** (`--features gui`) — egui + wgpu, drag-drop sprites, inline preview, polygon mesh overlay
- **Three output formats out of the box** — TexturePacker JSON Hash / Array, Godot `.tpsheet` (plugin), Godot native `.tres` (zero plugin)

## Install

The release pipeline ships pre-built binaries to [dev.gamesci-lite.com](https://dev.gamesci-lite.com) for macOS arm64/x86_64, Linux x86_64/aarch64, and Windows x86_64. Or build locally:

```bash
cd mj_atlas
cargo build --release                    # CLI only
cargo build --release --features gui     # CLI + GUI
```

The binary lands at `target/release/mj_atlas`.

## Quick Start

```bash
mj_atlas pack ./sprites -o atlas --trim --pot
```

Outputs `./sprites/atlas.png` and `./sprites/atlas.json`.

A full demo (procedurally generated sprites + every interesting flag combination) is in [`examples/run_demo.sh`](examples/run_demo.sh):

```bash
python3 examples/gen_sprites.py    # generate 13 demo sprites
examples/run_demo.sh               # run mj_atlas with 5 different presets
```

## Output Formats

| Format            | Extension  | Use case                                                  |
|-------------------|------------|-----------------------------------------------------------|
| `json` (default)  | `.json`    | TexturePacker JSON Hash — universal                       |
| `json-array`      | `.json`    | TexturePacker JSON Array (frames as ordered list)         |
| `godot-tpsheet`   | `.tpsheet` | Godot 4 — needs the TexturePacker Godot plugin            |
| `godot-tres`      | `.tres`    | Godot 4 — generates `AtlasTexture` + `SpriteFrames`, **zero plugin needed** |

```bash
mj_atlas pack ./sprites -o atlas --format godot-tres --trim --pot
```

The Godot SDK at [`sdk/godot/addons/mj_atlas/`](sdk/godot/addons/mj_atlas/) provides a GDScript loader for the polygon-mesh JSON variant.

## Incremental Packing (`--incremental`)

mj_atlas writes a sidecar manifest (`<output>.manifest.json`) that records the packed layout, options hash, per-sprite SHA256 content hash, and the maximal free rectangles inside each atlas. On re-run with `--incremental` the tool diffs the input directory against the manifest and picks the cheapest path:

| Diff                                                 | Action                                          | Cost      |
|------------------------------------------------------|-------------------------------------------------|-----------|
| Nothing changed                                      | **Full skip** — no decode, no write             | ~10 ms    |
| Pure addition that fits in free space                | Partial repack — paint new sprite in free rect  | low       |
| In-place pixel edit (same trimmed dimensions)        | Partial repack — replace pixels at existing rect | low       |
| Removal                                              | Partial repack — clear old rect, free space     | low       |
| Resized edit / new sprite that doesn't fit / option change | Full repack                                | full      |

The critical invariant is **UV stability**: every sprite that did not change keeps its exact `(x, y, rotated)` across runs. Already-shipped game code can drop in a new atlas PNG and continue using its baked UVs — only the metadata sidecar gains the new sprite entries. This makes mj_atlas safe to use as part of a hot-patch / live-asset workflow.

```bash
# build with cache
mj_atlas pack ./sprites -o atlas --trim --pot --incremental

# add a sprite, run again — UV-stable partial repack
echo "icon_added.png appears in ./sprites"
mj_atlas pack ./sprites -o atlas --trim --pot --incremental

# force-bypass the cache when you want determinism verification
mj_atlas pack ./sprites -o atlas --trim --pot --incremental --force
```

The `--json` output surfaces cache state so CI pipelines can short-circuit:

```json
{
  "status": "ok",
  "atlases": 1,
  "cached_atlases": 1,
  "skipped": true,
  "files": [{"image": "atlas.png", "from_cache": true, "...": "..."}]
}
```

See [`docs/INCREMENTAL.md`](docs/INCREMENTAL.md) for the manifest schema, failure modes, and CI integration patterns.

## Polygon Mesh

Adding `--polygon` switches the output to per-sprite triangle meshes that hug the opaque pixels. Game engines can render the mesh instead of the rectangle, cutting transparent-fragment overdraw by 30%+ for irregular sprites.

```bash
mj_atlas pack ./sprites -o atlas --trim --pot \
    --polygon --polygon-shape auto --max-vertices 12
```

| Option                  | Effect                                                                |
|-------------------------|-----------------------------------------------------------------------|
| `--polygon`             | Enable mesh extraction                                                |
| `--tolerance 1.5`       | Douglas-Peucker simplification (lower = tighter, more vertices)       |
| `--polygon-shape concave` (default) | Keep simplified outline                                   |
| `--polygon-shape convex`            | Replace each component with its convex hull (fewer verts) |
| `--polygon-shape auto`              | Pick convex when concave/hull-area ≥ 0.85, else concave   |
| `--max-vertices N`                  | Hard budget — escalate tolerance until total ≤ N (×1.5 per round, max 8 rounds) |

Multi-component sprites (e.g. a UI badge with three disjoint icons) get one contour per connected component, all packed into one combined `vertices` + `triangles` set. See [`docs/POLYGON.md`](docs/POLYGON.md) for a worked example.

## Manifest as First-Class Artifact (v0.3)

Once you've packed with `--incremental`, the manifest sidecar (`<output>.manifest.json`) becomes a content-addressed view of your sprite library. v0.3 adds four read/write subcommands that operate on it directly — no repack required.

| Subcommand | Purpose |
|---|---|
| `mj_atlas inspect <atlas_or_manifest>` | Pretty-print the manifest: per-atlas stats, occupancy, free-rect count, tag aggregation, sprite list |
| `mj_atlas diff <a> <b>` | Compare two manifests — added / removed / pixel-changed / resized / **moved** (UV-stability break) / tag changes |
| `mj_atlas verify <atlas>` | Re-hash atlas PNGs (and optionally sprite sources with `--check-sources`) against the manifest; non-zero exit on mismatch |
| `mj_atlas tag <atlas> <sprite> --add ui,icon --set-attribution "CC0"` | Read or write per-sprite metadata: tags, attribution, source URL — preserved across repacks |

All four accept the manifest path, the atlas PNG, the sidecar metadata, or the directory containing them — paths are auto-resolved (multi-bin `_<N>` suffixes are also handled).

Tags / attribution / source URL live in the manifest under each sprite entry and are **excluded from the cache key** — editing them never invalidates the incremental cache.

```bash
# What's in this atlas?
mj_atlas inspect ./out/atlas.png

# Did the layout change between two builds? Did UVs stay stable?
mj_atlas diff ./build_a/atlas.manifest.json ./build_b/atlas.manifest.json

# Pre-deploy sanity check
mj_atlas verify ./out/atlas.png --check-sources

# Annotate sprites for downstream tooling
mj_atlas tag ./out/atlas.png walk_01.png --add walk,character --set-attribution "CC0 procedural"
mj_atlas tag ./out/atlas.png hero_idle.png --add hero,idle --set-source-url https://opengameart.org/...
```

JSON output (`--json`) is available on every subcommand for CI / dashboards.

## CLI Reference

```
mj_atlas pack <INPUT_DIR> [OPTIONS]
mj_atlas inspect <ATLAS_OR_MANIFEST>
mj_atlas diff <A> <B>
mj_atlas verify <ATLAS_OR_MANIFEST> [--check-sources]
mj_atlas tag <ATLAS_OR_MANIFEST> [SPRITE] [--add ...] [--remove ...] [--clear]
                                          [--set-attribution ...] [--clear-attribution]
                                          [--set-source-url ...] [--clear-source-url]
                                          [--list]
mj_atlas formats               # list output formats
mj_atlas gui                   # interactive GUI (--features gui)
mj_atlas preview <ATLAS_FILE>  # preview a packed atlas (--features gui)
```

Run `mj_atlas <subcommand> --help` for the full option list. All commands accept `--json` for machine-readable output (errors go to stderr as JSON).

For LLM / agent integration, [`llms.txt`](llms.txt) provides the same information in a token-efficient structured format.

## Comparison

| Feature                          | mj_atlas | TexturePacker | free-tex-packer |
|----------------------------------|:--------:|:-------------:|:---------------:|
| Open source                      |    ✓     |       ✗       |        ✓        |
| Single static binary             |    ✓     |       ✗       |        ✗        |
| Polygon mesh                     |    ✓     |       ✓       |        ~        |
| Multi-component polygon meshing  |    ✓     |       ✗       |        ✗        |
| Godot native `.tres` output      |    ✓     |       ~       |        ✗        |
| **Incremental + UV-stable**      |  **✓**   |       ✗       |        ✗        |
| Manifest sidecar (asset registry foundation) | ✓ |    ✗       |        ✗        |
| Lossy PNG (palette quantization) |    ✓     |       ✓       |        ~        |
| GUI                              |   opt-in |       ✓       |        ✓        |

## License

MIT. The `--quantize` feature pulls in `imagequant` which is GPL-3.0; binaries built with quantization enabled inherit GPL terms. The default build is MIT-clean.

## Contributing

This crate lives in the [`r_lit`](https://github.com/...) Rust CLI tool collection. Each tool is an independent Cargo crate with its own release cycle. PRs welcome — please run `cargo test` and follow the existing module layout (`src/pack/`, `src/output/`, error types in `src/error.rs`).
