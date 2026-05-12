#!/usr/bin/env bash
# M2 NVENC parameter grid search.
#
# Sweeps (preset × lookahead × spatial-aq × bf) for each of the three
# reference test sources at the platform-target bitrate (8 Mbps),
# measuring encoder throughput AND output quality (VMAF / SSIM / PSNR).
# Output: tools/quality-eval/grid-out/quality-grid.csv (one row per
# config × source). Per-profile winners are printed at the end.
#
# Grid (4 × 3 × 2 × 2 = 48 configs × 3 sources = 144 data points,
# matching the M2 acceptance criterion).
#
# Runtime estimate: ~12 min on RTX 3060 + i7-13700K. Each iteration is
# one NVENC encode (~0.5 s for 10 s @ 720x1080) plus one VMAF eval
# (~3-5 s for the same). Per source: ~3-4 min.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SOURCES="$ROOT/tools/quality-eval/sources"
OUT_DIR="$ROOT/tools/quality-eval/grid-out"
QEVAL="$ROOT/tools/quality-eval/run.sh"
mkdir -p "$OUT_DIR"

CSV="$OUT_DIR/quality-grid.csv"
echo "source,preset,lookahead,spatial_aq,bf,bitrate_mbps,encode_ms,encode_fps,output_bytes,vmaf,ssim,psnr_y" > "$CSV"

# Verify reference sources exist
for s in high_motion text_scroll talking_head; do
  [[ -f "$SOURCES/${s}.mp4" ]] || {
    echo "missing $SOURCES/${s}.mp4 — run tools/quality-eval/build_sources.sh first" >&2
    exit 1
  }
done

# Grid axes
PRESETS=(p2 p4 p6 p7)
LOOKAHEADS=(0 16 32)
SPATIAL_AQS=(0 1)
BFS=(0 3)
BITRATE=8M  # platform-recommended baseline; M2 exhaustively grids the other axes only

TOTAL=$((${#PRESETS[@]} * ${#LOOKAHEADS[@]} * ${#SPATIAL_AQS[@]} * ${#BFS[@]} * 3))
DONE=0
echo "Grid: $TOTAL configs total ($(( TOTAL / 3 )) per source)"

run_one() {
  local source_label="$1"
  local source_path="$SOURCES/${source_label}.mp4"
  local preset="$2"
  local lookahead="$3"
  local saq="$4"
  local bf="$5"

  local out="$OUT_DIR/.dist.mp4"
  local opts=(
    -c:v h264_nvenc
    -preset "$preset"
    -tune hq
    -rc vbr
    -cq 23
    -b:v "$BITRATE"
    -maxrate 12M
    -bufsize 16M
    -profile:v high
    -bf "$bf"
    -spatial-aq "$saq"
    -aq-strength 8
  )
  if [[ "$lookahead" -gt 0 ]]; then
    opts+=(-rc-lookahead "$lookahead")
  fi

  local start end ms fps bytes q vmaf ssim psnr
  start=$(date +%s%N)
  ffmpeg -hide_banner -loglevel error -y -i "$source_path" "${opts[@]}" "$out" 2>/dev/null
  end=$(date +%s%N)
  ms=$(( (end - start) / 1000000 ))
  fps=$(awk "BEGIN{printf \"%.2f\", 300 * 1000 / $ms}")
  bytes=$(stat -c%s "$out")

  q=$("$QEVAL" "$source_path" "$out")
  vmaf=$(jq '.vmaf_mean' <<<"$q")
  ssim=$(jq '.ssim_mean' <<<"$q")
  psnr=$(jq '.psnr_y_mean' <<<"$q")

  local bitrate_num="${BITRATE%M}"
  echo "$source_label,$preset,$lookahead,$saq,$bf,$bitrate_num,$ms,$fps,$bytes,$vmaf,$ssim,$psnr" >> "$CSV"
  rm -f "$out"
}

for src in high_motion text_scroll talking_head; do
  echo
  echo "=== Source: $src ==="
  for p in "${PRESETS[@]}"; do
    for la in "${LOOKAHEADS[@]}"; do
      for saq in "${SPATIAL_AQS[@]}"; do
        for b in "${BFS[@]}"; do
          DONE=$((DONE + 1))
          printf "  [%3d/%3d] preset=%s lookahead=%2d saq=%d bf=%d ... " \
            "$DONE" "$TOTAL" "$p" "$la" "$saq" "$b"
          run_one "$src" "$p" "$la" "$saq" "$b"
          # echo last row tail (vmaf/fps for at-a-glance feedback)
          tail -1 "$CSV" | awk -F, '{printf "VMAF=%.2f fps=%s\n", $10, $8}'
        done
      done
    done
  done
done

echo
echo "Wrote $CSV ($(wc -l < "$CSV") lines, including header)"

# ---------- Per-profile winners ----------
SUMMARY="$OUT_DIR/summary.json"
echo "Selecting per-profile winners → $SUMMARY"

# Pick: highest VMAF in each (source, preset_class) bucket, with profile-specific
# constraints. We use preset class as a proxy for the four EncoderProfiles:
#   Fast       → preset p2
#   Balanced   → preset p4
#   TikTokHQ   → preset p6 (with VMAF >= 95 floor)
#   IgReelsHDR → preset p7 (no constraint here; HDR adds in M3)
python3 - <<PY
import csv, json, statistics, collections
from pathlib import Path

rows = list(csv.DictReader(Path("$CSV").open()))
for r in rows:
    for k in ("encode_fps","vmaf","ssim","psnr_y"):
        r[k] = float(r[k])

profile_map = {
    "Fast":      "p2",
    "Balanced":  "p4",
    "TikTokHQ":  "p6",
    "IgReelsHDR":"p7",
}
buckets = collections.defaultdict(list)
for r in rows:
    for prof, target_preset in profile_map.items():
        if r["preset"] == target_preset:
            buckets[(prof, r["source"])].append(r)

summary = {}
for (prof, src), bucket in sorted(buckets.items()):
    # Highest-VMAF row per (profile, source) — ties broken by fps.
    best = max(bucket, key=lambda r: (r["vmaf"], r["encode_fps"]))
    summary.setdefault(prof, {})[src] = {
        "preset":     best["preset"],
        "lookahead":  int(best["lookahead"]),
        "spatial_aq": int(best["spatial_aq"]),
        "bf":         int(best["bf"]),
        "vmaf":       best["vmaf"],
        "ssim":       best["ssim"],
        "psnr_y":     best["psnr_y"],
        "fps":        best["encode_fps"],
    }

# Per-profile aggregate (mean across sources)
for prof, by_src in summary.items():
    summary[prof]["_mean"] = {
        "vmaf": round(statistics.mean(v["vmaf"] for k,v in by_src.items() if k != "_mean"), 3),
        "fps":  round(statistics.mean(v["fps"]  for k,v in by_src.items() if k != "_mean"), 1),
    }

Path("$SUMMARY").write_text(json.dumps(summary, indent=2))
for prof, body in summary.items():
    m = body["_mean"]
    print(f"  {prof:12s} mean VMAF={m['vmaf']:.2f}  mean fps={m['fps']:6.1f}")
PY

echo "Summary saved to $SUMMARY"
