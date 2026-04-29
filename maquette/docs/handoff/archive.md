# Handoff Archive · maquette v0.2 → v0.9b

> **Purpose** — Compressed historical record for milestones whose full
> `vX.Y-complete.md` files were retired during the 2026-04-29 docs
> cleanup. Each entry preserves: version, ship date, theme, key
> decisions locked, and what the version delivered. For full pre-cleanup
> detail see git history (e.g.
> `git log --all -- maquette/docs/handoff/v0.4-complete.md`).
>
> **Live milestones** (still kept as standalone files because `NEXT.md`
> actively references them): `v0.10b-bis-complete.md`,
> `v0.10c1-complete.md`, `v0.10c2-blockmeta-complete.md`,
> `v0.10c3-block-composer.md`, `v0.10d1-complete.md`,
> `v0.10d1cd-complete.md`.

---

## v0.2 — Grid MVP + persistence (2026-04-23)

**Theme.** First playable: 2D paint canvas + real-time 3D preview, fixed
12-color palette, `.maq` JSON project file (schema v1), File menu
(New/Open/Save/Save As) via `rfd` native dialogs, status bar in user
domain (no "camera/light/mesh/UV" jargon).

**Key decisions.** Project format = pretty-printed JSON v1; canvas cap
= 64×64; UX vocabulary locked to "preview / turn / canvas / project /
color"; toon shader path **deferred** to v0.3 (decision point held);
`rfd` sync dialogs accepted on macOS native.

**Carried forward.** No undo/redo, no autosave, no export — all
explicitly scheduled later.

---

## v0.3 — Toon look + undo/redo + shortcuts + palette schema v2 (2026-04-23)

**Theme.** Stylized preview shipped: WGSL `ToonMaterial` (3-band cel
shading at 35% ambient floor) + `bevy_mod_outline` inverted-hull. 256-
deep per-cell undo/redo. Full keyboard map (`Cmd+N/O/S/Shift+S/Z/Y/R`,
digits 1–9). Palette schema v2 stores inline `Vec<RgbaPayload>`,
backward-compatible v1 reader.

**Key decisions.** Toon = custom WGSL + `bevy_mod_outline` (hybrid);
light direction hard-coded in shader (hidden from user, per North
Star); undo granularity = per-cell, no stroke grouping yet; shortcut
layer uses `egui::input_mut` + `KeyboardShortcut` (not Bevy
`ButtonInput<KeyCode>`); schema upper-version check rejects
`version > SCHEMA_VERSION`.

**Carried forward.** `Edit → Clear Canvas` not undoable; `1`–`9` only
covers 9 of 12 default-palette swatches.

---

## v0.4 — Meshing + height + glTF export (2026-04-23)

**Theme.** Performance + first export. Per-cell `CellCube` entities
replaced by **culled per-face per-color buckets** (one mesh per painted
palette color). Brush height slider 1..=8 (cap is a *product* decision,
locked in `COST_AWARENESS.md` — going higher invites a MagicaVoxel-style
fork). `MAX_GRID` bumped 64 → 128. `File → Export…` writes glTF/GLB v2
(JSON+BIN) via a hand-rolled writer; outline ships as **inverted-hull
geometry** (extrude along normals by `width_pct% × diagonal`, reverse
winding); per-engine guides under `docs/export/{godot,unity,blender}.md`.

**Key decisions.** Outline on export = inverted-hull only (shader
outlines are an engine concern, documented per-engine); outline width
= % of bounding diagonal, clamped 0..=10; height cap **= 8** (product
not technical); `gltf` crate added as dev-dep for round-trip
verification.

**Demoted to v0.6.** Palette editor UI, stroke-grouping undo, true
greedy-rectangle merging.

---

## v0.5 — Headless CLI + lib/bin split (2026-04-23)

**Theme.** Triggered by user request "maquette 支持 headless 么？纯 cli ci
场景很需要". Established the **Headless Invariant**: pure file-format /
mesher / exporter / palette code lives in `src/lib.rs`; GUI-only
(`ui/camera/scene/toon/history/session/preview_mesh`) lives in
`src/main.rs`. New `src/bin/maquette_cli.rs` ships verbs `export`,
`info`, `validate`. Test suite jumps to 13 lib + 6 CLI unit + 7 CLI
integration.

**Key decisions.** Binary names `maquette` (GUI) + `maquette-cli`
(headless); exit codes `0/1/2`; format inference order
`--format → extension → GLB`; color syntax `#RRGGBB` only; no
`assert_cmd` dev-dep, no checked-in `.maq` fixtures (build in-memory in
tests).

**Carried forward.** CLI release build still transitively pulls Bevy
GUI stack — proper feature-gate deferred to v0.7.

---

## v0.6 — Palette editor + stroke undo + greedy meshing (2026-04-23)

**Theme.** Three independent feature lines. (A) Right-click swatch →
context menu with inline `egui::color_edit_button_rgb` and Delete modal
(*Erase* | *Remap to:*); "+" slot HSL-shifts +45° from current color.
(B) `EditHistory.undo/redo` upgraded to `VecDeque<Stroke>` =
`Vec<PaintOp>`; one pointer-down→up gesture = one undo entry. (C)
Classic 2D greedy rectangle meshing per face direction; preview &
exporter both call `build_color_buckets` (greedy default), culled
oracle retained for tests + a CLI test asserts greedy is on the
shipping path.

**Key decisions.** Palette is **sparse** — `Vec<Option<Color>>`, deleted
slots stay `None` so live indices are stable across sessions
(unlocks future collaboration); delete fallback = erase by default;
schema bumped to v3 (deserializer still backward-compatible with v1
& v2).

**Carried forward.** Color edits don't enter `EditHistory` (no user
asked); palette import/export stays as v0.7 stretch.

---

## v0.7 — Headless render + GUI feature-gate + palette CLI (2026-04-26)

**Theme.** (A) `maquette-cli render` = pure-Rust isometric PNG
rasterizer (yaw −45°, pitch ≈ 35.264°, edge-function barycentric +
depth buffer, ~100 lines, only `png` for encoding). (B) `gui` Cargo
feature gates `bevy_egui / bevy_panorbit_camera / bevy_infinite_grid /
bevy_mod_outline / rfd`. **Honest measurement:** `--no-default-features
--bin maquette-cli` saves ~43 s on cold build, *not* the 5× initially
quoted — Bevy's own default features dominate the rest. Trimming Bevy
itself moved to v0.9. (D) `maquette-cli palette {export,import}` round-
trips `colors.json` (schema v1, hex strings + `null` for deleted slots).

**Key decisions.** Rasterizer is pure Rust + `png`, no shell-outs; if
platform drift hits goldens, switch to perceptual hash (don't loosen
tolerance); golden PNG **deferred** — v0.7 ships structural assertions
only (PNG magic + dims + non-bg pixel count + top-brighter-than-side
luminance + byte-reproducibility); feature flag = `gui` (short name
wins for CLI users); palette format is opaque-only (alpha excluded by
design).

---

## v0.8 — Multi-angle preview + float window + onboarding (2026-04-26)

**Theme.** Closes the original-brief "T-shape multi-face preview"
gap. (A) Three orthographic PIPs (Top/Front/Side) anchored bottom-
right, 180×180 logical px, share scene with main perspective camera,
toggle = `View → Multi-view Preview` / `F2`. (B) `Float` toolbar button
spawns secondary OS window (720×720) with own `PanOrbitCamera`,
initialised from main pose; close button mirrored back to docked.
(C) Empty-state hint painted by `egui::Painter` on any zero-painted
canvas. (D) `Fit to Model` (key `F`) reframes to AABB centre + 2.4×
half-diagonal radius (no angle change, distinct from `Reset`); new
floating top-right toolbar surfaces `Fit / Reset / Multi / Float`.

**Key decisions.** T-view = PIPs in corner (not splitter quadrants —
splitters add drag plumbing and per-quadrant state, deferred); both
windows render simultaneously when floating (matches dual-monitor use
case); PIPs = glance-only, don't accept mouse input; empty-state lives
in egui not bevy_ui (immediate-mode adjacency); Multi/Float state does
not yet persist across launches (waits on the prefs file in v0.9).

---

## v0.9 A — Autosave + crash recovery (2026-04-23)

**Theme.** "Crash mid-session = lose everything" eliminated. Sidecar
`<path>.maq.swap` (bit-identical to `.maq` format) is flushed on every
committed stroke + every `WindowFocused { focused: false }` event, no
debounce. Cleared on next successful Save / Save As. On `File → Open`,
if `swap_is_newer(path) == Some(true)`, a centered modal offers
Recover (load swap, mark dirty) or Discard (delete swap, keep
`.maq`). New lib API: `swap_path / swap_is_newer / write_swap /
remove_swap`, plus `EditHistory::strokes_committed() -> u64` (monotonic
across `clear()`). Two new CLI integration tests pin the contract: CLI
treats a swap path like a regular project, but ignores a sibling swap
when handed the `.maq`.

**Key decisions.** Swap suffix `.swap` (visible to Finder/grep, not a
hidden dotfile or temp dir); flush = stroke-committed + blur, no timer;
recovery has 2 outcomes only (no "keep both"); untitled-projects
autosave + startup auto-recover **deferred to v0.9 C** alongside
`~/.config/maquette/prefs.toml`; autosave success is invisible (no
toast).

**Tests at ship.** 87 passed (was 76; +11 new across `project::tests`,
`history::tests`, and the two CLI cases).

---

## v0.9 B (polish) + v0.10 A + v0.10 B — combined delivery (2026-04-27)

> Three milestones bundled because they share the async-compute
> task-pool plumbing introduced for the macOS 26 export deadlock fix.

### §1 — v0.9 polish & macOS 26 fix

* **Async export pipeline** (`src/export_dialog.rs` new). Replaced
  synchronous `rfd::FileDialog::save_file()` (which deadlocked under
  winit's main-thread loop on macOS 26+ via NSSavePanel.runModal) with
  `rfd::AsyncFileDialog` driven through `AsyncComputeTaskPool::spawn`.
  GUI shows centered "Exporting…" modal with `elapsed Xs` counter.
* **Brush floating HUD + Paint Mode** (Overwrite ↔ Additive, key `A`).
  Additive mode reuses `stroke_touched` so a 5-cell drag adds height
  at most once per cell.
* **Right-click cycles block shape** + **Backspace/Delete erases** +
  **height number badge** + **2-D sphere ring marker**. New `ShapeKind
  { Cube, Sphere }` with `#[serde(default)]` (full backward compat).
  Cube mesher routes everything through `is_cube_voxel`; spheres
  render as one `Sphere(0.5)` entity per column via
  `build_sphere_instances`.
* **PIP click → smooth main-camera animation** + **PIP border colour
  coding** (Top=green, Front=blue, Side=red) + **per-PIP axis compass
  disc**; world axes alpha lowered to ~55%.
* **Float window pose memory** (close button now copies float-pose
  back to docked).
* **Preview zoom −/+ buttons** in the floating toolbar; `+/=`/`-`
  shortcuts; clamp `[3, 120]`.
* **Event-driven rendering** — `WinitSettings::desktop_app()` swapped
  in for the implicit `game()`. Idle CPU drops to ~0%.
  `camera::request_redraw_while_animating` keeps animations smooth.
* **Modal centering pass** (every menu modal now `Align2::CENTER_CENTER`,
  non-draggable; fixes "click does nothing" reports caused by modals
  spawning under the side panel).
* **`WindowResizeConstraints { 1000 × 640 }`** as a layout floor.
* **Egui primary-context anchored to the main camera** via
  `EguiGlobalSettings { auto_create_primary_context: false }` +
  explicit `PrimaryEguiContext` — fixes a Startup-ordering race where
  a PIP could grab the primary context and collapse the UI into
  180×180.
* `bevy 0.18` material bind-group migration: `assets/shaders/toon.wgsl`
  uses `@group(#{MATERIAL_BIND_GROUP})` macro.

### §2 — v0.10 A · texgen module + MockProvider + disk cache + CLI

* New `maquette::texgen` lib module (sync trait `TextureProvider`).
  `MockProvider` is deterministic offline (SplitMix64 noise over a
  prompt-derived base color), capped at 1024×1024.
* Disk cache at `$XDG_CACHE_HOME/maquette/textures/` (fallback
  `$HOME/.cache/maquette/textures/`). Filenames =
  `<sha256_with_domain_separator>.png`. Cache miss = `Ok(None)`.
* `maquette-cli texture gen` subcommand
  (`--prompt/--seed/--width/--height/--model/--provider {mock,rustyme}/
  --no-cache/-o`) logs `wrote <path> (<bytes>, provider=<name>,
  cache_key=<hex>)`.

### §3 — v0.10 B · RustymeProvider + revoke/purge + frozen worker contract

* Sync Redis producer (`redis = "0.27"`, no tokio): `LPUSH` on
  `rustyme:texgen:queue` + `BRPOP` on `rustyme:texgen:result`,
  `TaskEnvelope`/`ResultEnvelope`/`TextureResult` types are
  module-private (workers should copy the JSON shape from
  `docs/texture/rustyme.md`, not depend on `maquette` as a library).
* CLI verbs `texture revoke --task-id <id>` and
  `texture purge --queue <name>` POST to sonargrid admin.
* Configuration via env: `MAQUETTE_RUSTYME_REDIS_URL`,
  `_ADMIN_URL`, `_QUEUE_KEY`, `_RESULT_KEY`,
  `_RESULT_TIMEOUT_SECS`.
* **`docs/texture/rustyme.md`** = authoritative wire protocol.
  **`docs/texture/rustyme-worker-roadmap.md`** documents the sonargrid
  worker plan (Stage 1 Echo / Stage 2 Fal.ai FLUX / Stage 3+ polish).

**Tests at tail of v0.9b.** 76 lib + 11 history + 6 export + 19 CLI =
**112 passed**, 1 ignored (`live_round_trip_against_running_rustyme` —
needs sonargrid + Stage 1 Echo). `cargo clippy --all-targets` clean.
`cargo build --no-default-features --bin maquette-cli` green
(Headless Invariant intact).

**Carried forward into v0.10b-bis →.** `#TEX-B` end-to-end verification
blocked on sonargrid Stage 1 Echo worker (producer side fully shipped
+ unit-tested). `v0.9 B Bevy feature trim` and `v0.9 C
~/.config/maquette/prefs.toml` both still on the v0.9 list, neither
started — see the live `v0.10b-bis-complete.md` and downstream files
for the picked-up state.
