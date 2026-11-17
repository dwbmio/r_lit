# Maquette

> Kit-based low-poly asset forge — stack blocks, paint textures, export 3D models and 2D isometric renders.

**Maquette** (v0.1.1) is a Rust desktop modeling tool and headless CLI for indie game developers who want to assemble low-poly voxel/kit props without firing up Blender. It splits cleanly into a **window-free core library** (`maquette`), a **headless CLI** (`maquette-cli`), and a **Bevy + egui desktop GUI** (`maquette`).

## Highlights

- **Headless / GUI Split**: `maquette-cli` compiles with `--no-default-features --bin maquette-cli` (zero `bevy_egui` / `wgpu` windowing overhead) for CI pipelines, thumbnails, and batch exports.
- **Voxel/Kit Modeling & Composer**: 2D/3D grid editing, multi-block composer (`block_composer`), built-in 12-block library plus optional `hfrog` cloud block-meta sync (`block_meta`), undo/redo (`history`), and `.maq` project save/load with autosave.
- **glTF 2.0 / GLB Export**: Exports composed scenes to `.glb` or `.gltf` with optional inverted-hull outline baking (`--outline-width`, `--outline-color`, `--no-outline`), shared by GUI `File -> Export` and `maquette-cli export`.
- **Pure-CPU Isometric Renderer**: `maquette-cli render` rasterizes `.maq` projects to PNG thumbnails without requiring a GPU or windowing session.
- **Palettes & AI Texture Pipeline**: Portable `colors.json` palette export/import (`palette_io`), per-slot texture metadata (`texture_meta`), deterministic `mock` provider, and `rustyme` (sonargrid/Redis) AI texture generation with SHA-256 disk caching (`texgen`).
- **GUI Extras**: Pan-orbit camera, toon preview & outline shader, multi-view PIP preview (`multiview`), floating panels, and cloud queue status.

## Tech Stack

| Component | Crate | Version |
|---|---|---|
| Core / ECS / 3D | `bevy` | 0.18 |
| GUI panels (`gui` feature) | `bevy_egui` | 0.39 |
| Camera (`gui` feature) | `bevy_panorbit_camera` | 0.34 |
| Ground grid (`gui` feature) | `bevy_infinite_grid` | 0.18 |
| Outline (`gui` feature) | `bevy_mod_outline` | 0.12 |
| CLI | `clap` | 4 |
| Texture queue / HTTP | `redis`, `ureq`, `sha2`, `png` | - |

## Build & Install

Pre-built binaries (Linux/macOS):

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/maquette/install.sh | bash
```

Build locally from `maquette/`:

```bash
# Desktop GUI + CLI (default `gui` feature)
cargo build --release
cargo run --bin maquette

# Headless CLI only (fast CI build, no GUI crates)
cargo build --no-default-features --bin maquette-cli --release
cargo test --no-default-features
```

From the monorepo root:

```bash
just build maquette release
```

## CLI Usage (`maquette-cli`)

```bash
# Export .maq project to GLB / glTF (with optional inverted-hull outline)
maquette-cli export scene.maq -o scene.glb
maquette-cli export scene.maq -o scene.gltf --no-outline

# Inspect and validate .maq files (`--json` is global per rlit_cli; `__schema` dumps JSON Schema)
maquette-cli info scene.maq --json
maquette-cli validate scene.maq
maquette-cli __schema

# Render pure-CPU isometric PNG thumbnail
maquette-cli render scene.maq -o preview.png --width 512 --height 512 --background '#181a1e'

# Palette import / export
maquette-cli palette export scene.maq -o colors.json
maquette-cli palette import scene.maq --from colors.json -o scene_recolored.maq

# Texture generation (mock offline or rustyme sonargrid cluster)
maquette-cli texture gen --prompt "mossy stone tile" -o stone.png --provider mock --seed 42
maquette-cli texture revoke <TASK_UUID>
maquette-cli texture purge texgen

# Block library metadata (local bundled + hfrog artifact registry)
maquette-cli block list --source all --json
maquette-cli block get oak_planks
maquette-cli block sync
```

## Project Layout

```text
src/
├── lib.rs              # Window-free core library (Headless Invariant)
├── bin/
│   └── maquette_cli.rs # Headless CLI binary (`maquette-cli`)
├── main.rs             # Bevy + egui GUI binary (`maquette`)
├── grid.rs             # 2D/voxel grid model & palette
├── mesher.rs           # Voxel grid -> triangle mesh builder
├── project.rs          # `.maq` project format serde load/save
├── export.rs           # glTF 2.0 / GLB writer + inverted-hull outline baker
├── render.rs           # Pure-CPU isometric PNG rasterizer
├── palette_io.rs       # `colors.json` palette import/export
├── texture_meta.rs     # Per-slot texture metadata & TexturePrefs
├── texgen.rs           # Mock & Rustyme AI texture providers + SHA-256 cache
├── block_meta.rs       # BlockMeta trait, LocalProvider & HfrogProvider
└── (GUI modules)       # ui, camera, scene, block_library, block_composer,
                        # preview_mesh, multiview, export_dialog, slot_texgen,
                        # texture_registry, toon, history, autosave, session,
                        # float_window, notify, cloud_status, fonts
```

## Part of r_lit

This crate belongs to [r_lit](https://github.com/dwbmio/r_lit). While `r_lit` focuses on short-running CLIs, `maquette` ships both a desktop GUI (`maquette`) and a headless CLI (`maquette-cli`) sharing one library core.
