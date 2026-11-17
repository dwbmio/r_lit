# Picboo-specific looplog serve & SSH tunnel helpers.
# Usage: just --justfile looplog/examples/picboo.just looplog-prod

set dotenv-load

#   just -f looplog/examples/picboo.just looplog-prod                   # random token
#   just -f looplog/examples/picboo.just looplog-prod token=<16+chars>  # fixed token
#   LOOPLOG_NO_OPEN=1 just -f looplog/examples/picboo.just looplog-prod
[doc("生产 picboo：起 serve + 打开视图，并打印 client 构建命令")]
looplog-prod token="":
    #!/usr/bin/env bash
    set -euo pipefail
    ROOT="$(cd "{{justfile_directory()}}/../.." && pwd)"
    HOST_TARGET="$(rustc -vV | awk '/^host:/ {print $2}')"
    BIN="$ROOT/looplog/out/$HOST_TARGET/looplog"
    CONFIG="$HOME/.config/looplog/config.toml"
    if [ ! -x "$BIN" ]; then
      echo "→ 构建 looplog"
      (cd "$ROOT/looplog" && cargo build --release)
      mkdir -p "$(dirname "$BIN")"
      cp "$ROOT/looplog/target/release/looplog" "$BIN"
    fi
    if [ ! -f "$CONFIG" ]; then
      mkdir -p "$(dirname "$CONFIG")"
      cp "$ROOT/looplog/examples/picboo-prod.toml" "$CONFIG"
      echo "✓ 生成配置 ${CONFIG}（之后在页面「⚙ 配置」里改）"
    fi
    TOKEN_FILE="$HOME/.config/looplog/serve.token"
    TOKEN="{{token}}"
    if [ -z "$TOKEN" ] && [ -f "$TOKEN_FILE" ]; then TOKEN="$(cat "$TOKEN_FILE")"; fi
    if [ -z "$TOKEN" ]; then
      TOKEN="$(openssl rand -hex 12)"
      mkdir -p "$(dirname "$TOKEN_FILE")" && printf '%s' "$TOKEN" > "$TOKEN_FILE" && chmod 600 "$TOKEN_FILE"
    fi
    if lsof -nP -iTCP:3768 -sTCP:LISTEN >/dev/null 2>&1; then
      echo "· 3768 已在监听，复用现有 serve"
    else
      "$BIN" serve --addr 0.0.0.0:3768 --allow-lan --token "$TOKEN" >/tmp/looplog-serve.log 2>&1 &
      SERVE_PID=$!
      trap 'kill "$SERVE_PID" 2>/dev/null || true' EXIT INT TERM
      sleep 0.6
      echo "✓ serve 0.0.0.0:3768（token=${TOKEN}）"
    fi
    LAN="$(ipconfig getifaddr en0 2>/dev/null || true)"
    [ -n "$LAN" ] || LAN="<本机局域网IP>"
    echo
    echo "client 日志回流（域名对齐，token 取 ${TOKEN_FILE}）："
    echo
    echo "  # loc 档 · 模拟器（127.0.0.1 就是 Mac，不经隧道）"
    echo "  cd hybrix/hybrix/taro-spike && \\"
    echo "    VITE_LOKI_ENABLE=1 VITE_LOKI_ENDPOINT=http://127.0.0.1:3768 \\"
    echo "    VITE_LOKI_TAG=picboo-client VITE_LOKI_TOKEN=$TOKEN VITE_LOKI_SAMPLE_RATE=1 \\"
    echo "    pnpm build:weapp:loc"
    echo
    echo "  # dev 档 · 模拟器/真机（走 dev-hs 子路径，需微信白名单已含 dev-hs）"
    echo "    VITE_LOKI_ENDPOINT=https://dev-hs.picboo.ink/_looplog"
    echo
    echo "  # prod 档 · 临时调试（线上包不要带）"
    echo "    VITE_LOKI_ENDPOINT=https://prod-c.picboo.ink/_looplog"
    echo
    if [ -n "${LOOPLOG_NO_OPEN:-}" ]; then
      "$BIN" --config "$CONFIG" view
    else
      "$BIN" --config "$CONFIG" view --open
    fi

[doc("检查 looplog serve + 隧道 + 域名推送是否可用")]
looplog-status:
    #!/usr/bin/env bash
    set -uo pipefail
    TOKEN_FILE="$HOME/.config/looplog/serve.token"
    TOKEN="$(cat "$TOKEN_FILE" 2>/dev/null || true)"
    echo "serve   : $(lsof -nP -iTCP:3768 -sTCP:LISTEN >/dev/null 2>&1 && echo running || echo DOWN)"
    echo "tunnel  : $(pgrep -f 'R 127.0.0.1:13768' >/dev/null && echo running || echo 'down (按需: just -f looplog/examples/picboo.just looplog-tunnel on)')"
    echo "remote  : $(ssh -o BatchMode=yes -o ConnectTimeout=6 picboo-ali 'sudo -n ss -ltn | grep -q 13768 && echo listening || echo DOWN' 2>/dev/null || echo unreachable)"
    if [ -n "$TOKEN" ]; then
      code=$(curl -sm 8 -o /dev/null -w '%{http_code}' -X POST https://dev-hs.picboo.ink/_looplog/loki/api/v1/push \
        -H "X-Looplog-Token: $TOKEN" -H 'Content-Type: application/json' -d '{"streams":[]}' || echo 000)
      echo "push    : $code (200=ok, 401=token 不匹配, 502=隧道断/服务停)"
    else
      echo "push    : skipped (no $TOKEN_FILE)"
    fi

[doc("按需建/断真机日志回流隧道（不常驻）")]
looplog-tunnel action="on":
    #!/usr/bin/env bash
    set -uo pipefail
    LAN="$(ipconfig getifaddr en0 2>/dev/null || true)"
    TOKEN="$(cat "$HOME/.config/looplog/serve.token" 2>/dev/null || true)"
    probe() {
      [ -n "$TOKEN" ] || { echo "no-token"; return; }
      curl -sm 8 -o /dev/null -w '%{http_code}' -X POST https://dev-hs.picboo.ink/_looplog/loki/api/v1/push \
        -H "X-Looplog-Token: $TOKEN" -H 'Content-Type: application/json' -d '{"streams":[]}' || echo 000
    }
    case "{{action}}" in
      on)
        [ -n "$LAN" ] || { echo "✖ 没有局域网 IP（en0）"; exit 1; }
        lsof -nP -iTCP:3768 -sTCP:LISTEN >/dev/null 2>&1 || echo "⚠ 本机 3768 没有 serve"
        exec ssh -N -T -o ExitOnForwardFailure=yes -o ServerAliveInterval=20 \
          -R 127.0.0.1:13768:${LAN}:3768 picboo-ali
        ;;
      off)
        if pkill -f 'R 127.0.0.1:13768'; then echo "已断开隧道"; else echo "没有在跑的隧道"; fi
        ;;
      status)
        echo "本机隧道: $(pgrep -f 'R 127.0.0.1:13768' >/dev/null && echo running || echo down)"
        echo "远端监听: $(ssh -o BatchMode=yes -o ConnectTimeout=6 picboo-ali 'ss -ltn | grep -q 127.0.0.1:13768 && echo listening || echo down' 2>/dev/null || echo unreachable)"
        echo "域名推送: $(probe) (200=通, 401=token 不匹配, 502=隧道断)"
        ;;
      *)
        echo "用法: just -f looplog/examples/picboo.just looplog-tunnel [on|off|status]"; exit 2;;
    esac
