#!/usr/bin/env bash
# gen-mc-blocks.sh — batch-generate a 12-block Minecraft-style
# texture set through Maquette's texgen pipeline.
#
# Two providers, one script:
#   --provider fal   (default)   real AI imagery via texgen-fal
#                                worker; needs FAL_KEY set on the
#                                sonargrid container.
#   --provider cpu               programmatic CPU + LLM-prompted
#                                noise textures via texgen-cpu;
#                                no Fal dependency, no $$ cost,
#                                results aren't real pixel art.
#
# Each block id matches `LocalProvider::blocks` so the resulting
# PNGs can be slot-bound 1:1 in the editor.
#
# Outputs:
#   /tmp/mc-blocks/<id>.png        — for visual review
#   ~/.cache/maquette/textures/    — populated automatically by
#                                    Maquette's disk cache
#
# Usage:
#   scripts/gen-mc-blocks.sh                       # fal, default seed
#   scripts/gen-mc-blocks.sh --provider cpu        # cpu smart fallback
#   scripts/gen-mc-blocks.sh --seed 42             # reproducible run
#   scripts/gen-mc-blocks.sh --width 512           # bigger output
#
# Configure once in your shell to avoid re-typing on each run:
#   export MAQUETTE_RUSTYME_REDIS_URL=redis://10.100.85.15:6379/0
#   export MAQUETTE_RUSTYME_ADMIN_URL=http://10.100.85.15:12121

set -euo pipefail

PROVIDER="fal"
SEED=1
WIDTH=256
TIMEOUT=120

while [[ $# -gt 0 ]]; do
  case "$1" in
    --provider) PROVIDER="$2"; shift 2 ;;
    --seed)     SEED="$2";     shift 2 ;;
    --width)    WIDTH="$2";    shift 2 ;;
    --timeout)  TIMEOUT="$2";  shift 2 ;;
    -h|--help)
      sed -n '2,30p' "$0" | sed 's/^# \?//'
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
    # so the worker hits Flux Schnell (cheap + fast: ~$0.003,
    # ~3-8 s).
    : "${MAQUETTE_RUSTYME_MODEL:=fal-ai/flux/schnell}"
    export MAQUETTE_RUSTYME_MODEL
    ;;
  cpu)
    export MAQUETTE_RUSTYME_PROFILE=cpu
    # cpu smart sends the prompt to GLM-4-Flash for parsing; the
    # block names we send are intentionally Chinese-flavoured so
    # the LLM picks up a sensible primary color + noise style.
    export MAQUETTE_RUSTYME_STYLE_MODE=smart
    unset MAQUETTE_RUSTYME_MODEL
    ;;
  *)
    echo "unknown --provider: $PROVIDER (expected fal | cpu)" >&2
    exit 2
    ;;
esac

OUT_DIR="/tmp/mc-blocks"
mkdir -p "$OUT_DIR"

# Always use the *debug* binary — `target/release` builds linger
# across protocol changes (texture.gen → texgen.gen, the v0.10
# B-bis rename was a recent example) and a stale release binary
# silently routes envelopes the worker no longer understands. The
# debug binary is rebuilt by `cargo build` on every meaningful
# code change. Build once if missing.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/maquette-cli"

if [[ ! -x "$BIN" ]]; then
  echo "▶ building maquette-cli (one-time)…"
  (cd "$ROOT" && cargo build --quiet --bin maquette-cli)
fi

run_cli() {
  "$BIN" "$@"
}

# 12 blocks. Ids match `LocalProvider::blocks` so the generated
# textures slot directly into the bundled palette mapping.
# Prompts are tuned for fal (real AI) + still parse meaningfully
# under cpu-smart's GLM. Seed is the same across all blocks so a
# repeated run is byte-identical (per Maquette's
# determinism contract).

# IDs and their prompts.
BLOCKS=(
  "grass:Minecraft block top texture, lush grass, hand-painted pixel art, top-down lighting, vibrant green, slight color variance, 16x16 stylised, seamless tile"
  "dirt:Minecraft block side texture, rich brown dirt with scattered pebbles and tiny roots, pixel art, 16x16 stylised, seamless tile"
  "stone:Minecraft block texture, rough grey cobblestone, weathered cracks, pixel art, 16x16 stylised, seamless tile"
  "wood:Minecraft block side texture, oak planks with straight wood grain, warm honey colour, pixel art, 16x16 stylised, seamless tile"
  "sand:Minecraft block top texture, fine desert sand with subtle ripples, warm gold, pixel art, 16x16 stylised, seamless tile"
  "brick:Minecraft block texture, weathered red brick wall, horizontal mortar lines, pixel art, 16x16 stylised, seamless tile"
  "ice:Minecraft block top texture, translucent pale blue ice with thin cracks, pixel art, 16x16 stylised, seamless tile, cool palette"
  "water:Minecraft block top texture, calm deep blue water with gentle ripples, pixel art, 16x16 stylised, seamless tile"
  "lava:Minecraft block top texture, glowing molten lava with bright orange and dark red cracks, pixel art, 16x16 stylised, seamless tile, emissive look"
  "amethyst:Minecraft block texture, amethyst crystal cluster, faceted purple gems, pixel art, 16x16 stylised, seamless tile"
  "bone:Minecraft block texture, ivory bone block surface with thin vertical channels, pixel art, 16x16 stylised, seamless tile"
  "moss:Minecraft block top texture, damp moss-covered stone, layered greens, pixel art, 16x16 stylised, seamless tile"
)

echo "▶ provider=$PROVIDER  seed=$SEED  size=${WIDTH}²  timeout=${TIMEOUT}s"
echo "▶ output dir: $OUT_DIR"
echo

OK_COUNT=0
FAIL_COUNT=0
START=$(date +%s)

for entry in "${BLOCKS[@]}"; do
  id="${entry%%:*}"
  prompt="${entry#*:}"
  out="$OUT_DIR/$id.png"
  printf "  %-10s … " "$id"
  if run_cli texture gen \
       --provider rustyme \
       --prompt "$prompt" \
       --seed "$SEED" \
       --width "$WIDTH" --height "$WIDTH" \
       --no-cache \
       -o "$out" \
       >/dev/null 2>&1; then
    if [[ -s "$out" ]]; then
      bytes=$(wc -c <"$out" | tr -d ' ')
      printf "✔  %s bytes\n" "$bytes"
      OK_COUNT=$((OK_COUNT + 1))
    else
      printf "✘  empty output\n"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
  else
    printf "✘  CLI errored\n"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done

ELAPSED=$(( $(date +%s) - START ))
echo
echo "▶ done in ${ELAPSED}s — $OK_COUNT ok, $FAIL_COUNT failed"
echo "▶ pngs in $OUT_DIR"
echo "▶ disk cache: ~/.cache/maquette/textures/"
echo
if [[ "$PROVIDER" == "fal" && "$FAIL_COUNT" -gt 0 ]]; then
  cat <<'TIP'
hint: fal lane needs FAL_KEY set on the sonargrid worker container.
      Status check:
        curl -s "$MAQUETTE_RUSTYME_ADMIN_URL/api/admin/queues" | jq '.[] | select(.name=="texgen-fal")'
      If `failed` is climbing while `succeeded` stays 0, ops still
      hasn't dropped FAL_KEY. Re-run this script with --provider cpu
      to validate the rest of the pipeline in the meantime.
TIP
fi
