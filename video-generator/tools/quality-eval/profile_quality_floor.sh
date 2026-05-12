#!/usr/bin/env bash
# M2 quality regression: enforce VMAF floor per EncoderProfile.
#
# This is the slow companion to `tests/profile_speed_floor.rs`. It runs
# each profile against the three reference sources (high_motion,
# text_scroll, talking_head) and asserts mean VMAF clears the per-profile
# threshold. Intended for the M2 acceptance pipeline (`benches/m2.sh`),
# not for `cargo test` — each VMAF eval takes 3–5 s.
#
# Floors (calibrated from the M2 grid search):
#   Fast       : VMAF ≥ 88
#   Balanced   : VMAF ≥ 92
#   TikTokHQ   : VMAF ≥ 95
#   IgReelsHDR : VMAF ≥ 95  (M2 == TikTokHQ; M3 will tighten)
#
# Bumps to thresholds should be matched by entries in docs/optimization-log.md.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SOURCES="$ROOT/tools/quality-eval/sources"
QEVAL="$ROOT/tools/quality-eval/run.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

declare -A FLOORS=(
  [Fast]=88
  [Balanced]=92
  [TikTokHQ]=95
  [IgReelsHDR]=95
)

# Profile → ffmpeg encoder args (must mirror movie-maker/src/encoder_profile.rs)
declare -A PROFILE_ARGS=(
  [Fast]="-c:v h264_nvenc -preset p2 -tune hq -rc vbr -cq 23 -b:v 6M -maxrate 9M -bufsize 12M -profile:v high -bf 0"
  [Balanced]="-c:v h264_nvenc -preset p4 -tune hq -rc vbr -cq 23 -b:v 8M -maxrate 12M -bufsize 16M -profile:v high -bf 3"
  [TikTokHQ]="-c:v h264_nvenc -preset p6 -tune hq -rc vbr -cq 21 -b:v 10M -maxrate 14M -bufsize 20M -profile:v high -bf 3 -b_ref_mode middle -rc-lookahead 16 -spatial-aq 1 -temporal-aq 1 -aq-strength 8"
  [IgReelsHDR]="-c:v h264_nvenc -preset p6 -tune hq -rc vbr -cq 21 -b:v 10M -maxrate 14M -bufsize 20M -profile:v high -bf 3 -b_ref_mode middle -rc-lookahead 16 -spatial-aq 1 -temporal-aq 1 -aq-strength 8"
)

PASS=0
FAIL=0
declare -A RESULTS

for profile in Fast Balanced TikTokHQ IgReelsHDR; do
  args="${PROFILE_ARGS[$profile]}"
  floor="${FLOORS[$profile]}"
  total=0
  count=0
  echo
  echo "=== Profile: $profile (floor: VMAF ≥ $floor) ==="
  for src_label in high_motion text_scroll talking_head; do
    src="$SOURCES/${src_label}.mp4"
    [[ -f "$src" ]] || { echo "  SKIP (missing source: $src)"; continue; }
    dist="$TMP/${profile}_${src_label}.mp4"
    ffmpeg -hide_banner -loglevel error -y -i "$src" $args "$dist" 2>/dev/null
    q=$("$QEVAL" "$src" "$dist")
    vmaf=$(jq '.vmaf_mean' <<<"$q")
    printf "  %-15s VMAF=%6.2f\n" "$src_label" "$vmaf"
    total=$(awk "BEGIN{print $total + $vmaf}")
    count=$((count + 1))
  done
  mean=$(awk "BEGIN{if($count>0){printf \"%.2f\", $total/$count}else{print 0}}")
  RESULTS[$profile]=$mean
  if awk "BEGIN{exit ($mean >= $floor) ? 0 : 1}"; then
    echo "  PASS: mean VMAF=$mean ≥ floor $floor"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: mean VMAF=$mean < floor $floor"
    FAIL=$((FAIL + 1))
  fi
done

echo
echo "================================================================"
echo "Summary:"
for profile in Fast Balanced TikTokHQ IgReelsHDR; do
  printf "  %-12s mean VMAF: %s (floor: %s)\n" "$profile" "${RESULTS[$profile]}" "${FLOORS[$profile]}"
done
echo "Passed: $PASS, Failed: $FAIL"
[[ $FAIL -eq 0 ]] || exit 1
