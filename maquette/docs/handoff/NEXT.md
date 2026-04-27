# NEXT · maquette

**Status, 2026-04-27 (late evening).** v0.9 A (autosave + crash
recovery) shipped 2026-04-23. A second wave of v0.9 polish + the
v0.10 A/B texture pipeline shipped earlier today
(see `v0.9b-complete.md`). The same evening **v0.10 B-bis** landed
(see `v0.10b-bis-complete.md`): the producer is now live-verified
against sonargrid at `10.100.85.15:12121/ui`, with one
foreign-reply infinite-loop bug fixed in passing (and the same
bug in `rustyme-py`'s SDK fixed + regression-tested in the
sonargrid working tree, awaiting maintainer review). Then
**v0.10 C-1** landed (see `v0.10c1-complete.md`): lib-side schema
v4 with `ProjectMeta`, `Palette::slot_meta`, `TextureHandle`,
`TexturePrefs`, and the `*_with_meta` load/save APIs. Working
tree is clean, **143 tests + clippy green** (107 lib + 11 history
+ 6 export + 19 CLI; was 118).

Active fronts:

* **v0.9 follow-up (B + C)** — Bevy feature trim and the prefs file
  are still on the v0.9 list. Neither is started.
* **`#TEX-B` end-to-end** — ✅ unblocked + verified (cpu solid +
  cpu smart LLM + fal routing + revoke + cache + bug fix). See
  `v0.10b-bis-complete.md` § 4.
* **`#TEX-C` lib-side** — ✅ shipped in C-1. The "open v3 → save →
  re-open" verification clause is satisfied at the lib layer; the
  remaining "override_hint edit enters undo chain" clause requires
  a GUI editor and is part of D-1.
* **v0.10 C-2 / D-1** — *next thing the agent should pick up.*
  See "Outstanding work" §1 below.
* **User-side validation backlog** — see `USER-TODO.md`. A bunch of
  v0.9 polish items just landed (`#1c-async`, `#17b`, `#17c`, `#18`,
  `#19b`, `#20b`, all `#TEX-A` plumbing) and need a human-eyes pass
  on real hardware.

Reference: `v0.4-complete.md` · `v0.5-complete.md` · `v0.6-complete.md`
· `v0.7-complete.md` · `v0.8-complete.md` · `v0.9a-complete.md` ·
`v0.9b-complete.md` · `v0.10b-bis-complete.md` ·
`v0.10c1-complete.md`.

## Roadmap snapshot

| Ver  | Theme                                                   | Status    |
|------|---------------------------------------------------------|-----------|
| v0.4 | Meshing + height + export                               | shipped   |
| v0.5 | Headless CLI + CI infra                                 | shipped   |
| v0.6 | Palette editor + stroke undo + greedy meshing           | shipped   |
| v0.7 | Headless render + GUI feature-gate + palette CLI        | shipped   |
| v0.8 | Multi-angle preview + float window + onboarding + QoL   | shipped   |
| v0.9 | Robustness: autosave + GUI polish + Bevy trim + prefs   | A + polish shipped; **B + C pending** |
| v0.10 | AI texture MVP (mock → Fal → schema → preview → bake)  | A + B shipped; **C ready, D blocked on worker** |
| v1.0 | Release candidate: docs, icon, smoke matrix, tag        | not yet   |

### v0.10 phase detail

| Phase | Scope | Status |
|---|---|---|
| **A** | `texgen` lib module (trait, types, disk cache) + `MockProvider` (deterministic, offline) + `maquette-cli texture gen` | **shipped 2026-04-24** |
| **B** | `RustymeProvider` (LPUSH `texgen.gen` envelope, BRPOP the PNG back) + `--provider rustyme` + `texture revoke / purge` CLI + frozen worker contract (`docs/texture/rustyme.md`) + sonargrid-side worker roadmap (`docs/texture/rustyme-worker-roadmap.md`) | **shipped 2026-04-24** + **B-bis 2026-04-27 evening** (live integration with sonargrid `texgen-cpu` / `texgen-fal`, `image_b64` shape, profile env, foreign-reply bug fix). `#TEX-B` end-to-end ✅. |
| **C** | Project schema v4: per-project `model_description: String`; per-palette-slot `override_hint: Option<String>` + `texture: Option<TextureHandle>`; `TexturePrefs { view_mode, ignore_color_hint }`; serde forward / backward compat (`#[serde(default)]` on every new field, old `.maq` still opens); undo/redo covers `model_description` and `override_hint` edits as first-class edit events | **C-1 (lib) shipped 2026-04-27 evening**; C-2 (GUI undo wiring + autosave migration) lands with D-1 |
| **D-1** | GUI material panel: "What is this model?" single prompt + [Generate] + auto-derived per-slot hints (palette color + cell count + top/middle/bottom bias + adjacency) + Rustyme **Canvas group** fan-out (one task per non-empty slot) + toon shader optional base color texture (one shared seamless tile per slot; all cells of that color share UVs) + View toggle "Flat / Textured" | not yet — **the user-experience milestone**; needs C + worker |
| **D-2** | Per-slot `[regenerate]` + `[edit hint]` affordances in the palette list; re-uses D-1's single-task path (no group needed). Writes the new `override_hint` through the undo stack | not yet |
| **D-3** | _(deferred, may skip)_ 2D-canvas rectangle selection mode that regenerates only the slots whose cells fall inside the box. Explicitly deprioritised by user 2026-04-24 ("选中范围这个我理解可以没必要了") — the palette already carves the model into regions | deferred |
| **E** | glTF baking: per-palette material with `pbrMetallicRoughness.baseColorTexture`; single tile per slot, outline mesh kept compatible | not yet |
| **F** | docs (`docs/texture/`) + `USER-TODO.md` validation block + provider switching guide | partial — protocol + worker roadmap shipped, user guide pending |

After D-1 ships we re-evaluate whether E gets pulled in before v1.0
or deferred — D-1 alone is enough to *feel* whether AI textures
speed up the iteration loop, which is the core validation the user
wants.

## Outstanding work (agent, priority-ordered)

### Now — start here next session

1. **v0.10 D-1 (+ C-2 sub-tasks) — GUI material panel + autosave
   migration + EditHistory wiring**. The schema is in place
   (C-1, see `v0.10c1-complete.md`); the worker is verified
   (B-bis). What remains is putting it in front of the user.
   Concretely:
   * **GUI material panel.** Right side panel: "What is this
     model?" `TextEdit` bound to `ResMut<ProjectMeta>`, plus a
     `[Generate]` button. On click, fan-out one
     `texgen.gen` task per non-empty palette slot via Rustyme
     **Canvas group**, group-id collected from the chord
     callback the worker emits.
   * **Per-slot prompt derivation.** Palette colour (RGB) + cell
     count + top/middle/bottom bias + adjacency → augment the
     project-wide `model_description` into a per-slot prompt the
     worker actually sees. `override_hint` (when set) replaces
     this entirely.
   * **EditHistory generalisation (the C-2 piece).** Widen
     `bin/history.rs::PaintOp` into an `EditOp` enum with
     `Paint(...) | SetModelDescription { before, after } |
     SetOverrideHint { slot, before, after }`. The lib already
     hands you the `before` value: `Palette::set_override_hint`
     and `Palette::set_texture` return the previous value. One
     undo stack, three event types — keeps the user's mental
     model intact.
   * **Autosave migration.** Switch `autosave.rs::write_swap`
     and `session.rs` Save / SaveAs to
     `project::write_project_with_meta` (passing the
     `Res<ProjectMeta>`). Otherwise an autosave between two
     keystrokes will silently drop a freshly-typed
     `model_description`.
   * **Toon shader.** Optional `baseColorTexture` per
     palette-material; when `TexturePrefs::view_mode == Textured`,
     all cells of that colour share UVs into the per-slot tile.
     `Flat` / `Textured` toggle in the View menu.

### Blocked / external

2. **`#TEX-B` worker hardening.** The CPU lane is fully
   verified; FAL needs `FAL_KEY` set on the sonargrid host
   before fal lane stops timing out (Maquette already proved
   the routing + revoke path against an empty-key worker, see
   `v0.10b-bis-complete.md` § 4 / `USER-TODO.md` `#TEX-B-fal`).
   Not on Maquette's plate.
3. **v0.10 D-1** — *is* #1 above (renumbered now that the
   schema is in place and the worker is verified). The next big
   milestone whenever the agent has a free session.
4. **User validation pass** — `USER-TODO.md` has a stack of items
   freshly to-hand: `#1c` shape cycle / `#1c-async` async export /
   `#17b` PIP click / `#17c` PIP colour + axes / `#18` float pose
   memory / `#19b` zoom buttons / `#20b` event-driven render /
   `#21` autosave recovery / `#TEX-A` mock provider determinism /
   `#TEX-B` (after worker ships).

### Later — v0.9 closure

5. **v0.9 B — Bevy feature trim**. v0.7 gated the five extra GUI
   crates; Bevy itself still compiles with its default feature set.
   Audit `render / pbr / winit / animation / audio / gizmos / scene
   / text / gltf` and disable what Maquette doesn't use. Target:
   cold-build time drop ≥ 2 minutes, release binary < 25 MB. Record
   before / after in `v0.9-complete.md`. Risk: a feature we disable
   turns out to be transitively required by `bevy_egui` /
   `bevy_panorbit_camera` / `bevy_mod_outline`. Ship feature-by-
   feature, CI each step.
6. **v0.9 C — preferences file**. `~/.config/maquette/prefs.toml`
   (platform-appropriate via `dirs`) persists
   `MultiViewState.enabled`, `FloatPreviewState.floating`, brush
   height, and the recent-files list. Reads on startup, writes on
   quit. GUI-only; the CLI never touches it. **Also unlocks**:
   untitled-project autosave (deferred from v0.9 A) and startup
   auto-recovery via last-opened-path persistence.
7. **v0.9 D (stretch) — perf pass**. Profile a 32×32 canvas with
   column heights up to 8: paint-to-preview latency, mesh rebuild
   time, PIP render overhead. Budget: 60 fps on an M1 base with
   multi-view on. Record hot paths. Optimise only what's actually
   hot.

### Pre-v1.0 polish (after v0.9 + v0.10 close)

8. **App icon final size ladder** — user picks one of the four
   proposals in `docs/icons/proposals/`; agent generates `.icns`,
   `.ico`, full PNG ladder, wires into `Cargo.toml`. (`USER-TODO.md
   #26`.)
9. **README + user-guide pass** (`USER-TODO.md #25`).
10. **Smoke matrix** (`USER-TODO.md #27`) — macOS + Linux
    minimum, Windows community.
11. **Tag + push v1.0** (`USER-TODO.md #28`) — agent writes
    CHANGELOG, user runs `git tag` / push.

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
   render / palette {export,import} / texture {gen,revoke,purge}`
   shipping. New verbs require an entry in `COST_AWARENESS.md` and
   a matching integration test in `tests/cli.rs`.
7. **Palette is sparse** (v0.6) — deleting a color leaves its slot
   as `None` and future `add` reuses the hole. Project files are
   schema v3; v1 / v2 files load automatically. v4 is the next bump
   (v0.10 C).
8. **Meshing is greedy by default** (v0.6). Culled mesher retained
   as `build_color_buckets_culled` for regression tests only.
9. **Palette portability format** (v0.7) — `colors.json`, schema v1,
   hex-string colors with `null` for deleted slots. See
   `maquette/src/palette_io.rs`.
10. **Render projection** (v0.7) — isometric (yaw −45°, pitch ≈
    35.264°), flat Lambert shading, sRGB PNG. No outline baked into
    the preview PNG.
11. **GUI feature flag** (v0.7) — `gui` is a default feature; CI can
    build the CLI with `--no-default-features --bin maquette-cli`.
12. **Multi-view preview = PIPs, not splitters** (v0.8). Three
    orthographic (Top / Front / Side) PIPs, toggled by `View →
    Multi-view Preview` or `F2`.
13. **Float preview window = parallel, not replacement** (v0.8).
    Floating *adds* a second OS window; the main in-editor preview
    keeps rendering. Closing the OS window docks. One floating
    window at a time.
14. **Fit-to-Model shortcut = `F`** (v0.8). Frames the model AABB
    without changing orbit angle. `Cmd+R` is full reset. `F2`
    toggles multi-view PIPs.
15. **Block shape = enum on `Cell::shape`** (v0.9 polish). Right-
    click cycles `Cube ↔ Sphere`; Sphere is a placeholder shape
    rendered in the preview only — exporter / CPU rasterizer skip
    it (documented in `mesher.rs`). When export learns spheres,
    bump schema v4 → v5; until then a `.maq` with sphere cells
    will export an "incomplete" mesh and the GUI surfaces a toast.
16. **AI texture provider trait = sync** (v0.10 A). The GUI offloads
    via `AsyncComputeTaskPool::spawn(async move { provider.generate(...) })`
    so we never drag a tokio runtime through the lib. CLI calls
    straight-line.
17. **Texture cache key = SHA-256 over (prompt, seed, w, h, model)**
    + a `b"maquette-texgen-v1\x00"` domain separator (v0.10 A).
    Bumping the separator invalidates every cached texture; do that
    only when the *meaning* of "request" changes, not when adding
    optional fields with backward-compatible defaults.
18. **Texture granularity = one seamless 128² tile per palette slot**,
    not per cell (v0.10 design). Keeps prompt count = non-empty-slot
    count (typically 2–6); keeps Fal bill and visual consistency
    tight.
19. **Palette colour serves dual purpose** (v0.10 design). It is
    edit-layer primary data (Flat view always returns the painted
    bytes, AI textures sit in a second layer on top), AND it feeds
    the AI two ways: tone coordination hint (default ON, toggle via
    `TexturePrefs::ignore_color_hint`) and partition signal (always
    ON; same `color_idx` = same material = same tile).
20. **Async export pipeline** (v0.9 polish). Save dialog uses
    `rfd::AsyncFileDialog` (avoids macOS 26 NSSavePanel.runModal
    deadlock under winit). Actual export runs on
    `AsyncComputeTaskPool`; main thread shows a centered progress
    modal and stays responsive. `ExportOutcome` message is the
    sole channel from worker → GUI / CLI.
21. **Event-driven rendering = `WinitSettings::desktop_app()`**
    (v0.9 polish). Idle CPU ≈ 0% (Godot / Blender editor model).
    Smooth animations (Fit / Reset / PIP click) work because
    `camera::request_redraw_while_animating` pumps `RequestRedraw`
    while `PanOrbitCamera` is still interpolating.

## Working notes (mutable scratchpad)

### Crate / build

* Cargo's unit-of-compilation is the crate. With default features,
  both binaries compile the full Bevy tree. CLI-only builds skip
  the five GUI extensions (≈ 43 s saved on a cold build); trimming
  Bevy's own default features is the v0.9 B win.
* The `[[bin]]` table in `Cargo.toml` pins `maquette` to
  `required-features = ["gui"]` so `cargo build --no-default-features`
  only builds the CLI. `default-run = "maquette"` so `cargo run`
  still launches the GUI.
* `bevy_mod_outline` is bin-only. If lib code ever needs outline
  data, it must live in the inverted-hull baker under `export.rs`,
  not under `mesher.rs`.

### Lib core

* `project::apply_to_grid_and_palette` exists specifically for the
  GUI (mutates existing `ResMut` handles). CLI and tests use
  `project::read_project` which returns fresh `(Grid, Palette)`.
  Don't add a third load API without reading this note.
* `ExportPlugin` + `ExportRequest` remain in the lib. The GUI uses
  the Message; the CLI calls `export::write_with_options` directly.
  Same `ExportOptions` struct, so any new option reaches both
  surfaces.
* Palette is sparse (`Vec<Option<Color>>`). Use `Palette::get(idx)`,
  `iter_live()`, `live_count()`, `add()`, `update()`, `delete()`.
  Never index `palette.colors[i]` directly.
* `EditHistory::begin_stroke` / `end_stroke` live under the GUI
  binary. The data structure itself is headless-tested.
* `EditHistory::strokes_committed` is the autosave trigger
  (monotonic counter, observed in `autosave.rs`). Anything that
  mutates `Grid` / `Palette` outside the normal stroke flow must
  call `EditHistory::record` to bump the counter, otherwise
  autosave will only flush on next window-blur.
* Greedy meshing in `mesher.rs` is the shipping path.
  `build_color_buckets_culled` stays as the regression oracle.
  Both are **strictly cube-only** — see `is_cube_voxel`. Sphere
  cells go through `build_sphere_instances(grid)` and are GUI-only
  preview entities (one `Sphere(0.5)` per column, scaled by
  height). Exporter / CPU rasterizer skip sphere cells; v0.9 polish
  surfaces a toast when an export emits zero geometry due to
  sphere-only canvas.
* `render::rotate_iso` bakes an iso rotation (yaw −45°, pitch ≈
  35.264°). The rasterizer uses an edge-function scanline loop
  with a depth buffer; no mipmaps, no MSAA — flat-shade quads only.
* `palette_io` owns the `colors.json` schema. New per-slot metadata
  must bump the schema version there and keep `read_palette_json`
  accepting the old version as a subset.

### GUI

* v0.8 GUI additions (`multiview.rs`, `float_window.rs`, the
  Fit / Reset / Multi / Float toolbar, empty-state overlay) all
  live in `src/` — none in the lib. `camera::painted_bbox` is
  bin-local; promote to `maquette::geom::painted_bbox` only if the
  CLI `render` grows a `--fit` flag.
* v0.8 `FloatPreviewState` spawns a `Window` + `Camera3d` with
  `RenderTarget::Window(WindowRef::Entity(...))` (Bevy 0.18 ships
  this as a standalone component). `WindowClosed` is the message
  that fires when the OS close button is clicked; don't confuse
  with `WindowCloseRequested`. v0.9 polish: closing the float
  window now copies its camera pose back to the docked camera, so
  the user picks up where they left off.
* PIP cameras keep `is_active = false` on spawn and get flipped by
  `apply_enabled`. Toggling does not despawn entities — cheap, and
  keeps camera transforms stable across toggles.
* v0.9 polish: PIP click → main camera animates to that ortho
  angle (yaw / pitch only, projection stays perspective). Border
  colour codes the missing axis (Top = green, Front = blue, Side
  = red). Each PIP draws a small tinted-disc compass in its top-
  right corner.
* `notify.rs` toasts are GUI-only. Lib code surfaces messages via
  bus (see `ExportOutcome`). Don't take `ResMut<Toasts>` from lib
  code — breaks the Headless Invariant.
* `EguiGlobalSettings { auto_create_primary_context: false }` is
  intentional. The primary egui context is explicitly attached to
  `MainPreviewCamera` in `camera.rs`; without this guard a Startup-
  ordering race lets a PIP camera grab it and the entire UI
  collapses into a 180×180 square.
* `WinitSettings::desktop_app()` is the editor render loop.
  `camera::request_redraw_while_animating` pumps `RequestRedraw`
  every frame while `PanOrbitCamera` is converging — without it,
  Fit / Reset / PIP-click animations would step in single 5 s
  heartbeat ticks.

### v0.10 texgen

* `texgen.rs` is in the lib (Headless Invariant). Trait is sync;
  GUI offloads via `AsyncComputeTaskPool::spawn(async move { ... })`.
* `MockProvider::MODEL_ID == "mock-v1"`. Bump only when the visual
  output rule changes (it's part of the cache key).
* `RustymeProvider` is sync Redis (`redis = "0.27"`, no tokio).
  `task_id = uuid::v4`. Result list is `BRPOP rustyme:texgen:result`
  with a configurable timeout; revoke / purge go through the admin
  HTTP via `ureq`. Worker contract is frozen in
  `docs/texture/rustyme.md` — do not change payload shape without
  versioning the queue name.
* The disk cache (`default_cache_dir()`) honours `XDG_CACHE_HOME`
  first, then `$HOME/.cache/maquette/textures/`. Filenames are
  `<cache_key>.png` where `cache_key` is the 64-char SHA-256 from
  `TextureRequest::cache_key`. Same prompt + seed + model + size
  → same file → `texgen: cache hit` log line, no provider call.

### Async export

* `export_dialog.rs` (GUI-only) wraps `rfd::AsyncFileDialog`
  through `AsyncComputeTaskPool`. Polled every frame via
  `future::poll_once` + a per-frame `RequestRedraw` (otherwise the
  reactive event loop's 5 s heartbeat would stall the dialog). The
  rest of the pipeline is unchanged from v0.8.
* Concurrency guard: `ExportInFlight` resource. The File menu's
  `Export…` greys to `Export… (running)` while it's set; redundant
  `ExportRequest` events are dropped with an `export: ignoring
  request` log line.

### Swap / autosave (v0.9 A)

* The swap file is a plain `.maq` under the hood — not compressed,
  not a journal. Pinned by CLI tests
  (`cli_reads_swap_file_like_a_regular_project`,
  `cli_export_ignores_sibling_swap_file`). If a future version
  needs a compact swap, bump the constant or add a sibling
  `.maq.swap.v2`; don't silently change the on-disk shape.
* Untitled-project autosave is **explicitly deferred** to v0.9 C.
  The reason is the path: with no parent `.maq` file there's no
  natural location for the swap, and a global cache dir needs
  `dirs` (which v0.9 C will pull in for prefs anyway).

## How to find things

* User-facing manual verification → `docs/handoff/USER-TODO.md`.
* Per-version delivery notes → `docs/handoff/v0.X-complete.md`.
* AI texture wire protocol → `docs/texture/rustyme.md`.
* sonargrid worker implementation roadmap →
  `docs/texture/rustyme-worker-roadmap.md`.
* Cost / billing awareness for new CLI verbs →
  `docs/handoff/COST_AWARENESS.md`.
