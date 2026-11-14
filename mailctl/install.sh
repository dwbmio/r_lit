#!/bin/bash
# mailctl 安装脚本（R2 / HFrog 制品）
# 用法：curl -fsSL https://r2.gamesci-lite.com/r_lit/mailctl/install.sh | bash
#       或  ./install.sh [--version 0.1.0] [--prefix ~/.local/bin]
set -euo pipefail

BINARY_NAME="mailctl"
R2_BASE="${R2_BASE:-https://r2.gamesci-lite.com/r_lit/mailctl}"
VERSION="${MAILCTL_VERSION:-latest}"
PREFIX="${PREFIX:-/usr/local/bin}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --prefix)  PREFIX="$2"; shift 2 ;;
    -h|--help) sed -n '2,4p' "$0"; exit 0 ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
done

detect_platform() {
  case "$(uname -s)_$(uname -m)" in
    Darwin_arm64)   echo "aarch64-apple-darwin" ;;
    Darwin_x86_64)  echo "x86_64-apple-darwin" ;;
    Linux_x86_64)   echo "x86_64-unknown-linux-musl" ;;
    Linux_aarch64)  echo "aarch64-unknown-linux-gnu" ;;
    *)              echo "unsupported"; exit 1 ;;
  esac
}

PLATFORM="$(detect_platform)"
if [[ "$VERSION" == "latest" ]]; then
  URL="${R2_BASE}/${PLATFORM}/${BINARY_NAME}.tar.gz"
else
  URL="${R2_BASE}/${VERSION}/${PLATFORM}/${BINARY_NAME}.tar.gz"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "下载 $URL"
curl -fsSL "$URL" -o "$TMP/pkg.tar.gz"
tar -xzf "$TMP/pkg.tar.gz" -C "$TMP"

if [[ -f "$TMP/SHA256SUMS" ]]; then
  ( cd "$TMP" && shasum -a 256 -c SHA256SUMS --quiet ) || { echo "SHA256 校验失败，拒绝安装" >&2; exit 1; }
  echo "SHA256 OK"
fi

install -m 0755 "$TMP/$BINARY_NAME" "$PREFIX/$BINARY_NAME"
echo "已安装：$PREFIX/$BINARY_NAME"
"$PREFIX/$BINARY_NAME" --version

cat <<'EOT'

下一步：
  1) 配置目录：~/.config/hermes-mail/accounts/<账号名>.toml（见 accounts.example/）
  2) 授权码入钥匙串：security add-generic-password -U -a <邮箱> -s mail-<账号名> -w '<授权码>'
  3) 自测：mailctl verify -a <账号名> && mailctl selftest -a <账号名> --wait 90
  4) 能力清单：mailctl capabilities --json
EOT
