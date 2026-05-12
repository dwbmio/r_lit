#!/usr/bin/env bash
# Build the three reference test sources used by the M2 grid search.
#
# Each source is 720x1080, 30 fps, 10 seconds, 8-bit YUV420P, encoded
# losslessly with libx264 (qp=0) so VMAF/SSIM/PSNR see the encoded
# distortion only — not source compression artifacts.
#
# Sources cover the three workload classes that drive different optimal
# NVENC parameters:
#
#   1. high-motion       — Mandelbrot zoom + kaleidoscope. Stresses motion
#                          search and B-frame referencing.
#   2. text-scroll       — vertical scrolling text overlay. Stresses
#                          high-frequency edges; AQ is critical here.
#   3. talking-head      — gradient + slow-moving overlay. Approximates
#                          static-bg + small-foreground content (closest to
#                          our actual gamereel-core use case).
#
# Idempotent — skips sources that already exist.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="$ROOT/tools/quality-eval/sources"
mkdir -p "$DEST"

W=720
H=1080
FPS=30
SECS=10

build_if_missing() {
  local label="$1"; shift
  local out="$DEST/${label}.mp4"
  if [[ -f "$out" ]]; then
    echo "  ✓ $label already built ($(stat -c%s "$out") bytes)"
    return
  fi
  echo "  building $label..."
  ffmpeg -hide_banner -loglevel error -y "$@" \
    -c:v libx264 -preset ultrafast -qp 0 -pix_fmt yuv420p -movflags +faststart \
    -t "$SECS" "$out"
  echo "    wrote $out ($(stat -c%s "$out") bytes)"
}

# 1) High motion: testsrc2 (animated color bars) overlaid with a strong
#    grain noise — high entropy *and* high temporal variation, which
#    starves NVENC's motion estimator (poor temporal prediction). Catches
#    encoders that depend too heavily on B-frames or skip-MBs to compress.
#    (Mandelbrot zoom would be more "interesting" content but is ~9 min/clip
#    to render losslessly, which makes the grid search hostile to iterate on.)
build_if_missing "high_motion" \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=${FPS}:duration=${SECS}" \
  -vf "noise=alls=20:allf=t+u"

# 2) Text scroll: small white text scrolling vertically over a low-entropy
#    blue background. AQ should give pixels near text edges more bits.
TEXT="VMAF QUALITY GRID SEARCH"
FONT="/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
[[ -f "$FONT" ]] || apt-get install -y fonts-dejavu-core >/dev/null 2>&1 || true
build_if_missing "text_scroll" \
  -f lavfi -i "color=c=0x103060:size=${W}x${H}:rate=${FPS}" \
  -vf "drawtext=fontfile=${FONT}:text='${TEXT}':fontcolor=white:fontsize=64:x=(w-text_w)/2:y=h-mod(40*t*${FPS}\,h+text_h)"

# 3) Talking head approximation: smooth gradient bg + a small disk drifting
#    slowly across center. Mostly static, so a good test of low-bitrate
#    behavior with B-frame heavy encoding.
build_if_missing "talking_head" \
  -f lavfi -i "gradients=size=${W}x${H}:rate=${FPS}:duration=${SECS}:c0=0x402040:c1=0x204060:c2=0x602030" \
  -vf "drawbox=x=(w-200)/2+50*sin(2*PI*t/4):y=(h-200)/2:w=200:h=200:c=0xffe0c0@0.85:t=fill"

# Sanity-print durations so we know they're all ${SECS}s.
echo
for f in "$DEST"/*.mp4; do
  dur=$(ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$f")
  echo "  $(basename "$f"): duration=${dur}s"
done
