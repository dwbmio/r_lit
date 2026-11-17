# mpreview

Mini interactive 3D model previewer. Same Godot `gl_compatibility` renderer your game ships, phone-frame viewport, animation switching, model stats, one-key screenshots — plus a headless Blender shot script sharing one camera preset (`cam.json`) for 1:1 Blender-vs-Godot comparison.

## Why not the existing tools

- `model-preview` skill (gd4): batch grid renders on a remote Blender. Good for bulk, no interaction, no Godot side.
- `mpreview`: single model, interactive, renders with the exact game renderer, and produces a matching Blender shot from the same camera preset.

## Run from source (no build needed)

```bash
# interactive viewer
/Applications/Godot.app/Contents/MacOS/Godot --path ./mpreview -- \
  --model ./beach_character_runtime_r2.glb --anim Idle --rot-x 0

# headless shot (needs a display or xvfb, not --headless: screenshots need a real renderer)
Godot --path ./mpreview --quit-after 200 -- \
  --model ./beach.glb --anim Walk --shot godot_shot.png --shot-frames 90

# matching Blender shot (same cam.json)
/Applications/Blender.app/Contents/MacOS/Blender --background --python blender_shot.py -- \
  ./beach.glb blender_shot.png cam.json --action Walk --size 405x720
```

## CLI flags

| flag | default | note |
|------|---------|------|
| `--model` | — | `.glb` path (absolute or `res://`), loaded at runtime via `GLTFDocument`, no import needed |
| `--anim` | first | animation to autoplay |
| `--rot-x` | 0 | extra X rotation in degrees (Quaternius-via-editor-import needs 90; runtime load stands at 0) |
| `--cam` | built-in | `cam.json` preset (Blender-space, z-up; viewer converts to Godot) |
| `--phone` | 405x720 | phone frame size |
| `--shot` / `--shot-frames` | — / 90 | auto screenshot + quit (automation) |
| `--compare` | — | show a reference PNG over the viewport (toggle with `C`) |

Keys: `Space` play/pause, `S` screenshot, `C` compare, `R` reset view, `Q` quit. Mouse drag orbits, wheel zooms.

## What the panel shows

File name/size, mesh + triangle counts, bone count, material count, animation count, model height (from skeleton span, works for collapsed bind poses too).

## Binary distribution

`r_lit` ships binaries via the intranet Jenkins `toolMap` + R2 + `install.sh`. `mpreview` is not cargo: build it with Godot export templates instead:

```bash
godot --headless --path ./mpreview --export-release "macOS" ./dist/mpreview-mac.zip
godot --headless --path ./mpreview --export-release "Linux" ./dist/mpreview-linux.zip
```

Linux CI shots need `xvfb-run` (screenshots require a real GL context; `--headless` uses a dummy driver and captures nothing). Exported size is ~40–70MB per platform.

## Conventions learned the hard way (keep these)

- `cam.json` is Blender-space; the viewer converts `(x,y,z)->(x,z,-y)`. Never hand-write `Transform3D` rows — verify with `-Z == fwd`.
- Editor-imported `.glb` instances of some files (this Quaternius one) lie flat in Godot and need `rotation.x = +90°`; the same file loaded at runtime via `GLTFDocument` stands at identity. Per-scene fix, never bake into the file.
- In GDScript the property is `rotation`, not `rotation_euler`.
