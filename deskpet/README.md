# deskpet

An efficient, frameless, transparent, always-on-top **3D desktop mascot** built
with [Bevy](https://bevyengine.org). It lives in the **system tray / macOS menu
bar**; click the tray icon to show or hide the mascot. The mascot is a rigged,
idle-animated 3D model (generated from an image via Meshy / fal.ai), wanders
your desktop, can be dragged, hops when clicked, and lets clicks fall through
everywhere it isn't drawn. A small egui HUD provides quick controls.

## Features

- **Tray / menu-bar resident**: app launches hidden; click the tray icon to
  toggle the mascot. Right-click for a Show / Hide / Quit menu. On macOS it runs
  as an *accessory* app (menu-bar only, no Dock icon).
- **3D mascot from an image**: loads a rigged, idle-animated `.glb`
  (`assets/block.glb`, generated via fal.ai's hosted Meshy image-to-3d). Falls
  back to a built-in procedural slime if no `.glb` is present. Switchable
  between the two generated models (`block` / `blast`) from the HUD.
- **Transparent, frameless, always-on-top** window (`ClearColor(Color::NONE)` +
  `CompositeAlphaMode::PostMultiplied`).
- **Per-pixel-feel passthrough**: clicks on the transparent area fall through to
  whatever is behind; clicks on the mascot body / HUD are captured.
- **Collapsible egui HUD**: a small gear that expands into a semi-transparent
  panel (walk speed, wander toggle, Hop, Switch, Quit).
- Random idle wandering, click-to-hop, drag-to-move, right-click / Esc to quit.
- **Lazy rendering**: adaptive frame rate — ~60 fps while interacting, ~8 fps
  idle heartbeat when perched. CPU is ~0% when truly still.
- **Lean memory**: trimmed Bevy/egui features (no audio/picking/bevy_ui/gamepad/
  sysinfo), 128² mascot texture, MSAA off. ~120 MB RSS / ~290 MB incl. GPU.

## Controls

| Action | Result |
|--------|--------|
| Click tray / menu-bar icon | Toggle the mascot (show / hide) |
| Right-click tray icon | Show / Hide / Quit menu |
| Left click on mascot body | Greeting hop |
| Left drag on body | Move the mascot window |
| Right click on body / Esc | Quit |
| Gear (mascot's top-right) | Open the HUD panel |
| HUD "Switch" | Swap block ↔ blast mascot |
| Click on transparent area | Falls through to what's behind |

## Build & Run

No root `Cargo.toml` workspace — build inside this directory.

```bash
cd deskpet
cargo run            # dev
cargo run --release  # optimized
```

`assets/` is resolved relative to the working directory, so running the binary
directly (`./target/release/deskpet`) from the crate dir also finds the models.

## Generating / replacing the mascot

The mascot is a `.glb` under `assets/`. The ones shipped were made from a
reference image via **fal.ai's hosted Meshy 6 image-to-3d** (rigging + idle
animation), then their textures were downscaled. The pipeline scripts live in
`tools/`:

```bash
export FAL_KEY=<uuid>:<secret>          # fal.ai API key

# image -> rigged + idle-animated GLB (action 0 = Idle)
python3 tools/fal_meshy.py gen --image tools/block.png --out assets/block.glb --action 0

# shrink the embedded 2048² texture (16MB VRAM) down to 128² (~64KB)
python3 tools/glb_shrink_texture.py assets/block.glb --size 128
```

To use your own model: drop a humanoid rigged `.glb` at `assets/block.glb`
(scene 0, animation 0 = idle). No `.glb` → procedural slime fallback.

> Direct Meshy (`tools/meshy_image_to_3d.py` / a `meshy-animator` skill) needs a
> *paid* Meshy plan — the free plan returns HTTP 402 on task creation. fal.ai
> hosts the same Meshy model with pay-per-use billing (~$1.5/model), which is
> why the pipeline goes through fal.

## How passthrough works

`bevy::window::CursorOptions::hit_test` is a *whole-window* toggle, and once it
is `false` the window receives no Bevy cursor events. deskpet polls the
**OS-global cursor position** every frame (permission-free: CGEvent on macOS,
GetCursorPos on Windows) and flips `hit_test` based on whether the pointer is
over the mascot body or the HUD. Mouse buttons come from Bevy, since `hit_test`
is always on whenever a click matters. The window is also focused on approach so
macOS delivers events to it (a frameless overlay otherwise gets none).

## Memory / performance

| Lever | Effect |
|-------|--------|
| Mascot texture 2048² → 128² | VRAM 16 MB → 64 KB |
| Trim Bevy features (drop audio/picking/bevy_ui/gilrs/sysinfo) | smaller binary + fewer threads |
| Trim bevy_egui (`render` + `default_fonts` only) | drops clipboard/url + bevy_ui_render/picking |
| `Msaa::Off` | frees multisample targets |
| Adaptive frame rate | ~0% CPU idle |

Tunable constants are at the top of `src/main.rs` (`PET_W`, `WIN_H`, `HUD_W`,
`UI_SCALE`, walk/jump/frame-rate values).

## Platform Support

| Platform | Tray | Transparent | Passthrough | Status |
|----------|:----:|:-----------:|:-----------:|:------:|
| Windows  | yes (taskbar tray) | yes | yes | Supported |
| macOS    | yes (menu bar, no Dock) | yes | yes | Supported |
| Linux    | tray click events unsupported by `tray-icon`; Wayland can't self-position | best-effort | X11 flaky | Best-effort |

## License

MIT
