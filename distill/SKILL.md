---
name: distill
version: "1.1.1"
description: "多分支远端 Git 仓库只读代码探索 + 固化 VictoriaLogs 线上日志联合排查 CLI/TUI/MCP（v1.1.1 正式版，Pure Rust、内存态 Trigram+Rayon 亚毫秒引擎、iroh P2P QUIC 端到端加密、自动密钥脱敏）。支持 AI 自主通读远端全仓库源码与日志、直查目录 tree、全文 grep、按行 read、提交 log、实时日志 vlogs、服务端一体化 investigate、语义索引 search、分支 diff 与服务端 metrics 监控。触发: distill、安装distill、更新distill、升级distill、远端查代码、查代码实现、看代码质量、代码审查、review代码、查分支逻辑、查字段key、查接口契约、对比分支差异、分支最近提交、远程读仓库、查vlogs、VictoriaLogs、查线上日志、查报错、traceId排查、日志联合排查、500报错、连接池超时、distill metrics。"
---

# distill Skill（v1.1.1 正式发布版 · 面向 AI Agent 与自动化程序）

> **当前正式版本**：`v1.1.1`（2026-11-26）· 在线版本标识：`https://synology-minio.youxi123.com/starlink-artifacts/distill/version.txt` · 发布说明：`https://synology-minio.youxi123.com/starlink-artifacts/distill/CHANGELOG.md`

`distill` 是面向研发、测试、产品、运营与 AI Agent 的**多分支远端 Git 只读访问 + 固化 VictoriaLogs（`http://vlogs.hs99.vip:9428`）实时日志联合排查工具**。通过加密 `iroh` P2P QUIC（Ed25519 `EndpointId` 白名单 + ACL + `audit.jsonl` 审计 + 出站密钥脱敏）直连内网 `distilld`，**无需 SSH 账号、无需本地克隆**，即可全语言通读任意分支源码并联动线上日志排查。

---

## 一、关键字触发与任务路由

当用户消息命中以下任意关键字或意图时，优先调用 `distill`：

| 场景类别 | 触发关键字 | 推荐调用方式 |
|---|---|---|
| 安装 / 更新 | `distill`、`安装distill`、`更新distill`、`升级distill` | 执行「二、安装与更新」一键命令并校验 `distill --version` |
| 开放式代码分析 / 质量审查 | `看代码质量`、`实现质量`、`代码审查`、`review`、`梳理架构`、`找隐患` | `distill ask --repo <repo> --branch <branch> "<问题>"`（AI 自主调用远端 MCP 通读全仓库 + 按需查日志） |
| 远端看目录 / 读源码 / 搜代码 | `查代码实现`、`远程读仓库`、`查字段key`、`查逻辑`、`tree`、`grep`、`read` | `distill tree` / `distill grep` / `distill read`（毫秒级 raw 直查） |
| 查提交 / 查分支差异 | `最近提交`、`改了什么`、`分支差异`、`对比分支`、`diff` | `distill log` / `distill branches` / `distill diff` |
| 查接口 / DTO / 错误码 / 文档 | `查接口契约`、`API路由`、`错误码`、`字段定义` | `distill search --kind api_endpoint` 或 `distill grep` |
| 查线上日志 / 排障 | `查vlogs`、`VictoriaLogs`、`查线上日志`、`查报错`、`500`、`超时`、`traceId`、`日志联合排查` | `distill vlogs "<LogsQL>" --range 15m --raw` 或 `distill ask --with-logs ...` |
| AI / MCP 集成 | `distill mcp`、`接入distill`、`切AI模型`、`distill ai` | `distill mcp` / `distill ai list` / `distill ai set <provider>` |

---

## 二、安装、更新与对象存储制品（MinIO 已发布）

对象存储基地址：`https://synology-minio.youxi123.com/starlink-artifacts/distill`（与 `starlink-nebula` 同源）。

### 2.1 安装与一键原地更新（升级直接重跑同一条命令即可）

`install.sh` / `install.ps1` 会附带时间戳防缓存参数下载最新 `latest/<platform>/distill.gz`，解压后原子替换目标二进制，**安装与更新为同一命令**：

- **macOS（Apple Silicon `aarch64` / Intel `x86_64`）与 Linux（`x86_64`）**：
  ```bash
  bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
  ```
- **Windows（PowerShell，64 位 `x86_64`）**：
  ```powershell
  irm https://synology-minio.youxi123.com/starlink-artifacts/distill/install.ps1 | iex
  ```
- **服务端 `distilld` 下载/更新（Linux `x86_64`）**：
  ```bash
  BINARY_NAME=distilld INSTALL_DIR="$HOME/.distill/bin" \
    bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
  ```

### 2.2 安装器支持的环境变量

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `INSTALL_DIR` | `$HOME/.snappywatt/bin`（Windows：`%USERPROFILE%\.snappywatt\bin`） | 二进制安装目录 |
| `SNAPPYWATT_HOME` | `$HOME/.snappywatt` | `INSTALL_DIR` 的默认父目录 |
| `BINARY_NAME` | `distill` | 下载制品名（客户端 `distill`，服务端 `distilld`） |
| `INSTALL_NAME` | 同 `BINARY_NAME` | 落盘后的命令名 |
| `VERSION` | `latest` | 发布通道或固定版本标签（例如 `VERSION=v1.1.1` 锁定正式版） |
| `S3_BASE` / `S3_BUCKET` / `S3_PATH` | `https://synology-minio.youxi123.com` / `starlink-artifacts` / `distill` | 私有对象存储镜像覆盖 |

### 2.3 版本检测、自动更新与安装后校验（Agent 必做）

外部程序或 AI Agent 调用前可先比对本地与远端正式版本号，版本不一致或未安装时自动执行原地升级：

```bash
# 1. 检测本地版本 vs 对象存储最新正式版（当前为 1.1.1）
LOCAL_VER=$(distill --version 2>/dev/null | awk '{print $2}' || echo "none")
REMOTE_VER=$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/version.txt | tr -d '[:space:]')
if [ "$LOCAL_VER" != "$REMOTE_VER" ]; then
  bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
fi

# 2. 确认版本号与核心子命令就绪（如 PATH 未生效先 export PATH="$HOME/.snappywatt/bin:$PATH"）
distill --version   # 预期输出: distill 1.1.1
distill vlogs --help >/dev/null && distill investigate --help >/dev/null && distill metrics --help >/dev/null

# 3. 确认与远端 distilld 加密通道在线，且 server_version 与 vlogs_endpoint 就绪
distill ping
distill status
distill metrics --raw
```

> **提示（锁版本与更新升级）**：默认拉取 `latest/`（始终指向最新正式版）；如需锁定 `v1.1.1`，传 `VERSION=v1.1.1` 即可。若远端 `distilld` 重启更换了内网 UDP 端口，用 `distill configure --server-id <ID> --direct-addr <IP:PORT>` 更新直连提示。

### 2.4 MinIO 对象存储发布清单（v1.1.1）

- 版本与文档：`version.txt`、`latest/version.txt`、`SKILL.md`、`latest/SKILL.md`、`CHANGELOG.md`
- 安装脚本：`install.sh`、`install.ps1`
- 制品目录（同时提供 `latest/` 与正式版本快照 `v1.1.1/`）：
  - `latest/aarch64-apple-darwin/distill.gz` & `v1.1.1/aarch64-apple-darwin/distill.gz`
  - `latest/x86_64-apple-darwin/distill.gz` & `v1.1.1/x86_64-apple-darwin/distill.gz`
  - `latest/x86_64-unknown-linux-gnu/distill.gz` & `v1.1.1/x86_64-unknown-linux-gnu/distill.gz`
  - `latest/x86_64-unknown-linux-gnu/distilld.gz` & `v1.1.1/x86_64-unknown-linux-gnu/distilld.gz`

---

## 三、首次初始化与加白（仅首次）

```bash
# 1. 生成 Ed25519 密钥（$HOME/.distill-client/client.key，权限 0600）并打印 64 位 EndpointId 发给管理员加白
distill print-id

# 2. 保存服务端 EndpointId（可选附带内网直连 IP:PORT 加速连接）
distill configure --server-id <SERVER_ENDPOINT_ID> [--direct-addr <IP:PORT>]

# 3. 验证连通性与状态（查看在线状态、仓库列表、固化 VictoriaLogs 端点）
distill ping
distill status

# 4. 可选：配置本地 AI Provider（用于 distill ask 与交互式 TUI）
distill ai list
distill ai set codex    # 或 claude / openai / anthropic / deepseek / gemini / openrouter / qwen / moonshot / zhipu / ollama / custom
```

---

## 四、CLI 全部 Command 参数完整说明

### 4.1 全局参数（适用于所有子命令）

| 参数 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--home <HOME>` | `DISTILL_CLIENT_HOME` | `$HOME/.distill-client` | 客户端配置与私钥目录 |
| `--server-id <SERVER_ID>` | `DISTILL_SERVER_ID` | 读取 `client.toml` | 远端 `distilld` 的 64 位 EndpointId |
| `--direct-addr <IP:PORT>` | `DISTILL_DIRECT_ADDR` | 读取 `client.toml` | 可选内网直连地址（如 `10.254.144.213:35946`） |
| `-V`, `--version` | — | — | 打印版本号 |
| `-h`, `--help` | — | — | 打印帮助（支持 `distill <subcommand> --help`） |

### 4.2 全部 20 个子命令与参数明细

#### 1) 交互终端与身份配置

- **`distill`** / **`distill tui`**
  - 启动交互式分屏 AI 对话 TUI（默认命令，无额外子命令参数；默认开启鼠标自由选字，`Ctrl+S` 切点击模式，`Ctrl+Y` 复制结论，`Ctrl+P` 切 AI Provider）。
- **`distill print-id`**
  - 生成或读取本地 Ed25519 私钥并输出本机 64 位 `EndpointId`（无额外参数）。
- **`distill configure --server-id <SERVER_ID> [--direct-addr <IP:PORT>]`**
  - `--server-id <SERVER_ID>`（**必填**）：服务端 EndpointId。
  - `--direct-addr <IP:PORT>`（可选）：内网直连 UDP 地址。
- **`distill ai [setup | list | set]`**
  - `distill ai` / `distill ai setup`：交互式向导选择内置 AI Provider、模型与 API Key。
  - `distill ai list`：列出全部 12 个内置 Provider 预设及当前就绪状态。
  - `distill ai set <PROVIDER> [OPTIONS]`：
    - `<PROVIDER>`（**必填**）：`codex` / `claude` / `openai` / `anthropic` / `deepseek` / `gemini` / `openrouter` / `qwen` / `moonshot` / `zhipu` / `ollama` / `custom`。
    - `--model <MODEL>`：覆盖默认模型名。
    - `--base-url <BASE_URL>`：覆盖 API Base URL。
    - `--api-key <API_KEY>`：直接写入 API Key（`0600` 权限落盘）。
    - `--api-key-env <API_KEY_ENV>`：指定运行时读取的环境变量名。
    - `--cli-path <CLI_PATH>`：指定本机 `codex` / `claude` 可执行文件路径。
- **`distill configure-ai [OPTIONS]`**
  - `distill ai set` 的平铺参数别名：`--provider <PROVIDER>`（默认 `codex`）、`--base-url`、`--model`、`--api-key`、`--api-key-env`、`--cli-path`。

#### 2) 连通性、仓库与分支管理

- **`distill ping`**
  - 测试加密 QUIC 连通性，返回 `Pong`（含协议与服务端版本）。
- **`distill status`**
  - 查询远端服务运行时间、认证身份、缓存索引数、请求总数、DLP 脱敏数、仓库同步状态及 `vlogs_endpoint`。
- **`distill metrics [--raw]`**
  - 查询远端服务 `/metrics`（各仓库硬盘占用、活跃 worktree 与对应分支、分支索引缓存、LRU/TTL 清理统计、磁盘阈值与配置热加载次数）；`--raw` 输出易读文本表，默认 JSON。服务端亦默认暴露 HTTP `http://<host>:9464/metrics`（Prometheus 格式）、`/metrics.json` 与 `/healthz`。
- **`distill repos`**
  - 列出当前客户端白名单有权访问的仓库列表及默认分支。
- **`distill branches --repo <REPO>`**
  - `--repo <REPO>`（**必填**）：仓库名（如 `twosite`）；返回允许查询的分支、最新 commit SHA、提交摘要与索引状态。
- **`distill fetch --repo <REPO>`**
  - `--repo <REPO>`（**必填**）：触发远端对指定仓库执行 `git fetch --prune` 并返回更新后的分支清单。

#### 3) Raw Git 全语言通用直查（毫秒级、零检出、自动脱敏密钥）

- **`distill tree --repo <REPO> --branch <BRANCH> [--path <PATH>] [--depth <N>] [--limit <N>]`**
  - `--repo <REPO>`（**必填**）：仓库名。
  - `--branch <BRANCH>`（**必填**）：分支名（如 `main`、`dev/v1`）。
  - `--path <PATH>`（可选）：子目录相对路径（如 `apps/api/src`）。
  - `--depth <DEPTH>`（可选，默认 `3`）：最大递归深度（服务端上限 `8`）。
  - `--limit <LIMIT>`（可选，默认 `200`）：最多返回条目数。
- **`distill grep --repo <REPO> --branch <BRANCH> <PATTERN> [--path <PATH>] [-i|--ignore-case] [-C|--context <N>] [--limit <N>]`**
  - `--repo <REPO>`（**必填**）、`--branch <BRANCH>`（**必填**）。
  - `<PATTERN>`（**必填**，位置参数）：关键词或正则表达式。
  - `--path <PATH>`（可选）：限定子目录路径前缀。
  - `-i`, `--ignore-case`（默认 `true`）：忽略大小写。
  - `-C`, `--context <CONTEXT>`（可选，默认 `2`）：前后上下文行数（上限 `5`）。
  - `--limit <LIMIT>`（可选，默认 `40`）：最大命中条数。
- **`distill read --repo <REPO> --branch <BRANCH> <PATH> [--start <LINE>] [--end <LINE>] [--raw]`**
  - `--repo <REPO>`（**必填**）、`--branch <BRANCH>`（**必填**）。
  - `<PATH>`（**必填**，位置参数）：仓库内相对文件路径（自动拒绝 `.env` / `*.key` / `*.pem` 等密钥文件）。
  - `--start <START>`（可选）：起始行号（1-based，默认 `1`）。
  - `--end <END>`（可选）：结束行号（默认最多读取约 `300` 行，可分段续读）。
  - `--raw`（开关，默认 `false`）：直接输出带行号纯文本，不包 JSON 外壳。
- **`distill log --repo <REPO> --branch <BRANCH> [--limit <N>]`**
  - `--repo <REPO>`（**必填**）、`--branch <BRANCH>`（**必填**）。
  - `--limit <LIMIT>`（可选，默认 `15`，上限 `50`）：查看最近提交历史（SHA、时间、作者、说明、受影响模块）。

#### 4) 语义索引与双分支差异

- **`distill search --repo <REPO> --branch <BRANCH> --query <QUERY> [--kind <KIND>] [--limit <N>]`**
  - `--repo <REPO>`（**必填**）、`--branch <BRANCH>`（**必填**）、`--query <QUERY>`（**必填**）。
  - `--kind <KIND>`（可选）：过滤类型，支持 `api_endpoint` / `dto_schema` / `error_contract` / `domain_rule` / `entity_schema` / `migration_fact` / `doc_section` / `constant_catalog`（简写：`api` / `dto` / `error` / `rule` / `entity` / `migration` / `doc` / `constant`）。
  - `--limit <LIMIT>`（可选，默认 `10`）。
- **`distill diff --repo <REPO> --base <BASE> --target <TARGET>`**
  - `--repo <REPO>`（**必填**）、`--base <BASE>`（**必填**，如 `main`）、`--target <TARGET>`（**必填**，如 `dev/v1`）：对比两分支之间的接口、字段、错误码与文档行为差异。

#### 5) VictoriaLogs 线上日志查询 & 服务端一体化整合 & AI 自主问答

- **`distill vlogs [QUERY] [--range <RANGE>] [--start <RFC3339>] [--end <RFC3339>] [--limit <N>] [--repo <REPO>] [--raw]`**
  - `[QUERY]`（可选，默认 `*`）：LogsQL 表达式（如 `container:twosite-api error` 或 32 位 `traceId`）。
  - `--range <RANGE>`（可选，默认 `15m`）：相对时间窗（如 `5m`、`15m`、`30m`、`1h`、`6h`、`24h`）。
  - `--start <START>` / `--end <END>`（可选）：RFC3339 绝对起止时间戳。
  - `--limit <LIMIT>`（可选，默认 `50`，上限 `200`）：返回最大日志条数。
  - `--repo <REPO>`（可选）：仓库提示（如 `twosite`，未显式写 `container:` 时自动套用该仓库默认容器过滤）。
  - `--raw`（开关，默认 `false`）：以 `[时间] (namespace/container) 消息` 紧凑行格式输出，便于终端阅读与管道处理。
- **`distill investigate --repo <REPO> --branch <BRANCH> <QUESTION> [--with-logs | --no-logs] [--log-range <RANGE>]`**
  - `--repo <REPO>`（**必填**）、`--branch <BRANCH>`（**必填**）、`<QUESTION>`（**必填**，位置参数）。
  - `--with-logs`（开关，与 `--no-logs` 互斥）：强制在服务端联合查 VictoriaLogs。
  - `--no-logs`（开关）：仅执行 Raw Git 代码整合，不查日志。
  - `--log-range <LOG_RANGE>`（可选）：覆盖自动识别的日志时间窗（如 `5m`、`15m`、`1h`）。
  - 输出：服务端并发整合后的 `InvestigationBundle` 原始 JSON（适合外部 Agent / 脚本直接消费）。
- **`distill ask --repo <REPO> --branch <BRANCH> <QUESTION> [--with-logs | --no-logs] [--log-range <RANGE>] [--copy]`**
  - 参数同 `investigate`，额外支持 `--copy`（同时写入系统剪贴板）。
  - 执行流：服务端并发打包基线证据（Raw Git + 可选 VictoriaLogs）→ 本地 AI 挂载远端 `distill mcp` **自主多轮通读全仓库源码并按需追查 VictoriaLogs** → 输出带精确 `path:line` 与日志佐证的中文 Markdown 结论。

#### 6) MCP Server

- **`distill mcp`**
  - 以 JSON-RPC 2.0 over stdio 运行本地只读 MCP Server，供 Antigravity / Claude Code / Cursor / Codex / 自定义 Agent 直接调用。

---

## 五、常用示例

```bash
# 1. 宏观代码质量/架构审查（AI 自主遍历远端全仓库源码 + 联动 VictoriaLogs 核实线上报错）
distill ask --repo twosite --branch main "看下代码实现质量"

# 2. 直查固化 VictoriaLogs（http://vlogs.hs99.vip:9428）
distill vlogs "container:twosite-api error" --range 15m --limit 30 --raw

# 3. 线上故障联合排查（强制联动 1h 日志 + 源码定位）
distill ask --repo twosite --branch dev/v1 --with-logs --log-range 1h "排查 traceId 114c4eaf2466798ac1d3e6982e8bc138"

# 4. 外部程序直接拿服务端整合好的结构化证据 JSON（无需本地 AI）
distill investigate --repo twosite --branch dev/v1 "最近15分钟 twosite-api 有什么报错，对应哪段代码"

# 5. 快速原始 Git 查证（全语言通用）
distill tree --repo twosite --branch dev/v1 --path apps/api/src --depth 2
distill grep --repo twosite --branch dev/v1 "HttpAccess" -C 2
distill read --repo twosite --branch dev/v1 apps/api/src/observability/http-exchange-log.ts --start 180 --end 240 --raw
distill log --repo twosite --branch dev/v1 --limit 10
```

---

## 六、TUI 内置斜杠命令与快捷键

运行 `distill` 进入 TUI 后：
- **直接输入自然语言**：AI 自主通读远端源码并按需查询 VictoriaLogs 后直接回复结论。
- **斜杠命令**：`/vlogs [时间窗] [LogsQL]`（别名 `/logs`）、`/ask+log <问题>`（别名 `/diagnose`）、`/tree [子目录]`、`/grep <模式>`、`/read <路径> [起行] [止行]`、`/log [条数]`、`/diff <基准分支>`、`/branch <分支名>`、`/fetch`、`/status`、`/ai`（`/model`）、`/copy [#N]`、`/select`、`/clear`、`/help`、`/quit`。
- **快捷键**：`Ctrl+Y` 复制最新结论、`Ctrl+S` 切换自由选字/按钮点击模式、`Ctrl+P` 打开 AI Provider 设置、`Cmd+U`/`Ctrl+U` 清空输入、`Ctrl+F` 远端 Fetch、`Ctrl+R` 刷新状态。

---

## 七、MCP Server 接入（供外部 AI 程序挂载）

```json
{
  "mcpServers": {
    "distill": {
      "command": "distill",
      "args": ["mcp"]
    }
  }
}
```

暴露 **13 个只读安全工具**（均标记 `readOnlyHint: true`）：
`server_status`、`list_repos`、`list_branches`、`fetch_repo`、`list_tree`、`grep_repo`、`read_file`、`get_recent_commits`、`search_semantics`、`get_api_contracts`、`diff_branch_behavior`、`query_vlogs`、`investigate`。

---

## 八、安全边界与排错

- **本地目录**：`$HOME/.distill-client`（`client.key` `0600`、`client.toml`）。
- **安全护栏**：Ed25519 `EndpointId` 白名单 + 仓库/分支 ACL + 频率限制 + `audit.jsonl` 全量审计；自动拒绝 `.env` / `*.pem` / `*.key` / `id_rsa` 等密钥文件，并对所有代码、提交与日志出站内容自动脱敏 Token / 私钥 / 连接串 / API Key。
- **常见排错**：
  - `distill` 找不到命令：确认 `$HOME/.snappywatt/bin` 已加入 `PATH`，或重新执行 `install.sh`。
  - 连接超时或被拒：确认 `distill print-id` 已加白，且 `distill configure --server-id ... --direct-addr ...` 指向当前在线端口。
  - 缺少新子命令（如 `vlogs` / `investigate`）：重跑 `install.sh` / `install.ps1` 原地升级即可。
