# Using Maquette exports in Blender 4

Blender is both a consumption and an authoring target — you might
just view the asset, or you might continue sculpting/rigging it.
Both paths are intended use.

## Import

1. `File → Import → glTF 2.0 (.glb / .gltf)`.
2. Pick `your_asset.glb` (or `your_asset.gltf` — either works).
3. Leave the default import options.

Blender recognizes `KHR_materials_unlit` and creates materials
using the "Background" shader, which renders exactly the flat
color you painted. No lighting setup needed to see the model.

## Outliner structure you will see

```
Maquette (Empty)
├── Body      — Mesh, N material slots (one per palette color)
└── Outline   — Mesh, 1 material slot  (only if baked)
```

The empty parent makes moving the whole asset around a single
operation.

## Rendering the baked outline

Blender's Eevee and Cycles both default to backface culling OFF,
which means the inverted-hull technique **doesn't** just work out
of the box the way it does in Godot and Unity. Two options:

### Option A — enable backface culling per material (recommended)

1. Select the **Outline** mesh.
2. Open the material properties, find the **Settings** panel.
3. Tick **Backface Culling** (Eevee) or **Backface Culling** under
   the material's **Surface** rollup (Cycles 4.2+).

Now the inverted hull renders as intended.

### Option B — use Freestyle instead

1. Delete the Outline mesh.
2. In the Render Properties, enable **Freestyle**.
3. Select **Body** → Object Properties → View Layer Freestyle →
   set line thickness and color.

Freestyle produces cleaner outlines than inverted-hull but only
works at render time, not in the viewport.

## Switching to Principled BSDF

If you want the asset to shade properly under scene lights:

1. Select the Body mesh.
2. For each material slot, open the Shading editor.
3. Replace the `Background` node with a `Principled BSDF` and
   wire the palette color into `Base Color`.

For a cel look without writing shaders, Blender's `ShaderNodeBsdfToon`
or the Freestyle path work well.

## Exporting onward (to another engine)

Blender's own glTF exporter round-trips our files well. If you
alter the mesh and re-export:

* Leave **Include → Lights** and **Include → Cameras** unchecked.
  We never ship those and neither should the re-export.
* **Transform → +Y Up** is the glTF convention; keep it on.

## Troubleshooting

**The whole mesh looks like a plain flat color.**
: That's unlit materials working correctly. If you want
  light-responsive materials, see "Switching to Principled BSDF"
  above.

**The outline mesh renders as a solid opaque shell around the body.**
: You forgot to enable backface culling on the Outline material
  (see Option A above).

**Vertex normals look inverted in the viewport ("inside-out" look).**
: That's specific to the Outline mesh — its normals are
  intentionally flipped. The Body mesh should never have this
  problem; if it does, file a bug.
