# Maquette

> Kit-based low-poly asset forge — stack blocks, export 3D models and 2D sprites.

**Maquette** is a lightweight desktop tool for indie game developers who want to
assemble low-poly 3D props and bake 2D sprite sheets without firing up Blender.
You pick from a library of primitives, drop them in a viewport, arrange and
paint, then export to glTF / OBJ or render a batch of sprite frames.

The name comes from the French / film-industry term for a scaled-down modular
mock-up — which is exactly what this tool is for.

## Status

**MVP · v0.1** — the foundation:

- Empty window with a 3D viewport and pan-orbit camera
- Infinite ground grid with world axes
- Block library side panel with six primitives (Cube, Sphere, Cylinder, Cone, Plane, Torus)
- Properties panel showing transform of the last-spawned block
- Top menu bar scaffold (File / Edit / View / Help) with placeholder actions
- Status bar with block count and control hints

Not yet implemented (planned for v0.2+):

- Click-to-select blocks in the viewport (`bevy_picking`)
- Transform gizmo (`transform-gizmo-bevy`)
- Custom block import (user-provided glTF / OBJ)
- Project save / load (`.maq` JSON format)
- glTF export of the composed scene
- Multi-angle sprite sheet baking (8 / 16 / 24 directions)
- Procedural scripting hook (rhai or mlua)

## Tech Stack

| Component | Crate | Version |
|-----------|-------|---------|
| Engine / 3D | `bevy` | 0.18 |
| UI panels | `bevy_egui` | 0.39 |
| Camera | `bevy_panorbit_camera` | 0.34 |
| Ground grid | `bevy_infinite_grid` | 0.18 |

## Build

From this directory:

```bash
cargo run                # dev build (deps optimized, fast iteration)
cargo build --release    # optimized binary in target/release/maquette
```

From the monorepo root:

```bash
just build maquette release
```

First build will take a while — Bevy and its dependencies are substantial.
Subsequent incremental builds are fast.

## Controls

| Input | Action |
|-------|--------|
| Left-drag in viewport | Orbit camera |
| Right-drag / middle-drag | Pan camera |
| Scroll wheel | Zoom |
| Click a block in the library | Spawn it into the scene |
| Edit → Clear Scene | Delete every block |

## Project Layout

```
src/
├── main.rs      # App setup, plugin wiring
├── camera.rs    # Pan-orbit camera spawn
├── scene.rs     # Lighting, sky, infinite grid
├── block.rs     # BlockKind enum, spawn/clear systems, Messages
└── ui.rs        # egui panels (menu, library, properties, status)
```

The UI layer is **immediate-mode egui**, not ECS. ECS only holds scene data
(blocks, transforms, camera). Adding a new feature usually means one Message
type plus one Bevy system that consumes it.

## Design Notes

- Each placed block is an ECS entity with `BlockKind` + `Transform` +
  `Mesh3d` + `MeshMaterial3d` + `Name` components.
- UI → scene communication is one-way via `Message`s (`SpawnBlockEvent`,
  `ClearSceneEvent`). No shared mutable state.
- Bevy's `EguiPrimaryContextPass` schedule runs the UI once per frame in
  multi-pass mode.

## License

MIT

## Part of r_lit

This crate is part of the [r_lit](../README.md) monorepo of Rust tools.
Unlike the short-running CLI utilities in this repo, Maquette is a
long-lived desktop app — its inclusion here is a deliberate exception to
the "run, complete, exit" convention, since it shares the same build
infrastructure.
