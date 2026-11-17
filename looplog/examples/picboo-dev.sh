#!/usr/bin/env bash
# picboo 真机联调：一条命令启动视图，来源由配置文件驱动。
#
#   ./examples/picboo-dev.sh
#
# 首次运行会把 examples/picboo-config.toml 复制成 ~/.config/looplog/config.toml。
# 之后所有增删改都在页面「⚙ 配置」里做（或直接编辑该文件）。
#
# 环境变量：
#   LOOPLOG_CONFIG    配置文件路径（默认 ~/.config/looplog/config.toml）
#   LOOPLOG_DB        数据库路径（不设则用 looplog 默认路径）
#   LOOPLOG_BIN       looplog 可执行文件（默认 PATH 里的 looplog）
#   PICBOO_SERVE      1 = 同时起 looplog serve（小程序 LokiClient 直连用，默认 1）
#   PICBOO_TOKEN      直连 token（真机必填，>=16 字符）
set -uo pipefail

BIN="${LOOPLOG_BIN:-looplog}"
CONFIG="${LOOPLOG_CONFIG:-$HOME/.config/looplog/config.toml}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DB_ARGS=()
[ -n "${LOOPLOG_DB:-}" ] && DB_ARGS=(--db "$LOOPLOG_DB")

command -v "$BIN" >/dev/null || { echo "✖ 找不到 looplog：$BIN" >&2; exit 2; }

if [ ! -f "$CONFIG" ]; then
  mkdir -p "$(dirname "$CONFIG")"
  cp "$HERE/picboo-config.toml" "$CONFIG"
  echo "✓ 已生成配置：$CONFIG"
  echo "  ssh / sudo 前置检查见 looplog README「远端 sudo」"
fi

PIDS=()
cleanup() {
  echo
  echo "→ 收尾"
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
trap cleanup INT TERM EXIT

if [ "${PICBOO_SERVE:-1}" = "1" ]; then
  if [ -n "${PICBOO_TOKEN:-}" ]; then
    "$BIN" "${DB_ARGS[@]}" serve --addr 0.0.0.0:3768 --allow-lan --token "$PICBOO_TOKEN" \
      >/tmp/looplog-serve.log 2>&1 &
    echo "  ✓ serve 0.0.0.0:3768（真机：VITE_LOKI_ENDPOINT=本机 LAN IP，VITE_LOKI_TOKEN=$PICBOO_TOKEN）"
  else
    "$BIN" "${DB_ARGS[@]}" serve --addr 127.0.0.1:3768 >/tmp/looplog-serve.log 2>&1 &
    echo "  ✓ serve 127.0.0.1:3768（仅模拟器；真机请设 PICBOO_TOKEN）"
  fi
  PIDS+=($!)
fi

echo "→ 打开视图（来源按 $CONFIG 的 autostart 拉起）"
"$BIN" "${DB_ARGS[@]}" --config "$CONFIG" view --open
