#!/usr/bin/env bash
# Pre-M3 sanity check: CPU SIMD sws_scale vs GPU scale_cuda for RGBA→YUV.
#
# Three paths benchmarked, all 720x1080 × 30 fps × 10 s = 300 frames:
#
#   A) CPU  : sws_scale RGBA→YUV420P (current movie-maker hot loop)
#   B) GPU  : hwupload_cuda + scale_cuda RGBA→NV12 (M3 endpoint:
#             one PCIe upload, scale on GPU, leave on GPU)
#   C) GPU+DL: B + hwdownload back to CPU (worst case if a downstream
#              consumer still wants the result in CPU memory)
#
# Source: testsrc2 lavfi → format=rgba (so input cost is identical
# across paths). Output: -f null - (just measures the filter chain,
# no encode/mux noise).
set -euo pipefail

W=720
H=1080
FPS=30
DUR=10
FRAMES=$((FPS * DUR))

run_path() {
  local label="$1"; shift
  local args=("$@")
  # /usr/bin/time captures wall + cpu separately
  /usr/bin/time -f "%e %U %S %P" -o /tmp/scale_path.t \
    ffmpeg -hide_banner -loglevel error -y -benchmark "${args[@]}" -f null - 2>/tmp/scale_path.ff
  local stats fps_speed wall user sys cpu_pct
  read -r wall user sys cpu_pct < /tmp/scale_path.t
  fps_speed=$(grep -oE 'speed=[0-9.]+x' /tmp/scale_path.ff | tail -1 | tr -d 'speed=x' || echo 0)
  printf "  %-12s  wall=%5.2fs  cpu_user=%4.2fs  cpu_sys=%4.2fs  cpu%%=%-5s  realtime=%sx  (fps=%s)\n" \
    "$label" "$wall" "$user" "$sys" "$cpu_pct" "$fps_speed" \
    "$(awk "BEGIN{printf \"%.0f\", $FRAMES/$wall}")"
}

echo "Reference: 720x1080 × 30 fps × 10 s = $FRAMES frames, RGBA → YUV/NV12, no encode"
echo

echo "[A] CPU sws_scale RGBA→YUV420P"
run_path "CPU/sws" \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=${FPS}:duration=${DUR}" \
  -vf "format=rgba,format=yuv420p"

echo
echo "[B] GPU hwupload_cuda + scale_cuda RGBA→0RGB→NV12 (M3 endpoint, no download)"
# scale_cuda only accepts 0rgb/0bgr/yuv420p/nv12/yuv444p/p010le/p016le —
# NOT rgba. We pre-convert RGBA→0RGB on CPU (alpha drop, mostly memcpy)
# before hwupload. M3 will replace this CPU pre-step with a custom
# CUDA kernel doing RGBA→NV12 in one shot.
run_path "GPU/upload+scale" \
  -init_hw_device cuda=cu:0 -filter_hw_device cu \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=${FPS}:duration=${DUR}" \
  -vf "format=rgba,format=0rgb,hwupload_cuda,scale_cuda=format=nv12"

echo
echo "[C] GPU upload + scale_cuda + hwdownload back to CPU"
run_path "GPU/round-trip" \
  -init_hw_device cuda=cu:0 -filter_hw_device cu \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=${FPS}:duration=${DUR}" \
  -vf "format=rgba,format=0rgb,hwupload_cuda,scale_cuda=format=nv12,hwdownload,format=nv12"

rm -f /tmp/scale_path.t /tmp/scale_path.ff
