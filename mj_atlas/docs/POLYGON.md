# Polygon Mesh Output

`--polygon` makes mj_atlas output a per-sprite triangle mesh in addition to the rectangular `frame`. Engines can render the mesh instead of the rectangle, which removes transparent fragments and saves typically 30%+ of the per-sprite fragment cost on irregular shapes.

## Pipeline

```
sprite RGBA  ──►  alpha mask                                (trim_threshold)
              ──►  flood-fill connected components (8-conn)
              ──►  Moore-neighbor boundary trace per component
              ──►  Douglas-Peucker simplification (per component)
                   └─ optional: tolerance binary search to hit max_vertices
              ──►  shape mode: concave / convex / auto
              ──►  earcut triangulation (per component)
              ──►  concatenated vertices + triangle indices (with offsets)
```

Each connected component goes through the full pipeline independently and the
results are joined into one combined `vertices` + `triangles` set per sprite.

## Output JSON shape

```json
{
  "frames": {
    "multi_blob.png": {
      "frame": {"x": 4, "y": 4, "w": 96, "h": 56},
      "rotated": false,
      "trimmed": false,
      "spriteSourceSize": {"x": 0, "y": 0, "w": 96, "h": 56},
      "sourceSize": {"w": 96, "h": 56},
      "vertices":   [[5,4],[28,4],[28,28],[5,28],   [44,8],[80,8],[80,48],[44,48],   [20,42],[32,50],[20,54],[8,50]],
      "verticesUV": [[9,8],[32,8],[32,32],[9,32],   [48,12],[84,12],[84,52],[48,52], [24,46],[36,54],[24,58],[12,54]],
      "triangles":  [[0,1,2],[0,2,3],   [4,5,6],[4,6,7],   [8,9,10],[8,10,11]]
    }
  }
}
```

- `vertices` are in **sprite-local** coordinates (origin at the trimmed sprite's top-left corner). What you'd send to a vertex buffer.
- `verticesUV` are the same vertices in **atlas-PNG** coordinates (origin at atlas top-left). Used as texture sampling coords.
- `triangles` are flat indices into both arrays. Indices in different components are pre-offset, so this is a single triangle soup as far as the GPU cares.

## Shape modes

`--polygon-shape` controls per-component meshing:

| Mode      | Behavior                                                  | Verts | Overdraw |
|-----------|-----------------------------------------------------------|:-----:|:--------:|
| `concave` (default) | Keep the simplified concave outline                | high  | low      |
| `convex`  | Replace each component with its convex hull               | low   | medium   |
| `auto`    | Pick `convex` when `concave_area / hull_area ≥ 0.85`, else `concave` | mixed | mixed |

The 0.85 threshold for `auto` is empirical: typical character outlines have ratios of 0.6-0.75 (concave wins), simple icons usually have 0.9+ (convex wins, fewer verts at the same overdraw budget).

## Vertex budget (`--max-vertices`)

When you have a hard total-vertex budget per sprite (mobile draw calls, web targets), use `--max-vertices N`. mj_atlas runs Douglas-Peucker once at `--tolerance`, and if the total vertex count across components exceeds N it escalates the tolerance by a factor of 1.5 and retries. Capped at 8 iterations.

```
iter  tolerance  total_verts
0     1.5        47
1     2.25       28
2     3.375      18
3     5.0625     14   ✓ within budget
```

Caveat: each component has a hard floor of 3 vertices. A multi-component sprite with 5 disjoint blobs cannot go below 15 vertices total — escalating tolerance further has no effect at that point. The result will be ≥ floor; we don't drop components for the budget.

## Multi-component sprites

A common pattern: an icon strip rendered into one PNG with several disjoint blobs. mj_atlas treats each blob as its own component and meshes them independently:

```
input:                          output triangulation:
                                  ___       ____      _
  ●        █████      ▼          / | \     |    |    /|\
                                /__|__\    |____|   /_|_\
```

Three components → three separate triangle fans → joined into one
`triangles` array with offsets into one combined `vertices` array.

Components below 4 opaque pixels are filtered as noise (anti-aliased dust).

## Engine integration

The Godot SDK at `sdk/godot/addons/mj_atlas/` ships a GDScript loader that consumes this format and builds `MeshInstance2D` nodes. For other engines:

```glsl
// Vertex shader
attribute vec2 a_pos;       // from vertices
attribute vec2 a_uv;        // from verticesUV (divide by atlas size for [0,1])

void main() {
    v_uv = a_uv / u_atlas_size;
    gl_Position = u_mvp * vec4(a_pos, 0, 1);
}
```

For 3D engines doing 2D billboarding, the same data drops in as quad replacement.

## Inspecting meshes

The GUI's `--features gui` build has a **Mesh** checkbox in the preview canvas — toggle it to overlay the wireframe on top of the atlas. Useful for tuning `--tolerance` and shape modes:

```bash
mj_atlas pack ./sprites --polygon --polygon-shape auto --max-vertices 16
mj_atlas preview ./sprites/atlas.json   # then check "Mesh" in the toolbar
```
