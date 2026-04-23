# Using Maquette exports in a game engine

Maquette ships a deliberately boring glTF file:

* One **Body** mesh node with one primitive per palette color you
  painted with. Each primitive uses a dedicated material, marked
  unlit via `KHR_materials_unlit` with the palette color as
  `baseColorFactor`.
* An optional **Outline** mesh node, an inverted-hull silhouette
  that any engine can render with default backface culling. No
  shaders required.

That is everything. No textures, no lights, no cameras, no bones,
no animations. If something more complicated shows up in your
engine, it's a bug in Maquette, not an engine feature you're
missing.

## Per-engine guides

* [Godot 4](./godot.md)
* [Unity 6 (URP / Built-in)](./unity.md)
* [Blender 4](./blender.md)

## Preview ≠ export — permanently

The cel shading and outlines you see inside Maquette are a
preview convenience. They are *never* embedded in the export. The
export ships geometry and an optional geometry-based outline. If
you want a richer toon look in your engine, apply your engine's own
toon shader to the Body mesh — the per-color material split means
each color already has its own material slot you can override.

## File layout

`your_asset.glb`
: Single binary file. Drop it into your engine. This is the
  default and the recommended format.

`your_asset.gltf` + `your_asset.bin`
: Text JSON + sibling binary buffer. Use this when you want the
  asset diffable in git or hand-editable. Both files must travel
  together; importers resolve `.bin` relative to the `.gltf`.

## What's *not* in the file

* **No textures.** Color lives in `material.baseColorFactor` per
  primitive.
* **No normal maps, roughness maps, metalness maps.**
* **No UV channel.** Primitives have `POSITION` + `NORMAL` only.
  If your engine warns about missing UVs, that's cosmetic — the
  unlit material doesn't need them.
* **No skeleton, no rig, no skinning.**
* **No animation.**
* **No lights or cameras.**

This is on purpose and it is the reason every engine can round-trip
the file without surprises.
