# Manifest Subcommands (`inspect` / `diff` / `verify` / `tag`)

v0.3 promotes the manifest sidecar (`<output>.manifest.json`) from an internal cache file to a first-class artifact you can query, audit, compare, and annotate without ever repacking. Four read/write subcommands wrap it.

All four share the same path-resolution logic — pass any of:

- the manifest itself (`atlas.manifest.json`)
- a JSON file whose contents are a manifest, regardless of name (so `manifest_before.json` works)
- the atlas PNG (`atlas.png`)
- the sidecar metadata (`atlas.json` / `atlas.tpsheet` / `atlas.tres`)
- a multi-bin variant (`atlas_1.png` → `_1` is stripped, `atlas.manifest.json` is found)
- the directory containing them (errors if there are zero or multiple `*.manifest.json` files inside)

## `mj_atlas inspect`

Prints a human-readable summary, or full structured JSON with `--json`.

```text
Manifest: ./out/atlas.manifest.json
Tool:     mj_atlas 0.3.0
Inputs:   ./sprites
Sprites:  13  (13 unique, 0 aliases)
Atlases:  1   (50.5% occupancy across 32768 atlas px)
Options:  options_hash = 1e68efc1e85a…

[0] atlas.png  256x128  13 sprites  occupancy 50.5%  13 free rects  format=json

Tags:
     1  character
     1  walk

Sprites:
  badge_green.png   atlas=0 pos=(40,83) size=36x36
  walk_01.png       atlas=0 pos=(133,0) size=13x43  [character,walk]
  ...
```

The JSON form embeds the raw manifest plus a `summary` block so dashboards can read both compact stats and the full layout from one call.

## `mj_atlas diff`

Compares two manifests. Each sprite gets put into one of these buckets:

| Bucket | Meaning |
|---|---|
| `added` | only in B |
| `removed` | only in A |
| `pixel_change` | same name, same trimmed dims, different `content_hash` (in-place edit) |
| `resized` | same name, different trimmed dims (treated as remove+add by partial repack, triggers a full repack here) |
| `moved` | same name **and** content but different `(atlas_idx, x, y, rotated)` — this is a **UV-stability break**, meaning the layout changed between A and B |
| `tags_changed` | tags / attribution / source_url changed |
| `unchanged` | everything matches |

The top-line verdicts:

- `options_hash_changed`: did any pack-affecting option change between the two builds?
- `uv_stable`: was every unchanged sprite kept at its exact `(x, y, rotated)`?

```bash
# Typical CI use: fail the deploy if UVs broke (i.e. partial repack failed)
if [ "$(mj_atlas --json diff before.json after.json | jq -r .uv_stable)" != "true" ]; then
    echo "UV stability broken between builds — clients need a UV rebake."
    exit 1
fi
```

## `mj_atlas verify`

Re-hashes on-disk artifacts and checks against the manifest. Always verifies atlas PNG bytes; with `--check-sources` it also rehashes every sprite source file in the original `input_root`.

Exit code is 0 only when everything matches; non-zero when any drift is detected. Issues reported per file:

- atlas PNG missing
- atlas PNG `image_hash` drift (someone edited the PNG outside the tool)
- sprite source missing / decode-failed / dimensions changed / `content_hash` drift

This is the right hook for a CI step like "make sure the atlas we're about to publish actually corresponds to the sprite library the manifest claims".

## `mj_atlas tag`

Reads or edits the user-editable metadata on a sprite entry. Tags / attribution / source_url are NOT part of the cache key, so editing them never invalidates incremental.

```bash
# Add tags (deduplicated, sorted)
mj_atlas tag ./out/atlas.png hero_idle.png --add hero,character,idle

# Remove tags
mj_atlas tag ./out/atlas.png hero_idle.png --remove idle

# Set / clear attribution
mj_atlas tag ./out/atlas.png hero_idle.png --set-attribution "CC0 by Foo Bar"
mj_atlas tag ./out/atlas.png hero_idle.png --clear-attribution

# Source URL (e.g. opengameart, kenney, internal asset id)
mj_atlas tag ./out/atlas.png hero_idle.png --set-source-url https://opengameart.org/content/...

# List the metadata without touching anything
mj_atlas tag ./out/atlas.png hero_idle.png --list

# Bulk: omit the sprite name to apply to every sprite in the manifest
mj_atlas tag ./out/atlas.png --set-attribution "All assets © Acme Studios"
```

Tags survive across repacks. The `pack` subcommand reads any prior manifest before writing the new one and merges per-sprite metadata into the fresh manifest.

## Path resolution edge cases

- **Multi-bin atlases**: `atlas.manifest.json` is shared across `atlas.png`, `atlas_1.png`, `atlas_2.png` (pack writes one manifest per output, regardless of bin count). Pointing at any of those PNG paths resolves to the same manifest.
- **Renamed manifest copies**: `cp out/atlas.manifest.json before.json && mj_atlas diff before.json after.json` works — diff parses the file as JSON and accepts it because the contents are a valid manifest, even though the filename doesn't match `*.manifest.json`.
- **Wrong directory**: pointing `inspect` at a directory with no manifest fails with a clear error message; pointing at one with multiple manifests asks you to be specific.

## Forward compatibility

The manifest schema is stable at version 1. New optional fields (like `tags`, `attribution`, `source_url`) are added with `#[serde(default, skip_serializing_if = ...)]` so:

- v0.2 readers can still parse v0.3 manifests
- v0.3 readers parse v0.2 manifests (missing fields default to empty)
- Empty fields are NOT serialized, keeping the manifest compact

Future versions (v0.4 library refs, v0.5 remote-sync metadata) will follow the same pattern.
