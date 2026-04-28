#!/usr/bin/env bash
# gen-mc-blocks.sh — batch-generate a 12-block Minecraft-style
# texture set through Maquette's texgen pipeline, then derive
# mobile-friendly variants.
#
# ## Stages
#
# 1. **Generate master**  — Fal Schnell (or cpu-smart fallback)
#    produces a 256² PNG per block. Saved to
#    `/tmp/mc-blocks/master/<id>.png`.
#
# 2. **Resize for mobile** — `img_resize` scales each master down
#    to `--mobile-size` (default 64²) and writes
#    `/tmp/mc-blocks/mobile/<id>.png`.
#
# 3. **Compress to WebP**  — `cwebp` (libwebp) re-encodes each
#    mobile PNG to lossy WebP at `--webp-q` quality. Roughly
#    halves bytes versus PNG-8 at the same visual quality, and
#    every mobile GPU decodes WebP in hardware. Output:
#    `/tmp/mc-blocks/mobile/<id>.webp`.
#
# Why these stages:
# * Diffusion at 256² gives Fal Schnell enough headroom to hit
#   the prompt accurately; smaller targets degrade rapidly.
# * 64² is the Minecraft-mod sweet spot — large enough to read
#   per-block detail, small enough to slot into a 16x16 atlas at
#   1024² total without pressure on mobile GPU cache.
# * WebP at q=80 sits in the 1-3 KB range per block at 64², an
#   order of magnitude under the 49-148 KB raw Schnell PNGs.
#
# ## Prompt strategy
#
# Two improvements over the v1 of this script:
#
# * **Seamless tile** — every prompt now explicitly demands
#   `seamless tileable texture, edges blend continuously, no
#   border` so Fal's output can be tiled without visible seams
#   on a mesh face. Schnell isn't a seamless-tile-aware model
#   per se, but Flux schnell does respect the language strongly.
# * **Minecraft pixel-art style** — `pixel art, hand-painted,
#   blocky, low-poly stylised, 16x16 chunky aesthetic`. Pushes
#   the output toward the chunky readable style rather than a
#   photorealistic surface.
#
# ## Usage
#
#   scripts/gen-mc-blocks.sh                       # fal, 256² master, 64² mobile
#   scripts/gen-mc-blocks.sh --provider cpu        # offline fallback
#   scripts/gen-mc-blocks.sh --master-size 512     # higher-fidelity master
#   scripts/gen-mc-blocks.sh --mobile-size 32      # tinier mobile assets
#   scripts/gen-mc-blocks.sh --webp-q 70           # smaller WebPs (more loss)
#   scripts/gen-mc-blocks.sh --skip-mobile         # only generate masters
#
# Configure once in your shell to avoid re-typing on each run:
#   export MAQUETTE_RUSTYME_REDIS_URL=redis://10.100.85.15:6379/0
#   export MAQUETTE_RUSTYME_ADMIN_URL=http://10.100.85.15:12121

set -euo pipefail

PROVIDER="fal"
SEED=1
MASTER_SIZE=256
MOBILE_SIZE=64
WEBP_Q=80
TIMEOUT=120
SKIP_MOBILE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --provider)     PROVIDER="$2";     shift 2 ;;
    --seed)         SEED="$2";         shift 2 ;;
    --master-size)  MASTER_SIZE="$2";  shift 2 ;;
    --mobile-size)  MOBILE_SIZE="$2";  shift 2 ;;
    --webp-q)       WEBP_Q="$2";       shift 2 ;;
    --timeout)      TIMEOUT="$2";      shift 2 ;;
    --skip-mobile)  SKIP_MOBILE=1;     shift ;;
    -h|--help)
      sed -n '2,55p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

# Default rustyme wiring — override with env vars to point at a
# different cluster.
: "${MAQUETTE_RUSTYME_REDIS_URL:=redis://10.100.85.15:6379/0}"
: "${MAQUETTE_RUSTYME_ADMIN_URL:=http://10.100.85.15:12121}"
: "${MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS:=$TIMEOUT}"
export MAQUETTE_RUSTYME_REDIS_URL MAQUETTE_RUSTYME_ADMIN_URL \
       MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS

case "$PROVIDER" in
  fal)
    export MAQUETTE_RUSTYME_PROFILE=fal
    unset MAQUETTE_RUSTYME_STYLE_MODE
    # Critical: the sonargrid `texgen-fal` lua hook treats
    # `kwargs.model` as the literal Fal endpoint path. Maquette's
    # default `rustyme:texture.gen` (intended as a *cache-key*
    # discriminator on our side) lands as
    # https://fal.run/rustyme:texture.gen → 404. Override here
    # so the worker hits Flux Schnell.
    : "${MAQUETTE_RUSTYME_MODEL:=fal-ai/flux/schnell}"
    export MAQUETTE_RUSTYME_MODEL
    ;;
  cpu)
    export MAQUETTE_RUSTYME_PROFILE=cpu
    export MAQUETTE_RUSTYME_STYLE_MODE=smart
    unset MAQUETTE_RUSTYME_MODEL
    ;;
  *)
    echo "unknown --provider: $PROVIDER (expected fal | cpu)" >&2
    exit 2
    ;;
esac

OUT_DIR="/tmp/mc-blocks"
MASTER_DIR="$OUT_DIR/master"
MOBILE_DIR="$OUT_DIR/mobile"
mkdir -p "$MASTER_DIR" "$MOBILE_DIR"

# Always use the *debug* binary — `target/release` builds linger
# across protocol changes (texture.gen → texgen.gen) and a stale
# release binary silently routes envelopes the worker no longer
# understands.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/maquette-cli"
if [[ ! -x "$BIN" ]]; then
  echo "▶ building maquette-cli (one-time)…"
  (cd "$ROOT" && cargo build --quiet --bin maquette-cli)
fi

# Tools used in the post-processing stages. Only `cwebp` is
# strictly required (homebrew `libwebp`); `img_resize` is the
# in-house tool.
#
# Prefer the version built from this repo over whatever's on
# PATH — `/Users/admin/data0/dtool/img_resize` lags behind v0.2
# (it doesn't have the `r_resize` subcommand), so calling it
# from this script silently no-ops the resize step.
LOCAL_IMG_RESIZE="$ROOT/../img_resize/target/release/img_resize"
if [[ ! -x "$LOCAL_IMG_RESIZE" ]]; then
  echo "▶ building img_resize (one-time)…"
  (cd "$ROOT/../img_resize" && cargo build --release --quiet 2>/dev/null) || true
fi
if [[ -x "$LOCAL_IMG_RESIZE" ]]; then
  IMG_RESIZE_BIN="$LOCAL_IMG_RESIZE"
else
  IMG_RESIZE_BIN="$(command -v img_resize 2>/dev/null || true)"
fi
CWEBP_BIN="$(command -v cwebp 2>/dev/null || true)"
if [[ "$SKIP_MOBILE" -eq 0 ]]; then
  if [[ -z "$IMG_RESIZE_BIN" ]]; then
    echo "warn: img_resize not on PATH; mobile resize will be skipped" >&2
  fi
  if [[ -z "$CWEBP_BIN" ]]; then
    echo "warn: cwebp not on PATH (try: brew install webp); WebP step will be skipped" >&2
  fi
fi

# 12 blocks. Ids match `LocalProvider::blocks` so the generated
# textures slot directly into the bundled palette mapping.
#
# Prompts share a common suffix (seamless / pixel-art tags) and a
# unique block-specific opening; the suffix is what nudges Schnell
# into producing tileable, blocky output rather than a glossy
# photoreal pebbledash.
SUFFIX=", seamless tileable texture, edges blend continuously, no border, no text, no shadow, top-down orthographic view, hand-painted, pixel art, low-poly stylised, 16x16 chunky aesthetic, vibrant Minecraft colour palette"

BLOCKS=(
  "grass:Lush green grass top, slight color variance between blades, occasional flower or daisy pixel"
  "dirt:Rich brown dirt with scattered small pebbles and tiny roots"
  "stone:Rough grey cobblestone, weathered cracks, irregular block layout"
  "wood:Oak planks viewed end-grain, straight horizontal wood grain, warm honey tones"
  "sand:Fine desert sand with subtle wind ripples, warm gold tone, faint scattered grains"
  "brick:Weathered red brick wall, regular horizontal mortar lines, slight wear at edges"
  "ice:Translucent pale blue ice with thin white cracks, faint frost glitter"
  "water:Calm deep blue water with gentle ripples, hint of foam at edges"
  "lava:Glowing molten lava with bright orange channels and dark red volcanic crust, emissive look"
  "amethyst:Cluster of faceted purple amethyst crystals pointing outward, deep violet to lavender gradient"
  "bone:Ivory bone block surface with thin vertical channels, slightly cracked, neutral cream tone"
  "moss:Damp moss-covered stone surface, layered greens, occasional pebble peeking through"
)

echo "▶ provider=$PROVIDER  seed=$SEED"
echo "▶ master=${MASTER_SIZE}²   mobile=${MOBILE_SIZE}² + WebP q$WEBP_Q"
echo "▶ output: $OUT_DIR"
echo

OK_COUNT=0
FAIL_COUNT=0
RESIZE_COUNT=0
WEBP_COUNT=0
TOTAL_MASTER_BYTES=0
TOTAL_MOBILE_PNG_BYTES=0
TOTAL_MOBILE_WEBP_BYTES=0
START=$(date +%s)

for entry in "${BLOCKS[@]}"; do
  id="${entry%%:*}"
  prompt_head="${entry#*:}"
  prompt="${prompt_head}${SUFFIX}"
  master_path="$MASTER_DIR/$id.png"
  mobile_png="$MOBILE_DIR/$id.png"
  mobile_webp="$MOBILE_DIR/$id.webp"

  printf "  %-10s … " "$id"

  # 1) Generate master through Maquette → Rustyme → Fal/CPU.
  # Cache is left **on** so re-running this script with the same
  # seed/size/prompt is free (Maquette's disk cache short-circuits
  # before hitting Rustyme). Pass `--no-cache` is not needed —
  # cache miss only happens on the first run; explicitly running
  # with `--seed N+1` is the way to force fresh imagery.
  if "$BIN" texture gen \
        --provider rustyme \
        --prompt "$prompt" \
        --seed "$SEED" \
        --width "$MASTER_SIZE" --height "$MASTER_SIZE" \
        -o "$master_path" \
        >/dev/null 2>&1 \
     && [[ -s "$master_path" ]]; then
    master_bytes=$(wc -c <"$master_path" | tr -d ' ')
    TOTAL_MASTER_BYTES=$((TOTAL_MASTER_BYTES + master_bytes))
    OK_COUNT=$((OK_COUNT + 1))
    printf "master ✔ %5sB" "$master_bytes"
  else
    printf "✘ generate failed\n"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    continue
  fi

  # 2) Mobile resize through img_resize.
  if [[ "$SKIP_MOBILE" -eq 1 ]]; then
    printf "\n"
    continue
  fi
  if [[ -n "$IMG_RESIZE_BIN" ]]; then
    # img_resize r_resize has a quirky output convention: it writes
    # to *cwd* using only the input's file *name* (Path::new(f_name)
    # drops the dir component), not back to the input's directory.
    # Workaround: copy master → mobile_png, then cd into mobile_dir
    # before invoking; the tool then overwrites mobile_png in
    # place.
    cp "$master_path" "$mobile_png"
    if (cd "$MOBILE_DIR" && \
        "$IMG_RESIZE_BIN" r_resize \
            --rw "$MOBILE_SIZE" --rh "$MOBILE_SIZE" \
            "$id.png" \
            >/dev/null 2>&1) \
       && [[ -s "$mobile_png" ]]; then
      mobile_bytes=$(wc -c <"$mobile_png" | tr -d ' ')
      TOTAL_MOBILE_PNG_BYTES=$((TOTAL_MOBILE_PNG_BYTES + mobile_bytes))
      RESIZE_COUNT=$((RESIZE_COUNT + 1))
      printf "  png ${MOBILE_SIZE}² ✔ %5sB" "$mobile_bytes"
    else
      printf "  png ✘"
      rm -f "$mobile_png"
    fi
  fi

  # 3) WebP encode.
  if [[ -n "$CWEBP_BIN" && -s "$mobile_png" ]]; then
    if "$CWEBP_BIN" -q "$WEBP_Q" -m 6 "$mobile_png" -o "$mobile_webp" \
        >/dev/null 2>&1 \
       && [[ -s "$mobile_webp" ]]; then
      webp_bytes=$(wc -c <"$mobile_webp" | tr -d ' ')
      TOTAL_MOBILE_WEBP_BYTES=$((TOTAL_MOBILE_WEBP_BYTES + webp_bytes))
      WEBP_COUNT=$((WEBP_COUNT + 1))
      printf "  webp ✔ %5sB" "$webp_bytes"
    else
      printf "  webp ✘"
    fi
  fi
  printf "\n"
done

ELAPSED=$(( $(date +%s) - START ))
echo
echo "▶ done in ${ELAPSED}s"
echo "▶ $OK_COUNT generated · $RESIZE_COUNT resized · $WEBP_COUNT webp encoded · $FAIL_COUNT failed"
if [[ "$OK_COUNT" -gt 0 ]]; then
  printf "▶ master total: %s KB (avg %s KB/block)\n" \
    "$((TOTAL_MASTER_BYTES / 1024))" \
    "$((TOTAL_MASTER_BYTES / 1024 / OK_COUNT))"
fi
if [[ "$RESIZE_COUNT" -gt 0 ]]; then
  printf "▶ mobile png:   %s KB (avg %s KB/block)\n" \
    "$((TOTAL_MOBILE_PNG_BYTES / 1024))" \
    "$((TOTAL_MOBILE_PNG_BYTES / 1024 / RESIZE_COUNT))"
fi
if [[ "$WEBP_COUNT" -gt 0 ]]; then
  printf "▶ mobile webp:  %s KB (avg %s KB/block · %d%% of master)\n" \
    "$((TOTAL_MOBILE_WEBP_BYTES / 1024))" \
    "$((TOTAL_MOBILE_WEBP_BYTES / 1024 / WEBP_COUNT))" \
    "$((TOTAL_MOBILE_WEBP_BYTES * 100 / TOTAL_MASTER_BYTES))"
fi
echo "▶ pngs in $OUT_DIR (master/, mobile/)"
echo "▶ disk cache: ~/.cache/maquette/textures/"
