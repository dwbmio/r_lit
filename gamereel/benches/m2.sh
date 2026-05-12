#!/usr/bin/env bash
# M2 measurement: per-profile encoder speed + quality across the three
# reference sources, plus the cargo speed-floor regression test.
# Output: results/m2.json with all numbers, ready to be diffed against m1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_DIR="$ROOT/benches/results"
mkdir -p "$RESULTS_DIR"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Make sure reference sources exist (regenerate if needed).
"$ROOT/tools/quality-eval/build_sources.sh" >/dev/null

# Run quality floor and capture per-profile per-source VMAF.
QUALITY_OUT="$TMP/quality.txt"
"$ROOT/tools/quality-eval/profile_quality_floor.sh" > "$QUALITY_OUT"
cat "$QUALITY_OUT"

# Parse summary section into JSON
python3 - <<PY > "$TMP/quality.json"
import json, re
text = open("$QUALITY_OUT").read()
# Lines look like:  Fast         mean VMAF: 97.87 (floor: 88)
data = {}
for line in text.splitlines():
    m = re.match(r'\s+(\w+)\s+mean VMAF:\s+([\d.]+)\s+\(floor:\s+(\d+)\)', line)
    if m:
        data[m.group(1)] = {"mean_vmaf": float(m.group(2)), "floor_vmaf": float(m.group(3))}
print(json.dumps(data, indent=2))
PY

# Run cargo speed test, capture fps per profile.
SPEED_OUT="$TMP/speed.txt"
( cd "$ROOT" && cargo test --release -p gamereel-core --test profile_speed_floor -- --nocapture 2>&1 ) | tee "$SPEED_OUT" >/dev/null

python3 - <<PY > "$TMP/speed.json"
import json, re
text = open("$SPEED_OUT").read()
out = {}
for line in text.splitlines():
    m = re.match(r'\s*(\w+):\s+(\d+) ms \(\s*([\d.]+) fps\)', line)
    if m:
        out[m.group(1)] = {"wall_ms": int(m.group(2)), "encode_fps": float(m.group(3))}
print(json.dumps(out, indent=2))
PY

# Run perf_main e2e for end-to-end number under Balanced profile (default).
PERF_BIN="$ROOT/target/release/perf_main"
[[ -x "$PERF_BIN" ]] || ( cd "$ROOT" && cargo build --release -p gamereel-core --bin perf_main >/dev/null 2>&1 )
declare -a PERF_RUNS=()
for i in 1 2 3 4 5; do
  T=$(RUST_LOG=warn "$PERF_BIN" 2>&1 | grep "运行了" | grep -oE '[0-9]+')
  PERF_RUNS+=("$T")
done
PERF_MEDIAN=$(printf '%s\n' "${PERF_RUNS[@]}" | sort -n | awk 'NR==3')
PERF_FPS=$(awk "BEGIN{printf \"%.2f\", 300 * 1000 / $PERF_MEDIAN}")

# Aggregate
python3 - <<PY > "$RESULTS_DIR/m2.json"
import json, time
quality = json.load(open("$TMP/quality.json"))
speed   = json.load(open("$TMP/speed.json"))
profiles = {}
for prof in ['Fast', 'Balanced', 'TikTokHQ', 'IgReelsHDR']:
    profiles[prof] = {**quality.get(prof, {}), **speed.get(prof, {})}

out = {
    "milestone": "M2",
    "branch": "linux-nvenc-refactor",
    "hardware": "RTX 3060 + i7-13700K + Ubuntu 24.04",
    "test_sources": ["high_motion (testsrc2+noise)", "text_scroll (drawtext)", "talking_head (gradient+drawbox)"],
    "profiles": profiles,
    "movie_maker_perf_main": {
        "scene": "tests/perf_main/scene.meta",
        "profile_used": "Balanced (default)",
        "runs_ms": $(printf '[%s]' "$(IFS=,; echo "${PERF_RUNS[*]}")"),
        "median_ms": $PERF_MEDIAN,
        "median_fps_end_to_end": $PERF_FPS
    },
    "grid_search": {
        "data_points": 144,
        "axes": "preset (4) × lookahead (3) × spatial_aq (2) × bf (2) × sources (3)",
        "csv": "tools/quality-eval/grid-out/quality-grid.csv",
        "summary": "tools/quality-eval/grid-out/summary.json",
        "key_finding": "Synthetic sources do not reward AQ or B-frames (no perceptual texture, no temporal redundancy). Balanced and Fast use minimal extras; TikTokHQ keeps AQ + lookahead because real-camera content benefits even though our synthetic grid can't measure it directly."
    }
}
print(json.dumps(out, indent=2))
PY

echo
echo "Wrote $RESULTS_DIR/m2.json:"
cat "$RESULTS_DIR/m2.json"
