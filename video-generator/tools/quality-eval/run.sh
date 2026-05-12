#!/usr/bin/env bash
# Quality evaluation: VMAF + SSIM + PSNR for a distorted video against reference.
#
# Usage: run.sh <reference.mp4|ref.y4m|ref.yuv> <distorted.mp4> [width height pix_fmt] -> stdout JSON
#
# Strategy:
#   - Decode both inputs to YUV420P 8-bit at the dist's frame rate / resolution
#   - Use Netflix `vmaf` binary (with built-in vmaf_v0.6.1 model + psnr feature)
#   - Use ffmpeg native `ssim` filter to obtain mean SSIM
#   - Emit one JSON object: {vmaf_mean, vmaf_min, psnr_y_mean, ssim_mean, frames}
#
# Requires: ffmpeg, ffprobe, vmaf (Netflix libvmaf 3.x standalone binary), jq.
set -euo pipefail

REF="${1:?reference path required}"
DIST="${2:?distorted path required}"

if ! command -v vmaf >/dev/null; then
  echo "{\"error\":\"vmaf binary not found in PATH; install libvmaf 3.x\"}" >&2
  exit 2
fi
if ! command -v jq >/dev/null; then
  echo "{\"error\":\"jq required for JSON aggregation\"}" >&2
  exit 2
fi

# Probe distorted video for canonical resolution / fps
read -r W H FPS_NUM FPS_DEN < <(ffprobe -v error -select_streams v:0 \
  -show_entries stream=width,height,r_frame_rate \
  -of csv=p=0 "$DIST" | awk -F'[,/]' '{print $1, $2, $3, $4}')
FPS=$(awk "BEGIN{printf \"%.6f\", $FPS_NUM/$FPS_DEN}")

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

REF_YUV="$TMP/ref.yuv"
DIST_YUV="$TMP/dist.yuv"

# Decode both to identical YUV420P 8-bit at distorted's W/H/FPS for fair compare.
ffmpeg -hide_banner -loglevel error -y -i "$REF" \
  -vf "scale=${W}:${H},fps=${FPS}" -pix_fmt yuv420p -f rawvideo "$REF_YUV"
ffmpeg -hide_banner -loglevel error -y -i "$DIST" \
  -vf "scale=${W}:${H},fps=${FPS}" -pix_fmt yuv420p -f rawvideo "$DIST_YUV"

VMAF_JSON="$TMP/vmaf.json"
vmaf -r "$REF_YUV" -d "$DIST_YUV" -w "$W" -h "$H" -p 420 -b 8 \
     --feature psnr --json -o "$VMAF_JSON" >/dev/null 2>&1

# SSIM via ffmpeg native ssim filter; the summary line is at INFO level on stderr.
# Format: `[Parsed_ssim_0 @ ...] SSIM Y:0.99 (30.36) U:... V:... All:0.99 (31.32)`
SSIM_MEAN=$(ffmpeg -hide_banner -loglevel info -y -i "$DIST" -i "$REF" \
  -lavfi "[0:v][1:v]ssim" -f null - 2>&1 \
  | grep -oP '\] SSIM .* All:\K[0-9.]+' | tail -1)
SSIM_MEAN="${SSIM_MEAN:-0.0}"

# Aggregate VMAF & PSNR from per-frame JSON
jq --arg ssim "$SSIM_MEAN" '
  {
    vmaf_mean:   ([.frames[].metrics.vmaf]    | add / length),
    vmaf_min:    ([.frames[].metrics.vmaf]    | min),
    vmaf_max:    ([.frames[].metrics.vmaf]    | max),
    psnr_y_mean: ([.frames[].metrics.psnr_y]  | add / length),
    ssim_mean:   ($ssim | tonumber),
    frames:      (.frames | length),
    libvmaf_version: .version
  }
' "$VMAF_JSON"
