# Using Maquette exports in Unity 6

Unity's native glTF support landed in 2023 but is still
URP-biased; the instructions below cover URP (recommended) and
Built-in where they differ. HDRP works too but is overkill for a
low-poly asset and we don't test against it.

## Import

### URP or HDRP

1. Install **glTFast** from Unity Package Manager (`com.unity.cloud.gltfast`).
   Unity's built-in glTF pipeline is spottier than glTFast and
   doesn't always honour `KHR_materials_unlit`.
2. Drop `your_asset.glb` into the project. glTFast picks it up
   automatically.

### Built-in pipeline

1. Install **glTFast** the same way.
2. Before first import, open the asset's import settings → enable
   the "Legacy Standard Shader" fallback so materials resolve
   against Built-in's Standard shader.

## Scene hierarchy you will see

```
your_asset
├── Body      — MeshRenderer with N materials (one per palette color)
└── Outline   — MeshRenderer with 1 material   (only if baked)
```

Each palette color is its own material asset under
`your_asset.glb` in the Project view. Duplicate and override
freely — Maquette never regenerates these materials, only the
mesh data.

## Keeping the baked outline

The default URP Lit / Unlit materials come with `Cull Mode = Back`,
which is exactly what the inverted-hull technique needs. No action
required. If the outline disappears, check the Outline material's
render settings and restore backface culling.

To toggle at runtime: `outlineRenderer.enabled = false`.

## Swapping in a URP toon shader

Skip the baked outline on export, then in Unity:

1. Replace each Body material with a material using URP's Simple
   Lit or a community toon shader (e.g. `CodeAnimo/ToonShading`).
2. Keep each material's `_BaseColor` bound to the Maquette palette
   color — the glTF import already set it correctly, so the only
   thing you usually need to do is swap the shader graph.
3. Add an outline the Unity way: URP Renderer Feature "Render
   Objects" with an inverted-hull shader, *or* a full-screen
   outline post-process. Community shaders cover both paths.

## Troubleshooting

**Unity imports the file but shows a magenta mesh.**
: glTFast isn't installed, or URP / HDRP shader resolution failed.
  Re-import after installing glTFast; if still broken, inspect the
  console for the missing shader name and add it to
  `Edit → Project Settings → Graphics → Always Included Shaders`.

**Materials look "plastic".**
: Unity's built-in glTF importer (without glTFast) ignores
  `KHR_materials_unlit`. Install glTFast.

**The outline mesh is casting a shadow.**
: Disable "Cast Shadows" on the Outline MeshRenderer in the
  Inspector. It has no practical reason to cast a shadow.

**Colors look darker than the preview.**
: Unity's URP defaults to linear color space. Our baseColorFactor
  values are in sRGB per spec; glTFast converts correctly. If you
  see drift, check `Edit → Project Settings → Player → Other
  Settings → Color Space = Linear` (recommended for URP).
