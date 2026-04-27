#!/usr/bin/env bash
# Demonstrates mj_atlas with the most useful flag combinations.
#
# Usage:
#   examples/run_demo.sh
#
# Prerequisites:
#   - mj_atlas binary in PATH (or set $MJATLAS to the binary path)
#   - python3 with Pillow (for sprite generation)
set -euo pipefail

MJATLAS="${MJATLAS:-mj_atlas}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPRITES="$ROOT/examples/sprites"
OUT="$ROOT/examples/out"

echo "→ generating example sprites"
python3 "$ROOT/examples/gen_sprites.py" "$SPRITES" > /dev/null
mkdir -p "$OUT"

echo
echo "→ basic atlas (trim + POT)"
$MJATLAS pack "$SPRITES" -d "$OUT/basic" -o atlas --trim --pot

echo
echo "→ Godot 4 native .tres bundle (zero plugin)"
$MJATLAS pack "$SPRITES" -d "$OUT/godot" -o atlas --trim --pot --format godot-tres

echo
echo "→ polygon mesh + multi-component (concave / max-vertices=12)"
$MJATLAS pack "$SPRITES" -d "$OUT/polygon" -o atlas \
    --trim --pot --polygon --tolerance 1.5 \
    --polygon-shape auto --max-vertices 12

echo
echo "→ incremental: first run builds manifest"
$MJATLAS pack "$SPRITES" -d "$OUT/incremental" -o atlas --trim --pot --incremental --json | tail -8

echo
echo "→ incremental: second run is a cache hit (skipped: true)"
$MJATLAS pack "$SPRITES" -d "$OUT/incremental" -o atlas --trim --pot --incremental --json | tail -8

echo
echo "→ incremental + add new sprite triggers UV-stable partial repack"
python3 -c "
from PIL import Image, ImageDraw
img = Image.new('RGBA', (24, 24), (0,0,0,0))
ImageDraw.Draw(img).rectangle((1,1,22,22), fill=(140, 200, 80, 255))
img.save('$SPRITES/icon_added.png')
print('added: icon_added.png 24x24')"
$MJATLAS pack "$SPRITES" -d "$OUT/incremental" -o atlas --trim --pot --incremental 2>&1 | tail -4

echo
echo "→ inspect the incremental atlas (v0.3)"
# Use awk instead of `head` so the writer side of the pipe doesn't get SIGPIPE
# when the reader closes early (some shells map that to a non-zero exit and
# `set -e` would abort the demo).
$MJATLAS inspect "$OUT/incremental/atlas.png" | awk 'NR <= 10'

echo
echo "→ tag a sprite, then verify tags survive a re-pack (v0.3)"
$MJATLAS tag "$OUT/incremental/atlas.png" walk_01.png \
    --add walk,character --set-attribution "CC0 procedural" > /dev/null
$MJATLAS pack "$SPRITES" -d "$OUT/incremental" -o atlas --trim --pot --incremental --json > /dev/null
$MJATLAS tag "$OUT/incremental/atlas.png" walk_01.png --list

echo
echo "→ verify atlas integrity against manifest (v0.3)"
$MJATLAS verify "$OUT/incremental/atlas.png" --check-sources

echo
echo "→ done. outputs:"
ls -la "$OUT"/*/atlas.* 2>/dev/null | awk '{print "    "$0}' || true
