#!/usr/bin/env bash
# M0 baseline: pure libx264 medium encoding throughput on a synthetic source
# matching the perf_main workload (720x1080, 30fps, 10s, RGBA→YUV420P).
#
# This establishes the *encoding-only* baseline that gamereel-core's main loop
# is currently bottlenecked by. M1 will replace libx264 with NVENC and the
# whole gamereel-core perf_main path will be measurable; until then, the
# library can't run end-to-end on Linux due to the hardcoded videotoolbox.
#
# Usage: baseline.sh [out_dir]
# Output: baseline.json with {wall_ms, fps, output_bytes, vmaf, ssim, psnr, ...}
set -euo pipefail

OUT_DIR="${1:-/tmp/movie-maker-baseline}"
mkdir -p "$OUT_DIR"

W=720; H=1080; FPS=30; SECS=10
REF="$OUT_DIR/ref.mp4"
DIST="$OUT_DIR/dist.mp4"

# Reference: lossless H.264 of the synthetic source (acts as ground truth).
# Using `testsrc2` for richer content (higher entropy than testsrc).
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=${FPS}:duration=${SECS}" \
  -c:v libx264 -preset ultrafast -qp 0 -pix_fmt yuv420p "$REF"

# Distorted: libx264 medium @ 6 Mbps — the "x264 baseline" from M1's spec.
START=$(date +%s%N)
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=${FPS}:duration=${SECS}" \
  -c:v libx264 -preset medium -b:v 6M -maxrate 8M -bufsize 12M \
  -pix_fmt yuv420p "$DIST"
END=$(date +%s%N)

WALL_MS=$(( (END - START) / 1000000 ))
FRAMES=$(( FPS * SECS ))
ENCODE_FPS=$(awk "BEGIN{printf \"%.2f\", $FRAMES * 1000 / $WALL_MS}")
OUT_BYTES=$(stat -c%s "$DIST")

QUALITY=$(/root/r_lit/video-generator/tools/quality-eval/run.sh "$REF" "$DIST")

# Merge throughput stats with quality stats.
jq --argjson wall_ms "$WALL_MS" \
   --argjson fps "$ENCODE_FPS" \
   --argjson bytes "$OUT_BYTES" \
   --arg encoder "libx264 medium" \
   --argjson width "$W" --argjson height "$H" \
   --argjson seconds "$SECS" \
   '. + {
     milestone: "M0",
     encoder: $encoder,
     width: $width, height: $height, duration_s: $seconds,
     wall_ms: $wall_ms, encode_fps: $fps, output_bytes: $bytes,
     realtime_factor: ($fps / 30.0)
   }' <<< "$QUALITY" > "$OUT_DIR/baseline.json"

echo "Baseline written to $OUT_DIR/baseline.json:"
cat "$OUT_DIR/baseline.json"
