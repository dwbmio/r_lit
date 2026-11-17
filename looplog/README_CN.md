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

## 多流实时分栏（真机联调）

服务端与客户端日志分散在多个终端时，用 `tail` 把每路流接进本地库，再用 `view` 在一个窗口里分栏看。

### 1. 接入每一路流

`looplog tail` 有五种结构化源，差别在**时间戳与字段是否保真**：

| `--from` | 命令 | 时间戳 | 级别 | 附加字段 |
|---|---|---|---|---|
| `journald` | `journalctl -o json -f` | `__REALTIME_TIMESTAMP`（µs） | `PRIORITY` 0-7 | pid / identifier / unit / hostname |
| `nginx` | `tail -F` + 格式解析 | 日志内 `time_local`（带时区） | status 5xx→error / 4xx→warn | remote_addr / request / request_time / upstream_time |
| `logcat` | `adb logcat -v epoch -T N` | 设备 epoch 时间 | V/D→debug · I→info · W→warn · E/F/A→error | pid / tid / tag / priority |
| `file` | `tail -F` | 接收时间 | 关键词推断 | — |
| `cmd` | 任意流式命令 | 接收时间 | 关键词推断 | — |

```bash
# 服务端（journald，需要 sudo 读 system journal）
looplog tail --tag picboo-api --reconnect --sudo \
  --from journald --remote picboo-ali --unit picboo-api --since 15m

# 网关（openresty access log，解析状态码与上游耗时）
looplog tail --tag picboo-edge --reconnect --sudo \
  --from nginx --remote picboo-ali \
  --path /opt/wirerope-stack/wirerope/logs/ding/c_picboo_access.log --lines 200

# 客户端文件（raw；时间戳是接收时间）
looplog tail --tag picboo-client --reconnect \
  --from file --path _ai/runs/weapp-dev-debug/latest.log --lines 0

# Android 真机（多设备时加 --serial <adb devices 的序列号>）
looplog tail --tag android-logcat --reconnect \
  --from logcat --serial emulator-5554 --lines 200

# 兼容入口：任意命令
looplog tail --tag anything --reconnect -- ssh host 'journalctl -o json -f'
```

行为说明：

- **入库时间单独记录**为 `ingest_ms`，`ts_ms` 存源时间。`view` 里 `ts` 后会出现 `+820ms` / `+13.2h` 这类标记（`|lag| ≥ 250ms` 时才显示），机器时钟漂移会显示负数。
- journald 的 `PRIORITY` 常被 Rust tracing 写成 6(info)，真实级别在正文里 → 解析时**只升不降**地按内容修正；ANSI 颜色码会被剥离。
- 多个 `tail` 进程可同时写同一个库（WAL + busy_timeout + journal_mode 切换重试）。
- `--reconnect` 断线后指数退避（1s→2s→…→`--reconnect-max`），同一 run 继续追加。
  - **journald 精确续读**：记住已入库的最大 `ts_ms`，重连时用 `--since @<epoch>.<usec>`（比该毫秒晚 1µs），
    既是“不多读”也是“不漏读”。实测：第二次调用为 `--since @1792417000.000001`，2 行无重复。
  - `nginx` / `file` / `logcat` 没有可续的时间锚点，重连仍按 `--lines N` 重放，可能出现重复行（已知，待定语义）。
- Ctrl-C/Ctrl-​TERM 会把 run 标记为 `interrupted` 并杀掉子进程（不留孤儿）。
- 一次性历史回灌用 `run`；`tail` 的 `--since`（journald）/`--lines`（文件/logcat）只做预热。
- `logcat` 默认强制 `epoch`，避免设备本地时区和 threadtime 缺少年份造成时间轴偏差；不能解析的行保留为原文并退回接收时间。

### 2. 一个窗口看全部

```bash
looplog view --open \
  --column client:picboo-client \
  --column edge:picboo-edge \
  --column api:picboo-api \
  --column gql:picboo-gql
```

| 能力 | 说明 |
|---|---|
| 分栏 | 每个 `label:tag` 一栏，各栏独立滚轮 |
| 合并 | 所有 tag 按时间戳排成一条时间轴，最适合看「点按钮 → 网关 200 → 服务 503」的因果 |
| 过滤 + 高亮 | 顶栏输入框；`/正则/i` 形式走正则，普通词高亮；匹配即过滤 |
| 级别 | 全部 / warn+ / 仅 error / debug+ |
| 暂停 | 暂停拉取（不丢数据，继续后补齐） |
| 冻结 | 上滚看历史时该栏停止追加，出现「↓ N 条新日志」；点它（或按 `g`）**所有栏一起**回到最新并恢复跟随 |
| 复制 | 点击任意行复制「时间 [tag] 正文」 |
| 快捷键 | `/` 搜索、`p` 暂停、`m` 分栏/合并、`g` 回到最新 |
| 自动发现 | 不带 `--column` 时取最近窗口内活跃的前 4 个 tag |
| **时间线对齐** | 「⏱ 对齐」默认开：滚动任一栏，其他栏二分定位到**同一条时间线上**（第一个 `ts >= 锚点` 的行）；锚点=主动滚动那栏的顶部时间 |
| 时间游标 | 顶栏 `⏱ HH:MM:SS.mmm` 是当前对齐位置；每栏标题右侧是该栏最新一行的时间（各路的“进度”） |

`--open` 在 macOS 用 Chrome `--app` 模式开无地址栏独立窗口；其他平台走系统默认浏览器。

`view` 对数据库**只读**：不迁移、不写入，可与 `tail` / `serve` 同时运行。

可复用接口（后续原生 GUI 用同一套）：

```http
GET /                              # 内嵌单页
GET /api/lines?after_ms=&tags=a,b&limit=3000&tail=1
GET /api/tags?since_ms=
GET /healthz
```

### 3. 远端 sudo（tail 没有 tty）

`journalctl` 需要提权，交互式密码在子进程里无法输入。给目标机加只放行这几条命令的 sudoers：

```sudoers
# /etc/sudoers.d/looplog
picboo ALL=(root) NOPASSWD: /bin/journalctl -u picboo-api -f -o cat
picboo ALL=(root) NOPASSWD: /bin/journalctl -u picboo-gql -f -o cat
picboo ALL=(root) NOPASSWD: /usr/bin/tail -F /opt/wirerope-stack/wirerope/logs/ding/c_picboo_access.log
```

或把采集用户加入 `systemd-journal` 组直接读 journal。ssh 免密走已有 ssh config / warpgate 生成的 Hosts。

一条命令拉起整套（示例见 `examples/picboo-dev.sh`）：

```bash
./examples/picboo-dev.sh          # 起 4 路 tail + 打开 view
```

## 渲染与性能

**渲染栈是 HTML/DOM**（`src/view/index.html` 用 `include_str!` 内嵌进二进制，浏览器排版），
不是 egui，也不是 GPUI（GPUI 只在 roadmap 里作为原生壳的备选）。选它的理由是同仓零原生依赖、
一套代码跑 macOS/Linux、且调试时直接用 DevTools。代价是 DOM 有成本上限，用下面几条把它钉死。

| 机制 | 做法 |
|---|---|
| 增量渲染 | 稳态只 `insertAdjacentHTML('beforeend')` 追加新行；全量重绘只发生在过滤/切换/清屏 |
| DOM 上限 | 每栏保留最近 `1200` 行，超过 `1500` 批量裁掉头部的行，节点数恒定 |
| 后端索引 | `idx_lines_ts_seq(ts_ms, seq)`；查询用 `INDEXED BY` 强制走时间范围（否则优化器从 `runs(tag)` 驱动，会扫整窗） |
| 增量游标 | `/api/lines?after_ms=` 只取游标之后的行，前端按 `run#seq` 去重 |
| tag 聚合 | `/api/tags` 改为聚合 `runs`（O(runs)），不再扫 `log_lines` |
| 冻结 | 上滚时该栏停止追加，只累计「↓ N 条新日志」，回到底部才补一次 |

实测（MEASURED，1M 行库、16 分钟窗口、持续 1000 行/秒写入、4 栏、Chrome/Playwright）：

| 指标 | 优化前 | 优化后 |
|---|---|---|
| 稳态 `/api/lines`（p50） | 74 ms | **0.8 ms** |
| 首屏 3000 行 | 122 ms | **7.6 ms** |
| `/api/tags` | 312 ms | **0.7 ms** |
| 帧率 / 抖动 | 120fps / 0 长任务 | 120fps / 0 长任务 |
| 每栏 DOM 行数 | 随窗口增长 | 稳定 1200 |

浏览器控制台可用 `window.__looplog` 检查内部状态（`state`、`topTs()`、`syncing` 等），排查同步/渲染问题时用。

## 真机日志回流（域名通道）

真机的 `logger.*` 走**与业务 API 同域的子路径**，不占用额外域名、不加证书、不改微信合法域名：

```
手机小程序 → https://dev-hs.picboo.ink/_looplog/loki/api/v1/push
           → picboo-ali nginx（只放行 POST，2m，2s 超时）
           → 172.18.0.1:13768（socat 桥）
           → 127.0.0.1:13768（ssh -R，仅在你按需起隧道时存在）
           → 你 Mac 的 looplog serve:3768
```

| 环境 | 日志 endpoint | 说明 |
|---|---|---|
| loc | `http://127.0.0.1:3768` | 模拟器里的 127.0.0.1 就是 Mac，不经隧道 |
| dev | `https://dev-hs.picboo.ink/_looplog` | 与业务 API 同域 |
| prod | `https://prod-c.picboo.ink/_looplog` | 同上；线上包不要带 |

### 本机不常驻

隧道**不注册 launchd**，需要时临时起，Ctrl-C 即断：

```bash
just looplog-tunnel          # 前台建隧道（Ctrl-C 断开）
just looplog-tunnel status   # 本机隧道 / 远端监听 / 域名推送探测
just looplog-tunnel off      # 清残留
```

⚠️ 隧道目标必须是 **Mac 的局域网 IP**（`-R 127.0.0.1:13768:<LAN_IP>:3768`）。用 `127.0.0.1` 会让 looplog 把隧道来的流量判定为本机 loopback、**免 token 放行**——这是实测踩到的安全漏洞，`just` 里已固定用 LAN IP。

### 服务端配置（正本在 ci-all-in-one）

- `compose/ding/deployments/picboo/conf.d/{dev-hs,prod-c}-picboo.conf`：`location /_looplog/` → `172.18.0.1:13768`
- `compose/ding/deployments/picboo/host/looplog-tunnel-fwd.service`：socat 把 `172.18.0.1:13768` 桥到宿主 `127.0.0.1:13768`

安全边界：只允许 `POST`；body ≤ 2m；无 token 返回 401；Mac 没起隧道时 2s 内返回 502（客户端退避，不影响业务）。

## 配置

**唯一事实源**：`~/.config/looplog/config.toml`（`--config` 可覆盖，`XDG_CONFIG_HOME` 生效）。

```bash
looplog config          # 打印路径与当前内容
looplog config --init   # 生成默认配置（已存在需 --force）
```

```toml
version = 1

[view]
since = "15m"                 # 打开视图的时间窗口
autostart = true              # 打开视图时拉起 enabled 的来源
columns = [
  { label = "client", tag = "picboo-client" },
  { label = "api",    tag = "picboo-api" },
]

[serve]
addr = "127.0.0.1:3768"
allow_lan = false
allow_cidrs = []

[[source]]
name = "picboo-api"           # 唯一 id（页面里启停用它）
tag = "picboo-api"            # 写入 runs.tag，即分栏名
from = "journald"             # journald | nginx | file | cmd | logcat
remote = "picboo-ali"
unit = "picboo-api"
since = "15m"
sudo = true
reconnect = true
enabled = true
```

### 页面内管理（⚙ 配置）

顶栏「⚙ 配置」打开右侧抽屉，直接读写上面这个文件：

| 能力 | 说明 |
|---|---|
| 视图 | 时间窗口、是否 autostart、分栏列表（每行 `label:tag`） |
| 来源列表 | 每个来源显示 运行中/已停止 圆点、`from`、目标（unit/path + ssh 主机）、当前 run id |
| 启停 | 「启动 / 停止」立即生效，不写文件（临时观察用） |
| CRUD | 新增 / 编辑 / 删除（改的是内存草稿，点保存才落盘） |
| 保存并应用 | `PUT /api/config` → 写文件（先备份 `config.toml.bak`）→ 重启受影响的来源 |

语义约定：

- **配置是唯一事实源**；`view --column / --since` 只覆盖本次进程启动时的初值，页面加载后会以文件为准。
- 启停按钮只改运行时，不改 `enabled`；`enabled` 决定 autostart 与「保存并应用」时是否拉起。
- 保存会重启所有正在运行的来源（避免半旧半新）。正在看的日志会短暂中断。
- token **不写入配置**：`serve --allow-lan --token` 仍在命令行，避免密钥落到文件里。

示例配置：`examples/picboo-config.toml`；一键脚本 `examples/picboo-dev.sh`（首次运行会复制该模板）。

## HTTP 协议

默认只绑定 loopback。真机联调需要局域网访问时显式开启：

```bash
# token 至少 16 字符；非 loopback 对端必须带 token，且落在 --allow-cidr 白名单
# 默认白名单：127/8、::1、10/8、172.16/12、192.168/16、169.254/16
looplog serve --addr 0.0.0.0:3768 --allow-lan --token "$(openssl rand -hex 16)"
```

鉴权头：`Authorization: Bearer <token>` 或 `X-Looplog-Token: <token>`；loopback 对端免鉴权。

写入有两条路径：

| 端点 | 用途 |
|---|---|
| `POST /v1/runs` → `POST /v1/runs/{id}/lines` | 原生（ndjson，`sdk/ts/looplog.ts` 用这条） |
| `POST /loki/api/v1/push` | **Loki 兼容**（小程序里现成的 `LokiClient` 用这条） |

### Loki 兼容入口

picboo 小程序已有 `LokiClient`，把 endpoint 指向 looplog 就能回流，不必再写一个 reporter：

```json
{"streams":[{"stream":{"level":"info","source":"typescript","app_id":"picboo","looplog_tag":"picboo-client"},
  "values":[["1792465108984000000","{\"message\":\"login.start\",\"metadata\":{\"page\":\"pages/index/index\"}}"]]}]}
```

- `values` 的纳秒时间戳 → `ts_ms`，**保留端上时间**（这也是看滞后/时钟偏差的前提）
- label 依次取 `looplog_tag` → `app_id` → `source` 作为 `runs.tag`（`view --column` 用它分栏）
- line 是 JSON 时解出 `message` 与其余字段进 meta；否则整行当正文
- 同一 label 集合在本进程内复用同一个 run，会话不会碎成多条


```http
POST /v1/runs
POST /v1/runs/{run_id}/lines
PATCH /v1/runs/{run_id}
POST /loki/api/v1/push
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
