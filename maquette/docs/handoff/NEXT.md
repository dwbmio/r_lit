# NEXT · maquette

Current in-flight: **v0.9** — robustness work: autosave + crash
recovery, Bevy feature trim for a smaller release binary, and a
preferences file so viewport toggles stick across launches. v0.8
shipped the T-shape multi-angle preview (Top / Front / Side PIPs),
dock-to-float preview window, empty-state onboarding, Fit-to-Model,
and a discoverable preview toolbar. See `v0.8-complete.md`.

Reference: `v0.4-complete.md` · `v0.5-complete.md` · `v0.6-complete.md`
· `v0.7-complete.md` · `v0.8-complete.md` · `v0.9a-complete.md`.

## Roadmap snapshot

| Ver  | Theme                                                   | Status    |
|------|---------------------------------------------------------|-----------|
| v0.4 | Meshing + height + export                               | shipped   |
| v0.5 | Headless CLI + CI infra                                 | shipped   |
| v0.6 | Palette editor + stroke undo + greedy meshing           | shipped   |
| v0.7 | Headless render + GUI feature-gate + palette CLI        | shipped   |
| v0.8 | Multi-angle preview + float window + onboarding + QoL   | shipped   |
| v0.9 | Robustness: autosave, Bevy feature trim, prefs, perf    | in flight |
| v1.0 | Release candidate: docs, icon, smoke matrix, tag        | not yet   |

See `v0.8-complete.md` §"What's still needed to reach v1.0" for the
detailed rationale behind each post-v0.8 version theme.

## Inline patches since v0.8 (not a full version bump)

- **2026-04-23 · Rust quality audit** — zero `unwrap` / `expect` /
  `panic!` / `todo!` / `unimplemented!` / `unreachable!` in
  production code. All I/O modules expose typed `thiserror` error
  enums (`PaletteIoError`, `ProjectError`, `RenderError`,
  `ExportError`). CLI `main` returns `ExitCode::from(1)` with a
  friendly stderr on error. `#[allow(clippy::too_many_arguments)]`
  usages are all justified (Bevy systems). Gap found & fixed below.
- **2026-04-23 · Toast / notification system** (`src/notify.rs`).
  Closed the silent-I/O-failure UX bug: save / open / save-as
  errors in `session.rs` and export outcomes in `export.rs` now
  surface as color-coded toasts in the top-right. `Toasts`
  resource is GUI-only; the lib emits a new `ExportOutcome`
  message (`maquette::export::ExportOutcome`) that the GUI
  translates. Headless invariant preserved — CLI never depends on
  `notify.rs`. `Toasts::{info, warning}` are public but unused;
  marked `#[allow(dead_code)]` with a `v0.9+` comment (autosave
  will consume them).
- **2026-04-23 · App icon proposals** — four candidate PNGs
  landed under `docs/icons/proposals/`. User picks one in
  `USER-TODO.md #26`; agent then generates the full size-ladder
  + `.icns` / `.ico` and wires into `Cargo.toml`.
- **2026-04-23 · User verification checklist** —
  `docs/handoff/USER-TODO.md` created. Single-file flat list of
  every manual step (#1–#28) from v0.4 through v1.0 release,
  grouped by version, each with time estimate and pass criterion.
  This supersedes the "verification debt" sections below as the
  canonical user-facing artifact; NEXT.md's list stays as the
  agent-facing index.

## v0.9 sub-tasks (ordered)

- [x] **A** Autosave + crash recovery **(shipped 2026-04-23, see
      `v0.9a-complete.md`)**. Sidecar `<path>.maq.swap` flushed on
      stroke-committed + window-blur; recovery modal on File → Open
      when swap mtime > project mtime; lib gains `swap_path` /
      `swap_is_newer` / `write_swap` / `remove_swap` +
      `EditHistory::strokes_committed` monotonic counter. 11 new
      tests (project + history + cli). **Deferred to v0.9 C**:
      untitled-project autosave (needs prefs dir) and startup
      auto-recovery (needs last-opened-path persistence).
- [ ] **B** Bevy feature trim. v0.7 gated the five extra GUI crates;
      Bevy itself still compiles with its default feature set.
      Audit `render / pbr / winit / animation / audio / gizmos /
      scene / text / gltf` and disable what Maquette doesn't use.
      Target: cold-build time drop ≥ 2 minutes, release binary
      size < 25 MB. Record before / after in `v0.9-complete.md`.
      *Risk*: a feature we disable turns out to be transitively
      required by `bevy_egui` / `bevy_panorbit_camera` /
      `bevy_mod_outline`. Ship feature-by-feature, CI each step.
- [ ] **C** Preferences file. `~/.config/maquette/prefs.toml`
      (platform-appropriate via `dirs`) persists
      `MultiViewState.enabled`, `FloatPreviewState.floating`,
      brush height, and the recent-files list. Reads on startup,
      writes on quit. GUI-only; the CLI never touches it.
- [ ] **D (stretch)** Perf pass. Profile a 32×32 canvas with
      column heights up to 8: paint-to-preview latency, mesh
      rebuild time, PIP render overhead. Budget: 60 fps on an
      M1 base with multi-view on. Record hot paths. Optimise only
      what's actually hot.

## v0.9 decisions (agent to lock unless overruled)

- **Swap file location** = beside the `.maq`, suffix `.maq.swap`.
  Not in a global cache dir — keeps "a project is a directory"
  property, and the swap is visible to the user.
- **Swap policy** = flush on stroke close + on window blur. Not
  on every paint op (wasteful) and not on timer (surprising).
- **Bevy feature gate** = audit via `cargo tree --features …` +
  a sample build / click-test per toggle; ship a minimal set
  named `gui` and a `gui-audio` etc. only if users actually ask.
- **Prefs file format** = TOML. Small, human-editable, survives
  a failed deserialise with a logged warning + default values.
  Not JSON (overkill) and not RON (Bevy-idiomatic but less
  inspectable by end users).

## Locked decisions (carried forward)

1. **Canvas column height cap = 8.** Non-negotiable before v1.0.
2. **Export format = both `.gltf` and `.glb`**, user picks.
3. **Outline export = inverted-hull**, configurable width + color.
4. **Product positioning**: top-down low-poly asset editor, not
   MagicaVoxel / Qubicle.
5. **Headless Invariant** (2026-04-23) — data core compiles and
   tests without a window; every shippable operation has a CLI
   verb (or a documented reason why it's GUI-only interactive).
6. **CLI surface** — `maquette-cli export / info / validate /
   render / palette {export,import}` shipping. New verbs require an
   entry in `COST_AWARENESS.md` and a matching integration test in
   `tests/cli.rs`.
7. **Palette is sparse** (v0.6) — deleting a color leaves its slot
   as `None` and future `add` reuses the hole. Project files are
   schema v3; v1 / v2 files load automatically.
8. **Meshing is greedy by default** (v0.6). Culled mesher retained
   as `build_color_buckets_culled` for regression tests only.
9. **Palette portability format** (v0.7) — `colors.json`, schema v1,
   hex-string colors with `null` for deleted slots. See
   `maquette/src/palette_io.rs` for the canonical shape.
10. **Render projection** (v0.7) — isometric (yaw −45°, pitch
    ≈ 35.264°), flat Lambert shading on a fixed camera-space light,
    sRGB PNG. No outline baked into the preview PNG (engines see the
    inverted-hull already; the CLI render is a shape sanity-check,
    not a marketing shot).
11. **GUI feature flag** (v0.7) — `gui` is a default feature; CI can
    build the CLI with `--no-default-features --bin maquette-cli` to
    skip `bevy_egui` / `bevy_panorbit_camera` / `bevy_infinite_grid`
    / `bevy_mod_outline` / `rfd`. Trimming Bevy's *own* feature set
    is the v0.9 follow-up (§A above).
12. **Multi-view preview = PIPs, not splitters** (v0.8). Three
    orthographic (Top / Front / Side) picture-in-picture viewports
    in the bottom-right corner, toggled by `View → Multi-view
    Preview` or `F2`. Splittable quadrants were considered and
    deferred — cost/value not justified before v1.0.
13. **Float preview window = parallel, not replacement** (v0.8).
    Floating the preview *adds* a second OS window with its own
    `PanOrbitCamera`; the main in-editor preview keeps rendering.
    Closing the OS window docks. One floating window at a time.
14. **Fit-to-Model shortcut = `F`** (v0.8). Frames the model
    AABB without changing the orbit angle. `Cmd+R` still performs
    a full reset. `F2` toggles multi-view PIPs.

## Verification debt

### Still owed from v0.4 (human)

1. Paint at different brush heights; confirm vertical extrusion
   matches expectations.
2. End-to-end export validation: drop a `.glb` into Godot 4 /
   Unity 6 (glTFast) / Blender 4 and confirm the flow in
   `docs/export/*.md` is accurate.
3. Export as `.gltf` (text) and confirm the sibling `.bin` is
   next to it.

### From v0.5 (install / sanity)

4. `cargo install --path maquette` puts both `maquette` and
   `maquette-cli` on your `$PATH`.
5. `maquette-cli export foo.maq --out foo.glb` produces a file
   that opens cleanly in your engine of choice.
6. `maquette-cli info foo.maq --json | jq` works.
7. `cargo test` passes on your machine (76+ tests).

### From v0.6 (palette + stroke + greedy)

8. Palette editor: right-click a swatch, edit the color, click
   elsewhere → color persists. Restore unchanged via `.maq`
   round-trip.
9. Palette delete modal: paint a cell with a color, right-click
   → Delete → confirm "Erase" mode, cell clears; paint again,
   Delete → Remap to another color, cell updates.
10. Palette "+" button adds a hue-shifted swatch and selects it
    immediately. Shift-click / 1-9 shortcuts still map to the
    nth *live* color, not the nth slot.
11. Undo across a multi-cell drag rolls back the whole stroke,
    not one cell.
12. Export a moderately-sized fixture (say 16×16 flat slab) with
    the GUI. File size should be noticeably smaller than a v0.5
    export of the same canvas — greedy meshing win.

### From v0.7 (render + palette CLI)

13. `maquette-cli render <fixture>.maq --out preview.png --width
    800 --height 600` produces a PNG that a human can open and
    visually match against the in-app preview. Shading direction
    is consistent (top brighter than +Z/-X sides).
14. `cargo build --no-default-features --bin maquette-cli` succeeds
    and produces a working CLI without `bevy_egui` / friends in the
    dep tree.
15. Cross-engine validation (deferred from v0.7 C): export a `.glb`
    with the CLI, open in Godot 4 / Unity 6 / Blender 4, compare
    geometry + colors + outline against the CLI-rendered PNG.
    Archive screenshots under `docs/export/screenshots/` when done.
16. Palette CLI: `maquette-cli palette export proj.maq --out
    colors.json`, hand-edit a hex entry, `palette import proj.maq
    --from colors.json --out proj2.maq`. Open `proj2.maq` in the
    GUI; the edited color should be reflected in all painted cells
    using that slot.

### New in v0.8 (preview UX)

17. **Multi-view preview**. Paint a recognisable column (e.g. a
    3×3 L-shape) and confirm the Top PIP shows an L, the Front
    PIP shows the column's height, and the Side PIP shows the
    other profile. Toggle `F2` off and on; the PIPs hide / reappear
    and retain their camera pose.
18. **Float window**. Click `Float`; a second OS window opens at
    the current camera pose. Orbit in the float window, close it
    via the OS close button; the `Float` toggle flips off. Re-open
    it; the new window opens at the last *floating* pose, not the
    original docked pose (because docking copied it back).
19. **Fit to Model**. Paint a single cell at one corner of a
    32×32 canvas; press `F`. The preview reframes on the cell with
    ~70% viewport fill. Press `Cmd+R`; the view resets to defaults.
20. **Empty-state hint**. Start a new project; the hint panel is
    centered on the canvas. Paint one cell; it vanishes. `Edit →
    Clear Canvas`; it reappears.

## Working notes (mutable scratchpad)

- Cargo's unit-of-compilation is the crate. With default features,
  both binaries compile the full Bevy tree. CLI-only builds skip
  the five GUI extensions (≈ 43 s saved on a cold build); trimming
  Bevy's own default features (`render`, `pbr`, `winit`…) is the
  outstanding v0.9 win — the measured cold-build gap above that is
  Bevy compiling itself with `wgpu` / `naga` / etc.
- The `[[bin]]` table in `Cargo.toml` pins `maquette` to
  `required-features = ["gui"]` so `cargo build --no-default-features`
  only builds the CLI. `default-run = "maquette"` so
  `cargo run` still launches the GUI.
- `bevy_mod_outline` is bin-only. If any lib code ever needs
  outline data (e.g. for a "bake outline into vertex color"
  feature), it must live in the inverted-hull baker under
  `export.rs`, not under `mesher.rs`.
- `project::apply_to_grid_and_palette` exists specifically for the
  GUI (mutates existing `ResMut` handles). CLI and tests use
  `project::read_project` which returns fresh `(Grid, Palette)`.
  Don't add a third load API without reading this note.
- CLI integration tests shell out via `CARGO_BIN_EXE_maquette-cli`
  — the env var Cargo injects at test-compile time.
- `ExportPlugin` + `ExportRequest` remain in the lib. The GUI uses
  the Message; the CLI calls `export::write` directly. Same
  `ExportOptions` struct, so any new option reaches both surfaces.
- Palette is sparse (`Vec<Option<Color>>`). Use `Palette::get(idx)`,
  `iter_live()`, `live_count()`, `add()`, `update()`, `delete()`.
  Never index `palette.colors[i]` directly; slots can be `None`.
- `EditHistory::begin_stroke` / `end_stroke` live under the GUI
  binary. The data structure itself is headless-tested.
- Greedy meshing in `mesher.rs` is the shipping path.
  `build_color_buckets_culled` stays as the regression oracle.
- `render::rotate_iso` bakes an iso rotation (yaw −45°, pitch ≈
  35.264°). "Greater z = closer" in rotated space. The rasterizer
  uses an edge-function scanline loop with a depth buffer; no
  mipmaps, no MSAA — flat-shade-only quads, so a single-sample
  raster is perceptually equivalent to supersampled for the voxel
  use case.
- `palette_io` owns the `colors.json` schema. If a new palette
  feature adds per-slot metadata (e.g. names, tags), bump the
  schema version there and keep `read_palette_json` accepting the
  old version as a subset.
- **v0.8** GUI additions (`multiview.rs`, `float_window.rs`,
  the Fit / Reset / Multi / Float toolbar, empty-state overlay)
  all live in `src/` — none in the lib. `camera::painted_bbox` is
  currently bin-local; promote to `maquette::geom::painted_bbox`
  only if the CLI `render` grows a `--fit` flag.
- **v0.8** `FloatPreviewState` spawns a `Window` + `Camera3d` with
  `RenderTarget::Window(WindowRef::Entity(...))` (Bevy 0.18 ships
  this as a standalone component, not a field of `Camera`).
  `WindowClosed` is the message that fires when the OS close is
  clicked; don't confuse with `WindowCloseRequested`.
- **v0.8** PIP cameras keep `is_active = false` on spawn and get
  flipped by `apply_enabled`. Toggling via `MultiViewState` does
  **not** despawn entities — cheap, and keeps camera transforms
  stable across toggles.
- **Toasts** (`src/notify.rs`, 2026-04-23 patch). New lib code that
  wants to surface an end-user message should emit a `Message` in
  the lib (see `ExportOutcome`) and add a consumer system in
  `notify.rs` that translates it into `Toasts::{success, info,
  warning, error}`. Do **not** take `ResMut<Toasts>` from lib code
  — breaks the Headless Invariant. For GUI-only modules
  (`session.rs`, `autosave.rs`), direct `ResMut<Toasts>` is fine.
- **Autosave** (`src/autosave.rs`, v0.9 A). If any future feature
  mutates `Grid` or `Palette` outside the normal paint flow, make
  sure `EditHistory::record` is still called — autosave's trigger
  is the stroke counter, not dirty-bit observation. Anything that
  sets `CurrentProject::unsaved = true` without pushing a history
  stroke will get saved on next window-blur but not on the
  committing-tick, which is usually fine but worth knowing.
- **Swap format** (`project.rs`, v0.9 A). The swap file is a plain
  `.maq` under the hood — not compressed, not a journal, not a
  binary diff. This is the contract the CLI tests pin. If a future
  version needs a compact swap (larger projects, faster writes),
  bump the constant or add a sibling `.maq.swap.v2` — don't silently
  change the on-disk shape behind existing `read_project` readers.
