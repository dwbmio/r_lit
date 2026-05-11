# looplog

`looplog` 是一个本地优先的日志摄入与查询工具，用于 AI 辅助联调中的短周期日志回收。

它把日志写入本地 SQLite，默认只保留最近 24 小时，并提供仅监听 loopback 的 HTTP 摄入协议，让非 CLI 工具也能跨语言写入日志。

第一版 MVP 先对标微信小程序开发联调。

微信小程序开发者希望“立刻执行调试并让 AI 自主循环识别问题”的完整流程见 [`WECHAT_AI_DEBUG_CN.md`](WECHAT_AI_DEBUG_CN.md)。

## 构建

```bash
cargo build --release
```

## 快速开始

启动本地摄入口：

```bash
looplog serve --addr 127.0.0.1:3768
```

包裹构建命令并附带微信小程序元信息：

```bash
looplog run --tag miniprogram-build \
  --meta appid=wx123 \
  --meta project_path=./wxapp \
  --meta page=pages/index/index \
  -- npm run build
```

给 AI 查询最近日志：

```bash
looplog list --kind wechat_miniprogram --appid wx123 --json
looplog grep TypeError --appid wx123 --page pages/index/index --since 2h --json
looplog show <run_id> --tail 200 --json
```

## HTTP 协议

MVP 阶段服务只允许绑定 loopback 地址。

```http
POST /v1/runs
POST /v1/runs/{run_id}/lines
PATCH /v1/runs/{run_id}
GET /healthz
```

创建 run：

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

用 `application/x-ndjson` 追加日志行：

```jsonl
{"stream":"console","level":"error","event":"console","text":"TypeError: Cannot read property x of undefined"}
{"stream":"network","level":"info","event":"request","text":"GET /api/user 200"}
```

结束 run：

```json
{"status":"failed","exit_code":1}
```

## 微信小程序元信息

常用字段会冗余到 `runs` 表，方便 CLI 快速过滤：

- `kind`：通常是 `wechat_miniprogram`
- `project_path`
- `appid`
- `page`
- `session`
- `trace_id`

其他字段会保留在 `run_meta`，包括 `query`、`scene`、`compile_mode`、`tool_version`、`base_lib_version`、`platform`、`device`、`network`。

## 保留周期

`looplog` 不做长期日志仓库。默认只保留 24 小时内记录。`serve`、写入命令、查询命令都会自动做轻量清理。

手动清理：

```bash
looplog clean
looplog clean --keep-hours 6 --vacuum
```

MVP 阶段超过 24 小时的保留参数会被截断到 24 小时。

## SDK

轻量 TypeScript client 位于 `sdk/ts/looplog.ts`，微信小程序 alef 风格适配样例位于 `adapters/wechat/wechat_adapter.ts`。

SDK 默认调用 `looplog serve`，不直接写 SQLite。
