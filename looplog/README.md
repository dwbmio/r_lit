# looplog

`looplog` is a local-first log intake and query tool for short AI-assisted debugging loops.

It stores logs in a local SQLite database, keeps only the recent debugging window by default, and exposes a loopback-only HTTP intake protocol so non-CLI tools can write logs from any language.

The first MVP is tuned for WeChat Mini Program debugging.

For the immediate WeChat Mini Program workflow that lets an AI agent inspect logs and repeat the debug loop through CLI queries, see [`WECHAT_AI_DEBUG_CN.md`](WECHAT_AI_DEBUG_CN.md).

## Install / Build

```bash
cargo build --release
```

## Quick Start

Start the local intake server:

```bash
looplog serve --addr 127.0.0.1:3768
```

Wrap a build command and attach WeChat metadata:

```bash
looplog run --tag miniprogram-build \
  --meta appid=wx123 \
  --meta project_path=./wxapp \
  --meta page=pages/index/index \
  -- npm run build
```

Query recent logs for AI consumption:

```bash
looplog list --kind wechat_miniprogram --appid wx123 --json
looplog grep TypeError --appid wx123 --page pages/index/index --since 2h --json
looplog show <run_id> --tail 200 --json
```

## HTTP Protocol

The server only accepts loopback binds in the MVP.

```http
POST /v1/runs
POST /v1/runs/{run_id}/lines
PATCH /v1/runs/{run_id}
GET /healthz
```

Create a run:

```json
{
  "tag": "wx-console",
  "source": "wechat-devtools",
  "kind": "wechat_miniprogram",
  "cwd": "/path/to/wxapp",
  "meta": {
    "project_path": "/path/to/wxapp",
    "appid": "wx123",
    "page": "pages/index/index",
    "compile_mode": "preview",
    "base_lib_version": "3.x",
    "platform": "devtools",
    "session": "ai-debug-001"
  }
}
```

Append log lines as `application/x-ndjson`:

```jsonl
{"stream":"console","level":"error","event":"console","text":"TypeError: Cannot read property x of undefined"}
{"stream":"network","level":"info","event":"request","text":"GET /api/user 200"}
```

Finish the run:

```json
{"status":"failed","exit_code":1}
```

## WeChat Metadata

Common fields are indexed for filtering:

- `kind`: usually `wechat_miniprogram`
- `project_path`
- `appid`
- `page`
- `session`
- `trace_id`

Additional fields are kept in `run_meta`, including `query`, `scene`, `compile_mode`, `tool_version`, `base_lib_version`, `platform`, `device`, and `network`.

## Retention

`looplog` is intentionally short-lived. The default retention window is 24 hours. `serve`, write commands, and query commands run lightweight cleanup automatically.

Manual cleanup:

```bash
looplog clean
looplog clean --keep-hours 6 --vacuum
```

Values above 24 hours are capped to 24 in the MVP.

## SDK

A small TypeScript client lives in `sdk/ts/looplog.ts`. A WeChat Mini Program adapter sample lives in `adapters/wechat/wechat_adapter.ts`.

The SDK calls `looplog serve`; it does not write SQLite directly.
