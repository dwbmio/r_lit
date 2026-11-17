# Cost Awareness — What's expensive in a Kenney-style asset tool

> Permanent reference, append-only. Every version should re-read this before
> deciding priorities. Based on analysis of kenney.nl/tools and accumulated
> project insight.

---

## The North Star

Maquette is a **voxel / block-style Toon low-poly asset editor** with
optional **AI-assisted region hinting**, **not** a Mixer (asset-remix) tool
and **not** a doodle-to-3D tool. The editor hides 3D concepts (camera,
lights, UV, materials) and exposes only: grid + color + height + style +
export.

This framing matters because the "most expensive" part of the product is
different for each of the three archetypes:

| Archetype | Most-expensive area |
|-----------|---------------------|
| Mixer (avatar / ship / creature) | Stylistically coherent **art asset library** + remix rules + export |
| **Block / voxel editor (us)** | **3D editor completeness + export compatibility + polish** |
| Doodle → 3D | Constrained procedural geometry, or ML inference quality + deployment |

So for Maquette, budget goes into **editing completeness**, **export
robustness**, and **feel / polish** — in that order.

---

## Permanent Cost-List (sorted by long-term total effort)

### 🔴 Tier 1 — where most of the work will live

#### Product-level polish
- Reliable **undo / redo** (every action path must round-trip)
- **Project save / autosave / crash recovery**
- **Input feel**: snapping, alignment, symmetry, sensible defaults, hotkeys
- **Performance at scale**: large canvases, real-time preview, no hitches on edit
- **Docs, tutorials, onboarding samples, i18n (zh/en minimum)**
- **Error UX**: every failure becomes a visible toast, not a panic or silent log

→ These almost always exceed the effort of the "core feature" they support.
  Assume the polish pass after any new capability is **the same size** as
  building the capability.

#### Export & interop
- glTF / GLB / OBJ / FBX round-trips: mesh, material names, UVs, normals,
  tangents, vertex color, units, axis convention
- Engine-specific gotchas: **"opens cleanly in Unity / Godot / Unreal /
  Blender without manual fixing"** is a real testing matrix, not a checkbox
- FBX specifically: there is no good Rust writer; choose one of:
  a) write ASCII FBX ourselves (3–5 days of work + ongoing maintenance),
  b) shell out to Blender CLI (dependency on user machine),
  c) ship glTF-only and document it clearly.
- Consistent **scale and units** between editor display and exported file

### 🟡 Tier 2 — meaningful but bounded

#### Voxel → mesh pipeline
- Greedy meshing / face culling / material grouping
- Flat vs smooth normal options
- Vertex-color vs texture-atlas tradeoff (vertex color wins for our use case;
  document that decision)
- LOD generation by varying the merge aggressiveness

#### Toon rendering pipeline
- Cel-shading shader (3–4 bands)
- Outline rendering (post-process normal/depth detection, or inverted-hull
  mesh trick)
- Consistent look under all viewing angles / at all zoom levels
- This is work but has a clear ceiling; many open references exist.

#### Multi-view preview + detachable window
- Bevy multi-camera + render-to-texture + egui image bridge: tricky wiring,
  not conceptually novel
- Multi-window Bevy app + per-window egui: doable, has reference example
- Cost is in the edge cases (DPI, focus, resize), not the happy path

### 🟢 Tier 3 — cheap if scoped, dangerous if unscoped

#### AI region hinting
- Minimum viable scope: **preset-library offline** (no AI at all) +
  **JSON-schema-constrained LLM call** (ollama local or one cloud endpoint)
- Danger: unbounded scope (agentic flows, multi-turn refinement, image
  generation). Resist.

#### Procedural scripting
- If we add rhai / mlua, design the script API to be small, declarative,
  and idempotent. Never expose the internal ECS.
- Defer until after v1.0 if possible.

### ⚫ Tier 4 — buy / borrow / defer, don't build

- Font & icon sets → use Kenney's free sets or existing icon fonts
- Dialog widgets → use egui built-ins
- File pickers → `rfd`, done
- HTTP / async → `reqwest` + `tokio` when needed, not now

---

## Permanent "do not fall for these" list

1. **Do not build a Mixer inside the voxel editor.** If we want character
   mixing later, it's a separate tool. Scope discipline.
2. **Do not expose camera/lighting/material/UV concepts.** Users who want
   those already have Blender. Our moat is "you don't need Blender".
3. **Do not add PBR.** Toon is the only shading model. Fewer knobs = more
   consistent output.
4. **Do not chase FBX fidelity at the cost of glTF quality.** glTF is the
   modern-engine native path; FBX is legacy convenience.
5. **Do not ship an LLM dependency.** AI is optional; the tool must be
   100% useful offline with preset libraries only.
6. **Do not add multiplayer / cloud sync before v1.0.** r_lit has murmur
   for P2P; resist the temptation until there's real demand.

---

## Export Golden Rule (permanent, product-defining)

> **Exports contain geometry and standard materials only. Nothing the
> target engine cannot render out of the box.**

User decision, 2026-04-23: Maquette's primary target engines are
**Godot, Unity, and Blender**. Files must open cleanly in all three
without user intervention.

### Allowed in exports

- Triangle mesh with positions, normals, UVs (even if unused), and
  **vertex colors** (every engine reads these).
- Standard material: single `base_color` (white + vertex color drives it)
  or `unlit` flag. No PBR dance, no textures unless we explicitly ship a
  cel-lookup LUT (deferred).
- A secondary **inverted-hull outline mesh** as a sibling mesh in the
  same glTF/GLB: scaled +N%, flipped winding, flat black, unlit. This
  gives "toon outline" by pure geometry — reads correctly in Godot, Unity,
  Blender with zero shader setup on the user's side.

### Never in exports

- Bevy-specific shader graphs / WGSL snippets.
- Post-process parameters (depth edge detect, SSAO, bloom, etc.).
- Engine-specific node graphs or material graph JSON.
- Any toggle whose behavior depends on the Maquette runtime.

### The Preview ≠ Export invariant

- The **preview** in Maquette can use any technique: `bevy_mod_outline`,
  custom WGSL post-process, compute shaders — all fair game.
- The **export** is a pure-geometry artifact. If a visual effect in the
  preview cannot be expressed as "mesh + vertex color + inverted-hull mesh",
  it must NOT ship in the export; instead, document it in the per-engine
  guide (see below).
- Surface this invariant to the user — in Help/About and in the Export
  dialog — so nobody is surprised.

### Per-engine guidance documents

Ship alongside every export (and in `docs/export/`):

- `godot.md` — how to render the main mesh with Godot's `shader_type
  canvas_item` toon preset, or just use the inverted-hull mesh as-is.
- `unity.md` — URP "Simple Lit" + vertex color setup, or URP outline
  renderer feature if the user wants that instead of inverted-hull.
- `blender.md` — Principled BSDF with `Facing` node for rim, or Freestyle
  for lines, or just import and render inverted-hull as-is.

These are short (half-page each), static, and do not require us to keep
up with engine updates — they describe one working path, not all paths.

---

## Competitive Positioning — who we are, who we aren't

User-confirmed, 2026-04-23.

### What Maquette is

> **A top-down 2D canvas that produces a low-poly stylized game asset,
> with hidden 3D concepts and a Godot/Unity/Blender-friendly export.**

The 2D canvas is the primary interaction. The 3D preview is a mirror,
not a workspace. You paint a layout, you get an asset.

### What Maquette is not (and must resist becoming)

| Tool | Paradigm | Why we're not it |
|------|----------|------------------|
| **MagicaVoxel** | Full 3D voxel editor with a 3D brush in 3D space | Our primary brush is 2D top-down. You pick a cell by x/z, not by x/y/z. MagicaVoxel's UX is *better at 3D painting than we will ever be.* We compete by being *faster for a top-down asset*, not by being a worse 3D painter. |
| **Qubicle** | Multi-matrix voxel rigging / character authoring | We have no rig, no bone, no multi-matrix composition inside one project. Those belong to the downstream engine. |
| **Kenney Asset Forge** | Pre-built blocks snapped together | No asset library. We generate the blocks from the canvas, not assemble from presets. |
| **Kenney Shape** | Stroke → extruded 3D | We don't extrude strokes; we paint cells. Deeper shape control, lower free-form freedom. |

### The key axis we compete on

> **Time from blank canvas to engine-ready asset, for a top-down
> readable shape, while hiding every 3D concept.**

Not "expressive power". Not "voxel fidelity". Specifically the
_onboarding-to-shipping_ distance for someone who wants a tree, a house,
a chest, a tile — and does not want to learn Blender or MagicaVoxel.

Any feature that improves this axis is a yes. Any feature that trades
this axis for expressiveness (3D brush, per-voxel painting, matrix
hierarchy) is a **product line split** (see next section), not a
feature addition to Maquette proper.

---

## Post-v1.0 Possible Product Line Split

> Record kept per user decision, 2026-04-23. **Do NOT act on this before
> v1.0 ships.** It is written here so nobody is surprised when the
> discussion comes up, and nobody smuggles early voxel-character
> features into v0.x iterations.

If Maquette finds users and they start asking for things that
collide with the "top-down asset" north star, the answer is almost
never "add the feature". The answer is "spin up a sibling tool".

### Candidate split (for discussion after v1.0 only)

- **Maquette Asset** (current line) — top-down 2D canvas, height ≤ 8,
  one mesh per project, low-poly game asset. Stays what it is.
- **Maquette Figure** (hypothetical new line) — full 3D voxel-character
  editor with a 3D brush, height effectively uncapped, multi-piece /
  rigged composition. Would share: file format family, palette system,
  export pipeline, toon render, handoff protocol. Would NOT share: the
  2D canvas as primary brush.

### Signals that would justify the split

Do NOT split preemptively. Split only if ≥ 2 of these are true after
v1.0:

1. Real users (not us) repeatedly hit the height cap and ask for more.
2. Real users ask for "paint directly in 3D" as a recurring feature request.
3. Export targets are dominated by character-style assets rather than
   prop/tile/architecture-style assets.
4. The 2D canvas UX starts accumulating "cheat modes" to simulate 3D
   painting (layer buttons, side-view toggles, etc.) — a smell that we're
   building the wrong tool.

### What to NOT do before v1.0

- Do NOT add a side-view or front-view paintable canvas "just in case".
- Do NOT raise the height cap past 8 "to see what users do with it".
- Do NOT support multi-piece / multi-matrix projects inside one `.maq`.
- Do NOT add a 3D brush even as an experiment — users will find it,
  come to rely on it, and block later product decisions.

The discipline here is more important than the code. v1.0 is the
earliest moment this discussion is allowed to be reopened.

---

## The Headless Invariant

User-confirmed, 2026-04-23.

> **Every shippable Maquette operation must be reachable without a
> window.** The GUI is a presentation layer on top of a headless
> data core, not a monolith with CLI hooks retro-fitted on.

### Why this matters

1. **Game build pipelines.** A studio putting Maquette in its
   content pipeline runs the exporter inside `make`, a GitHub
   Action, or a Godot `.gdscript` pre-build hook. If export
   requires a window, it doesn't integrate — full stop.
2. **CI honesty.** A GUI-only app can only be "tested" via
   screenshots and flake-prone UI automation. A headless core can
   be tested with `cargo test` the same way every other crate is,
   including real round-trips through the `gltf` parser and real
   engine fixtures in later phases.
3. **Agent development sanity.** In agentic development (us) the
   agent cannot open the GUI to verify a change. The agent needs
   to be able to `cargo run --bin maquette-cli export ...`, inspect
   the output, and iterate. Without that, every session ends with
   "have the human check the GUI", which is slow and lossy.

### Invariant rules (enforced at PR time, not by ceremony)

1. **No GUI in the core library.** `lib.rs` and everything under
   `pub mod grid / project / mesher / export / …` must compile
   with `default-features = false` and must never `use bevy_egui`,
   `use bevy_panorbit_camera`, or similar windowing crates. The
   test suite must continue to pass in that configuration.
2. **Every user-visible operation has a CLI verb.** If you add a
   menu item to the GUI, you add a matching `maquette-cli` verb
   (or argue in the PR why an operation is fundamentally
   interactive — e.g. painting).
3. **CLI is the first integration target for tests.** New feature
   work lands with tests that invoke the CLI path, not tests that
   spawn a Bevy `App`. Spawning an `App` is reserved for the
   things that truly need it (plugin wiring, render behaviour).
4. **Test-first for non-GUI work.** For anything that touches
   grid / project / mesher / export, the agent writes the
   CLI-invoked test before or alongside the code. The human then
   only has to verify the GUI *presentation* of the feature, not
   its correctness.

### What this implies for pre-v1.0 roadmap

- **v0.5 becomes a headless-CLI release** (see `NEXT.md`). It is
  not optional, not a stretch, and not deferable to post-v1.0. It
  is a prerequisite for declaring the tool "usable in a real
  workflow".
- After v0.5, every later version ships with CLI coverage for any
  new user-visible export/project operation.

### What this explicitly doesn't mean

- We do not need `maquette-cli paint` or any interactive-CLI
  surface for actually authoring assets. The GUI is the authoring
  surface. CLI covers build-pipeline operations: `export`,
  `info`, `validate`, and later `render` for visual regression.
- We do not eliminate Bevy as a dependency of the CLI. The CLI
  binary still links `bevy_ecs` / `bevy_reflect` (our data
  structures use `#[derive(Resource)]`), but runs no window,
  opens no wgpu device, and exits on its own after writing files.

---

## Cheap polish that pays compound interest

These are quick wins we should slot into almost every version:

- Good default window size and layout
- Sensible keyboard shortcuts (Ctrl+Z, Ctrl+S, 1-9 for palette)
- Named colors in tooltips (not "#e64c59")
- Loading spinner / progress indicator for anything > 200 ms
- Non-blocking toasts for success + error
- "Reset to default" for every panel
- Double-click-to-rename on anything named

Add one or two of these to each version's polish budget, don't let them
accumulate to v1.0.

---

## Revision log

- 2026-04-23 — initial commit based on Kenney tool analysis and v0.2 Day 1
  experience. Reviewed by the user.
