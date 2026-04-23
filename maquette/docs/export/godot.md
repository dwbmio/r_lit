# Using Maquette exports in Godot 4

## Import

1. Copy `your_asset.glb` into your Godot project's `res://` tree.
   Godot will pick it up and import it as a `.glb` scene.
2. Instance the scene into a 3D scene (drag it into the tree).

Godot's default glTF importer honours `KHR_materials_unlit`, so
each color primitive arrives already flat-shaded, matching the
Maquette preview's base colors (minus the cel bands — that's a
preview-only effect).

## Scene structure you will see

```
your_asset
├── Body      — MeshInstance3D, N surfaces (one per palette color)
└── Outline   — MeshInstance3D, 1 surface  (only if you baked one)
```

Each surface on **Body** has its own material slot. Override any
one of them to change that color's look in-engine without touching
the other colors.

## If you want a proper toon shader in Godot

Skip the baked outline on export, then in Godot:

1. On **Body**, set each surface's material to a `ShaderMaterial`
   using Godot's `canvas_item` / 3D toon shader of choice.
2. Add your own outline. Options:
   - Screen-space outline via a `post_process` shader (best
     quality, most work).
   - Re-bake the inverted-hull outline in Maquette and keep it.
     This is the easiest path and what we ship for.

## If you want to keep the baked outline

1. On **Outline**, keep the default standard material — it's
   already unlit black.
2. Make sure the Outline mesh's material has `cull_mode = Back`
   (Godot's default). That's what turns the inverted hull into a
   visible silhouette.
3. If you want to toggle the outline at runtime, just
   `.visible = false` on the Outline node.

## Troubleshooting

**The outline is invisible.**
: You probably have `cull_mode = Front` or `Disabled` on the
  Outline material. Set it back to `Back` (Godot default).

**Colors look slightly washed out.**
: Godot applies sRGB conversion on import. Our exports specify
  sRGB colors in `baseColorFactor`; if you've flipped Godot's
  project setting `rendering/textures/default_filters/srgb` you
  may see shifts. Revert to default.

**The mesh looks "rubbery" under light.**
: Something's using the lit PBR path instead of unlit. Double
  check each Body material's `shading_mode = Unshaded` (Godot
  derives this from the `KHR_materials_unlit` extension on
  import — it should be automatic).
