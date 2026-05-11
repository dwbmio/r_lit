# 微信小程序 AI 自主循环调试指南

这份文档面向微信小程序开发者：让微信开发者工具、桥接脚本或构建命令把本轮调试日志写入 `looplog`，然后让 AI 只通过 `looplog` CLI 识别失败原因、修改代码、重新验证，进入下一轮自主联调。

`looplog` 只收集本地 24 小时内的短周期调试信息。它不是线上日志系统，也不会监听外网地址。

## 启动本地日志入口

```bash
cargo build --release
./target/release/looplog serve --addr 127.0.0.1:3768
curl -s http://127.0.0.1:3768/healthz
```

预期返回：

```json
{
  "status": "ok",
  "service": "looplog"
}
```

## 最小可用接入

先用 `looplog run` 包裹现有小程序构建命令：

```bash
looplog run \
  --tag miniprogram-build \
  --meta appid=wx123 \
  --meta project_path=/path/to/wxapp \
  --meta page=pages/index/index \
  --meta compile_mode=dev \
  -- npm run build
```

AI 下一步可查询：

```bash
looplog list --kind wechat_miniprogram --appid wx123 --json
looplog grep "TypeError|ReferenceError|WAService|fail|ERR_|undefined|null" --appid wx123 --since 2h --json
looplog show <run_id> --tail 300 --json
```

## HTTP 写入协议

创建 run：

```bash
RUN_ID=$(curl -s http://127.0.0.1:3768/v1/runs \
  -H 'content-type: application/json' \
  -d '{
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
      "device": "iPhone 15",
      "session": "ai-debug-001"
    }
  }' | python3 -c 'import json,sys; print(json.load(sys.stdin)["run_id"])')
```

追加 NDJSON 日志：

```bash
curl -s "http://127.0.0.1:3768/v1/runs/${RUN_ID}/lines" \
  -H 'content-type: application/x-ndjson' \
  --data-binary $'{"stream":"console","level":"error","event":"console","text":"TypeError: Cannot read property x of undefined"}\n{"stream":"network","level":"info","event":"request","text":"GET /api/user 200"}\n'
```

结束 run：

```bash
curl -s -X PATCH "http://127.0.0.1:3768/v1/runs/${RUN_ID}" \
  -H 'content-type: application/json' \
  -d '{"status":"failed","exit_code":1}'
```

## TypeScript SDK

`sdk/ts/looplog.ts` 提供极薄 HTTP Client：

```ts
import { LoopLogClient } from "./sdk/ts/looplog";

const client = new LoopLogClient({ endpoint: "http://127.0.0.1:3768" });
const run = await client.startRun({
  tag: "wx-console",
  source: "wechat-devtools",
  kind: "wechat_miniprogram",
  cwd: "/path/to/wxapp",
  meta: {
    project_path: "/path/to/wxapp",
    appid: "wx123",
    page: "pages/index/index",
    compile_mode: "preview",
    base_lib_version: "3.x",
    platform: "devtools",
    session: "ai-debug-001",
  },
});

await run.append([{ stream: "console", level: "error", event: "console", text: "TypeError: ..." }]);
await run.finish({ status: "failed", exit_code: 1 });
```

`adapters/wechat/wechat_adapter.ts` 是 alef 风格适配层样例，负责把微信小程序上下文转换成 `looplog` 的统一协议。

## AI 自主循环约定

1. 确认 `looplog serve` 正在运行。
2. 用 `looplog list --json` 找最新 run。
3. 用 `looplog grep --json` 搜索高价值错误。
4. 用 `looplog show --json` 展开失败上下文。
5. 修改代码后重新触发构建或开发工具写入新 run。
6. 只比较最新 run，直到错误消失或进入新的明确错误。

## 推荐 meta

建议每轮至少写入：`appid`、`project_path`、`page`、`compile_mode`、`base_lib_version`、`platform`、`device`、`session`、`trace_id`。
# 微信小程序 AI 自主循环调试指南

这份文档面向微信小程序开发者：目标是让微信开发者工具、桥接脚本或构建命令把本轮调试日志写入 `looplog`，然后让 AI 只通过 `looplog` CLI 就能识别失败原因、修改代码、重新触发验证，进入下一轮自主联调。

`looplog` 只收集本地 24 小时内的短周期调试信息。它不是线上日志系统，也不会监听外网地址。

## 1. 构建并启动本地日志入口

在 `looplog` 目录构建：

```bash
cargo build --release
```

启动本地 HTTP 摄入口：

```bash
./target/release/looplog serve --addr 127.0.0.1:3768
```

健康检查：

```bash
curl -s http://127.0.0.1:3768/healthz
```

预期返回：

```json
{
  "status": "ok",
  "service": "looplog"
}
```

## 2. 最小可用接入：用 CLI 包裹构建命令

如果你已经有小程序构建脚本，先不写任何 SDK，也可以直接让 AI 使用 `looplog run`：

```bash
looplog run \
  --tag miniprogram-build \
  --meta appid=wx123 \
  --meta project_path=/path/to/wxapp \
  --meta page=pages/index/index \
  --meta compile_mode=dev \
  -- npm run build
```

这会捕获 stdout/stderr、退出码、appid、页面路由等信息。AI 下一步可用：

```bash
looplog list --kind wechat_miniprogram --appid wx123 --json
looplog grep "TypeError|ReferenceError|WAService|fail|ERR_" --appid wx123 --since 2h --json
looplog show <run_id> --tail 200 --json
```

## 3. 推荐接入：开发工具或桥接脚本写 HTTP 协议

微信开发者工具本身或外部桥接脚本可以用本地 HTTP 协议创建一轮调试 run。

创建 run：

```bash
RUN_ID=$(curl -s http://127.0.0.1:3768/v1/runs \
  -H 'content-type: application/json' \
  -d '{
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
      "device": "iPhone 15",
      "session": "ai-debug-001"
    }
  }' | python3 -c 'import json,sys; print(json.load(sys.stdin)["run_id"])')
```

追加日志行，格式是 `application/x-ndjson`：

```bash
curl -s "http://127.0.0.1:3768/v1/runs/${RUN_ID}/lines" \
  -H 'content-type: application/x-ndjson' \
  --data-binary $'{"stream":"console","level":"error","event":"console","text":"TypeError: Cannot read property x of undefined"}\n{"stream":"network","level":"info","event":"request","text":"GET /api/user 200"}\n'
```

结束本轮 run：

```bash
curl -s -X PATCH "http://127.0.0.1:3768/v1/runs/${RUN_ID}" \
  -H 'content-type: application/json' \
  -d '{"status":"failed","exit_code":1}'
```

## 4. TypeScript SDK 接入方式

`sdk/ts/looplog.ts` 提供极薄 HTTP Client，适合 Node、开发者工具插件、桥接脚本复用。

```ts
import { LoopLogClient } from "./sdk/ts/looplog";

const client = new LoopLogClient({ endpoint: "http://127.0.0.1:3768" });
const run = await client.startRun({
  tag: "wx-console",
  source: "wechat-devtools",
  kind: "wechat_miniprogram",
  cwd: "/path/to/wxapp",
  meta: {
    project_path: "/path/to/wxapp",
    appid: "wx123",
    page: "pages/index/index",
    compile_mode: "preview",
    base_lib_version: "3.x",
    platform: "devtools",
    session: "ai-debug-001",
  },
});

await run.append([
  {
    stream: "console",
    level: "error",
    event: "console",
    text: "TypeError: Cannot read property x of undefined",
  },
]);

await run.finish({ status: "failed", exit_code: 1 });
```

`adapters/wechat/wechat_adapter.ts` 是 alef 风格适配层样例：它负责把微信小程序上下文转换成 `looplog` 的统一协议，后续可以扩展到页面打开、网络请求、真机调试等事件。

## 5. 给 AI 的自主循环约定

当 AI 进入微信小程序联调任务时，建议按这个流程执行：

1. 确认 `looplog serve` 正在运行：

```bash
curl -s http://127.0.0.1:3768/healthz
```

2. 查看最近的小程序 run：

```bash
looplog list --kind wechat_miniprogram --appid wx123 --limit 10 --json
```

3. 搜索高价值错误：

```bash
looplog grep "TypeError|ReferenceError|SyntaxError|WAService|fail|ERR_|undefined|null" \
  --appid wx123 \
  --since 2h \
  --json
```

4. 展开失败 run 的上下文：

```bash
looplog show <run_id> --tail 300 --json
```

5. 修改代码后，重新触发构建或让开发工具/桥接脚本写入新 run。

6. 再次执行 `list`、`grep`、`show`，只比较最新 run，直到错误消失或进入新的明确错误。

## 6. 建议写入的微信小程序 meta

为了让 AI 具备足够上下文，建议每轮 run 至少写入：

- `appid`：小程序 appid。
- `project_path`：本地小程序项目根目录。
- `page`：当前页面路由，例如 `pages/index/index`。
- `compile_mode`：`dev`、`preview`、`custom`、`device` 等。
- `base_lib_version`：微信基础库版本。
- `platform`：`devtools`、`ios`、`android`。
- `device`：模拟器或真机型号。
- `session`：一次 AI 联调会话 id。
- `trace_id`：用于串起构建、页面打开、接口请求的链路 id。

## 7. 清理与安全边界

`looplog` 默认只保留 24 小时内记录：

```bash
looplog clean
```

服务只允许绑定 `127.0.0.1`、`localhost` 或 `[::1]`。如果尝试绑定外网地址，MVP 会拒绝启动。
