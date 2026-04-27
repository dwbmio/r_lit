# Incremental Packing

This document describes the manifest schema, the diff-classification logic, and the failure modes that fall back to a full repack.

## Why

Re-running a packer on a sprite library that mostly didn't change should be cheap. mj_atlas serializes enough state into a sidecar JSON file that subsequent runs can:

1. **Full-skip** when nothing changed (~10 ms — just hash inputs and bail out).
2. **Partial repack** when changes are local, preserving the exact `(x, y, rotated)` of every unchanged sprite (the **UV stability** invariant).
3. Fall back to **full repack** when the layout cannot satisfy the diff without growing.

UV stability is the key property: it means already-deployed game clients can swap in a new atlas PNG without rebaking baked UVs in shaders, prefabs, or saved scenes. Only the metadata sidecar gains new sprite entries.

## Manifest schema

The manifest lives next to the atlas at `<output_dir>/<output_name>.manifest.json`:

```json
{
  "version": 1,
  "tool": "mj_atlas 0.2.0",
  "options_hash": "5a3e…b71c",
  "input_root": "/abs/path/to/sprites",
  "sprites": {
    "icon_red.png": {
      "rel_path": "icon_red.png",
      "file_size": 1234,
      "mtime": 1730000000,
      "content_hash": "f3…", "polygon_hash": null,
      "trim_offset": [2, 3],
      "trimmed_size": [28, 28],
      "source_size": [32, 32],
      "atlas_idx": 0,
      "content_x": 4, "content_y": 4,
      "rotated": false,
      "alias_of": null
    }
  },
  "atlases": [
    {
      "image_filename": "atlas.png",
      "data_filename": "atlas",
      "width": 256, "height": 128,
      "image_hash": "9c…",
      "format": "json",
      "used_rects": [{"name": "icon_red.png", "x": 0, "y": 0, "w": 32, "h": 32, "rotated": false}],
      "free_rects": [{"x": 32, "y": 0, "w": 224, "h": 32}, {"x": 0, "y": 32, "w": 256, "h": 96}]
    }
  ]
}
```

**Three roles** the manifest plays:

1. **Cache key** — `options_hash` + per-sprite `content_hash` decide whether the cache is reusable.
2. **Layout state** — `used_rects` and `free_rects` (the maximal-rectangles set) are what partial repack uses to fit new sprites without disturbing existing ones.
3. **Asset registry foundation** — content-addressed view of the project, ready for higher-level tools (a "raw resource manager") to be built on top.

## Diff classification

For every input file present on disk, mj_atlas runs:

```
1. (file_size, mtime) match manifest entry?           → unchanged
   (no decode, no hash; this is the fast path)
2. else: decode pixels, compute SHA256 content_hash.
3. content_hash matches manifest?                     → unchanged (file was touched)
4. else if file dimensions match source_size?         → modified (in-place candidate)
5. else                                               → modified (size_changed)
```

For every manifest entry not present on disk: `removed`.

For every disk file not in manifest: `added`.

## Failure modes (full repack triggers)

A full repack is triggered when ANY of:

- `options_hash` mismatch (any pack-affecting option changed: `max_size`, `spacing`, `padding`, `extrude`, `trim`, `trim_threshold`, `rotate`, `pot`, `recursive`, `quantize`, `quantize_quality`, `polygon`, `format`, `polygon_shape`, `max_vertices`, `tolerance`)
- `--force`
- `input_root` differs (you ran the same `--output` against a different directory)
- An on-disk atlas PNG is missing or its `image_hash` changed (someone touched it externally — we can't trust the layout)
- A new or resized-modified sprite cannot fit in any free rect of any existing atlas
- An in-place modified sprite produced different trimmed dimensions after re-running trim (this turns into a relocation request — but we don't do relocate-with-shrink in v0.2; full repack handles it deterministically)

## CI integration

The `--json` output exposes cache state. Typical uses:

```bash
# Skip downstream steps when the atlas was a full cache hit
output=$(mj_atlas pack ./sprites -d ./out -o atlas --incremental --pot --json)
if [ "$(echo "$output" | jq -r '.skipped')" = "true" ]; then
    echo "atlas unchanged — skipping deploy"
    exit 0
fi
```

```bash
# Surface per-atlas cache state in build logs
mj_atlas pack ./sprites -d ./out -o atlas --incremental --pot --json | \
    jq -r '.files[] | "\(.image): \(if .from_cache then "cached" else "rebuilt" end)"'
```

## Excluded from the cache key

These options affect output filenames but NOT atlas pixels — they do not invalidate the cache:

- `output_dir` — only the location
- `output_name` — only the filename prefix

If you change either, the manifest path changes too (it's `<output_name>.manifest.json`), so a fresh cache is created at the new location. Old caches in old locations are left untouched.

## Determinism

Sprites are sorted by relative path before packing. Given identical inputs and options, mj_atlas produces byte-identical atlas PNGs and JSON sidecars across runs and platforms. This is what makes the `image_hash` integrity check reliable.

`--force --incremental` is the recommended way to verify determinism after refactors:

```bash
mj_atlas pack ./sprites -o atlas --incremental                # cache hit
hash1=$(shasum -a 256 ./sprites/atlas.png)
mj_atlas pack ./sprites -o atlas --incremental --force        # ignore cache
hash2=$(shasum -a 256 ./sprites/atlas.png)
[ "$hash1" = "$hash2" ] && echo "deterministic ✓"
```
