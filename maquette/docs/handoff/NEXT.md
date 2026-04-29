# NEXT · maquette

**Status, 2026-04-28 (afternoon).** Wave order: v0.9 A
(autosave + crash recovery) 2026-04-23 → v0.9 polish + v0.10 A/B
2026-04-27 → v0.10 B-bis live integration with sonargrid
(`v0.10b-bis-complete.md`) → v0.10 C-1 schema v4 + ProjectMeta
(`v0.10c1-complete.md`) → v0.10 C-2 BlockMeta + LocalProvider +
HfrogProvider + Block Library GUI panel
(`v0.10c2-blockmeta-complete.md`) → **v0.10 C-3 just landed**:
Block Composer second window with iterative texgen prompt
console + Save Local Draft + Publish to Hfrog
(`v0.10c3-block-composer.md`). `BlockMetaSource::LocalDraft` and
`HfrogPublisher` are both live. Working tree is clean, **188
tests + clippy green** (146 lib + 11 history + 6 export + 25
CLI; was 183).

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
* **`#BLOCK` (dashboard item)** — ✅ shipped in C-2. CLI
  `block list/get/sync` works (CLI smoke ≈ 3 min); GUI right
  panel + slot binding works (GUI smoke ≈ 3 min). Real-network
  `block sync` against the production hfrog node
  (`https://hfrog.gamesci-lite.com`, the new default replacing
  the legacy `starlink.youxi123.com/hfrog` mount) returns
  `0 blocks` until the Block Composer has populated some — see
  C-3 below: the *Block Composer* now publishes them directly
  without needing a curl bootstrap.
* **`#COMPOSER` (new dashboard item)** — ✅ shipped in C-3. The
  user can `Window → New Block Composer…`, iterate on prompts
  through the cpu / fal / mock provider lanes, save the result
  as a local draft (instantly visible in the main library), or
  publish straight to hfrog. Two new 3-min smoke tests
  (`#COMPOSER-mock`, `#COMPOSER-publish`) in USER-TODO.md.
* **v0.10 D-1 — closed 2026-04-29.**
  A + B (Material drawer + ProjectMeta wiring + EditHistory LIFO +
  per-slot right-click `Generate texture →`) shipped 2026-04-28; see
  `v0.10d1-complete.md`. C + D shipped 2026-04-29: `Generate all`
  fan-out (3 lanes side-by-side, busy-guard dedupes duplicates;
  deliberately *not* using Rustyme chord callback to keep
  per-slot progressive UX) + `Flat / Textured` toggle now actually
  flips render output through a new `ToonMaterial.base_color_texture`
  + `texture_registry` cache_key → `Handle<Image>` resolver. See
  `v0.10d1cd-complete.md`. **D-1 milestone complete.**
* **User-side validation backlog** — see `USER-TODO.md`. A bunch of
  v0.9 polish items just landed (`#1c-async`, `#17b`, `#17c`, `#18`,
  `#19b`, `#20b`, all `#TEX-A` plumbing) and need a human-eyes pass
  on real hardware.

Reference: `v0.4-complete.md` · `v0.5-complete.md` · `v0.6-complete.md`
· `v0.7-complete.md` · `v0.8-complete.md` · `v0.9a-complete.md` ·
`v0.9b-complete.md` · `v0.10b-bis-complete.md` ·
`v0.10c1-complete.md` · `v0.10c2-blockmeta-complete.md` ·
`v0.10c3-block-composer.md` · `v0.10d1-complete.md` ·
`v0.10d1cd-complete.md`.

## Roadmap snapshot

| Ver  | Theme                                                   | Status    |
|------|---------------------------------------------------------|-----------|
| v0.4 | Meshing + height + export                               | shipped   |
| v0.5 | Headless CLI + CI infra                                 | shipped   |
| v0.6 | Palette editor + stroke undo + greedy meshing           | shipped   |
| v0.7 | Headless render + GUI feature-gate + palette CLI        | shipped   |
| v0.8 | Multi-angle preview + float window + onboarding + QoL   | shipped   |
| v0.9 | Robustness: autosave + GUI polish + Bevy trim + prefs   | A + polish shipped; **B + C pending** |
| v0.10 | AI texture MVP (mock → Fal → schema → preview → bake)  | A + B + C + **D-1 (all four sub-slices) shipped 2026-04-28 / 04-29**; D-2 polish + E glTF bake pending |
| v0.11 | Cloud-as-Backup: hfrog as user's "云硬盘", local stays source-of-truth, manual `Push to cloud` per project + per block + per texture, merged Recent list with `local` / `cloud` badges and mtime-LWW reconciliation | not yet — designed 2026-04-29 (this session); implementation gated on v0.10 D-1 finishing |
| v1.0 | Release candidate: docs, icon, smoke matrix, tag        | not yet   |

### v0.10 phase detail

| Phase | Scope | Status |
|---|---|---|
| **A** | `texgen` lib module (trait, types, disk cache) + `MockProvider` (deterministic, offline) + `maquette-cli texture gen` | **shipped 2026-04-24** |
| **B** | `RustymeProvider` (LPUSH `texgen.gen` envelope, BRPOP the PNG back) + `--provider rustyme` + `texture revoke / purge` CLI + frozen worker contract (`docs/texture/rustyme.md`) + sonargrid-side worker roadmap (`docs/texture/rustyme-worker-roadmap.md`) | **shipped 2026-04-24** + **B-bis 2026-04-27 evening** (live integration with sonargrid `texgen-cpu` / `texgen-fal`, `image_b64` shape, profile env, foreign-reply bug fix). `#TEX-B` end-to-end ✅. |
| **C** | Project schema v4 + block-meta content layer + block authoring tool | **C-1 (lib schema v4) 2026-04-27 evening · C-2 (BlockMeta + Library panel) 2026-04-28 morning · C-3 (Block Composer second-window + Save Draft + Publish to Hfrog) 2026-04-28 afternoon** · undo wiring + autosave migration deferred to D-1 |
| **D-1** | GUI material panel: "What is this model?" single prompt + [Generate] + auto-derived per-slot hints (palette color + cell count + top/middle/bottom bias + adjacency) + Rustyme **Canvas group** fan-out (one task per non-empty slot) + toon shader optional base color texture (one shared seamless tile per slot; all cells of that color share UVs) + View toggle "Flat / Textured" | **shipped** — A+B 2026-04-28 (`v0.10d1-complete.md`) + C+D 2026-04-29 (`v0.10d1cd-complete.md`). Generate-all uses N independent tasks rather than chord callback so the progressive "this slot is done now" UX stays intact. |
| **D-2** | Per-slot `[regenerate]` + `[edit hint]` affordances in the palette list; re-uses D-1's single-task path (no group needed). Writes the new `override_hint` through the undo stack | not yet |
| **D-3** | _(deferred, may skip)_ 2D-canvas rectangle selection mode that regenerates only the slots whose cells fall inside the box. Explicitly deprioritised by user 2026-04-24 ("选中范围这个我理解可以没必要了") — the palette already carves the model into regions | deferred |
| **E** | glTF baking: per-palette material with `pbrMetallicRoughness.baseColorTexture`; single tile per slot, outline mesh kept compatible | not yet |
| **F** | docs (`docs/texture/`) + `USER-TODO.md` validation block + provider switching guide | partial — protocol + worker roadmap shipped, user guide pending |

After D-1 ships we re-evaluate whether E gets pulled in before v1.0
or deferred — D-1 alone is enough to *feel* whether AI textures
speed up the iteration loop, which is the core validation the user
wants.

### v0.11 phase detail — Cloud-as-Backup

Vision (designed 2026-04-29): **hfrog 是用户的云硬盘**. Local
filesystem stays source-of-truth; cloud is an explicit, manual
*backup + cross-machine sync* layer. The product invariant is
"close the lid on machine A, open machine B, find your stuff
sitting where you left it" — but only if you remembered to hit
`Push to cloud`. No automatic sync, no conflict-resolution
voodoo: last-write-wins by mtime, surfaced honestly in the UI.

**Decisions locked in this session** (vs. the `AskQuestion`
prompts, for traceability):

| Decision           | Choice                                          |
|--------------------|-------------------------------------------------|
| Scope              | `.maq` projects + block library + texture cache (all three) |
| Cloud write timing | Local always first; explicit `Push to cloud` button per artifact (cloud is backup, not source-of-truth) |
| Merge semantics    | Single-row Recent list; mtime-LWW; badge marks active source; older sibling shown as small grey caption |
| Timing             | Gated on v0.10 D-1 finishing (texturing UX is the current main thread; cloud is the next big arc) |

The "1-2s probe and fall back" requirement from the user only
affects the **mode chip** in 0.11.A — the IO path itself stays
local-first regardless, so probe failures degrade UX (chip flips
to "Local mode · Try cloud →") but never block the user from
saving / opening files.

| Phase | Scope | Status |
|---|---|---|
| **A** | `cloud_status` module: HTTP probe of `MAQUETTE_HFROG_BASE_URL/api/artifactory/list?runtime=ping` with 1.5 s timeout on startup. `AppCloudMode { Online, Offline { last_error }, Probing }` resource. Status-bar chip ("☁ Cloud OK" / "○ Local mode · Try cloud →") with click-to-reprobe. **Does not** change any IO path; pure UI surfacing. | not yet — first slice |
| **B-projects** | hfrog protocol extension: new runtime `maquette-project/v1` carrying the `.maq` JSON as the artifact body (fits hfrog's "any binary" payload mode if available; if hfrog forces PNG for the S3 leg, fall back to a JSON-as-metadata + zero-byte PNG approach — research called out in B's first task). `File → Push to cloud` menu item + button on the project status bar; `HfrogPublisher::publish_project` peer of the existing block publisher. Pull side: `maquette-cli project sync` + GUI button. | not yet — second slice |
| **B-blocks** | Auto-pull `maquette-block/v1` on startup when mode == Online (currently the user has to click `Sync hfrog`). Block Composer's `Publish` already covers the push side — no protocol change needed. | not yet — third slice |
| **B-textures** | `maquette-texture/v1` runtime carrying generated PNGs keyed by `cache_key`. `Push to cloud` on a project sweeps every `PaletteSlotMeta::texture` it references and uploads the PNGs. Open-from-cloud reverses this: `cache_get` miss → fetch from hfrog before falling back to "regenerate". Cross-machine "open the same project on a different machine and the textures are already there" is the user-visible win. | not yet — fourth slice |
| **C** | Recent / Browse projects panel — Maquette's first homepage / dashboard. Merges local (`~/Documents/Maquette/*.maq` + recent-files history from the v0.9 C prefs file when that lands) with hfrog (`maquette-project/v1` list). Single row per `(slug, latest_mtime)` with a `[local] / [cloud] / [both ↻]` badge; older sibling shown grey. Open from cloud downloads the `.maq` body to `~/.cache/maquette/projects/<slug>.maq` and routes through the existing local Open path. | not yet — fifth slice |
| **D** | Polish: per-row "Open the local copy" / "Open the cloud copy" right-click items for the rare case where mtime ordering misleads; `--cloud-first` CLI flag for `maquette-cli project open` so headless / scripted runs follow the same rules; offline indicator in the title bar when working on a project that has a known-newer cloud sibling. | not yet — sixth slice |

Pre-requisites flagged early so they don't surprise us when we
get there:

* **hfrog "any binary" payload support** — research at the start
  of v0.11 B-projects. The current `HfrogPublisher` uploads PNG
  via S3 presigned URL; `.maq` is a JSON text. If hfrog's S3 leg
  is content-type-agnostic (likely — it's just `PUT <presigned>`
  with the bytes the client supplies), this is a one-line change.
  If hfrog hard-codes PNG validation, we either lobby for that to
  go away or pack `.maq` into a metadata field + zero-byte PNG.
* **Project slug** — currently a project's identity is its
  filesystem path. For cloud the canonical id is a slug. Likely
  derive from `.maq`'s file name on first publish, persisted into
  `ProjectMeta::cloud_slug: Option<String>` (schema v5 — minor
  bump, same backward-compat dance as v3 → v4).
* **prefs.toml prerequisite** — Recent list (phase C) reads from
  the v0.9 C recent-files history. v0.9 C is currently at "Later
  — v0.9 closure"; if we want C cleanly we may need to pull v0.9
  C in first or scope phase C to a "scan
  `~/Documents/Maquette/*.maq`" minimum. Decision deferred to the
  start of phase C.

## Outstanding work (agent, priority-ordered)

### Now — start here next session

v0.10 D-1 is **closed** as of 2026-04-29 (see
`v0.10d1cd-complete.md`). Three plausible next threads, ordered
by user-visible payoff:

1. **v0.11.A — cloud status chip (1 short session).**
   See "v0.11 phase detail" above for the full six-phase plan;
   phase A is the smallest first slice that introduces no
   architectural risk because it touches **no IO path**.
   Concretely:

   * `cloud_status.rs` Bevy plugin owning a startup probe
     (1.5 s timeout) and an `AppCloudMode { Online, Offline,
     Probing }` resource.
   * Status-bar chip on the right edge: "☁ Cloud OK" /
     "○ Local mode · Try cloud →". Click reprobes; updates the
     resource and the chip in-place.
   * Probe is cheap: `GET <hfrog>/api/artifactory/list?runtime=ping`
     with 1.5 s timeout. Any non-2xx, timeout, or network error →
     `Offline { last_error }`.
   * Zero changes to File / Save / Open paths. Phase B is where
     the cloud actually starts owning bytes.
   * Sets the user up for the bigger v0.11.B (`Push to cloud`
     button + `maquette-project/v1` runtime + cross-machine sync)
     by establishing the mode-state-machine vocabulary first.

2. **v0.10 D-2 — per-slot regenerate / edit-hint polish.**
   The texturing path ships with 3 surfaces (right-click → Generate
   texture · Material drawer → Generate all · `G` / `Shift+G`
   keyboard) but no per-slot "regenerate the same prompt" or
   "edit the prompt for this slot specifically" UI. A right-click
   submenu `Override hint…` that opens a small popover, plus a
   `Regenerate (same hint)` peer of the existing `Generate
   texture` items, would close that gap. Writes the new
   `override_hint` through the unified undo stack (the `MetaEdit`
   enum is already hospitable — add `SetOverrideHint { slot,
   before, after }`).

3. **v0.10 E — glTF baking with textured materials.**
   Now that Textured preview works, the export pipeline is the
   weak link: `.gltf` ships flat colour even when the user has
   AI-generated textures. Phase E adds `pbrMetallicRoughness.
   baseColorTexture` per palette-material with one tile per slot
   (the same `<cache_key>.png` files), preserves the outline mesh
   via the existing baked-inverted-hull approach, and round-trips
   through Blender / Unity / Godot test scenes. This is what
   actually closes the "pixel-art-mockup → game-engine asset"
   loop the v0.10 milestone is named for.

   E is the *most* user-visible of the three but also the
   biggest scope; v0.11.A is the lowest-risk warm-up and D-2 is
   the easiest "small polish" if the user wants to take a
   breath before the next big arc.

### Blocked / external

3. **`#TEX-B` worker hardening.** The CPU lane is fully
   verified; FAL needs `FAL_KEY` set on the sonargrid host
   before fal lane stops timing out (Maquette already proved
   the routing + revoke path against an empty-key worker, see
   `v0.10b-bis-complete.md` § 4 / `USER-TODO.md` `#TEX-B-fal`).
   Not on Maquette's plate.
4. **v0.10 D-1 finish** — ✅ closed 2026-04-29. All four sub-slices
   (A: meta wiring · B: per-slot generate · C: Generate all ·
   D: Textured render) shipped. See `v0.10d1cd-complete.md` for
   the C+D handoff.
5. **v0.11 B / C / D — cloud IO + Recent panel.** Designed
   2026-04-29; gated on D-1 closing out. See "v0.11 phase detail"
   above for the slicing rationale (`maquette-project/v1`
   runtime, `Push to cloud` button, mtime-LWW Recent list).
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
