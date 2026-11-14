#!/bin/bash
# 校验已安装的 mailctl 二进制：签名 + SHA256。可作为 cron / 启动前自检。
set -uo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$DIR/mailctl"
SUMS="$DIR/SHA256SUMS"
rc=0

if [[ ! -x "$BIN" ]]; then
  echo "缺失：$BIN" >&2; exit 1
fi

if ( cd "$DIR" && shasum -a 256 -c "$SUMS" --quiet ) 2>/dev/null; then
  echo "SHA256  OK  ($(shasum -a 256 "$BIN" | awk '{print $1}'))"
else
  echo "SHA256  FAIL  二进制与清单不符（被替换？）" >&2
  rc=1
fi

if codesign --verify --strict "$BIN" 2>/dev/null; then
  echo "codesign OK"
else
  echo 'codesign 未签名或校验失败（ad-hoc 签名只能发现「换了二进制」，不防同用户重签）' >&2
fi

[[ -x "$BIN" ]] && "$BIN" --version 2>/dev/null | sed 's/^/version /'
exit $rc
