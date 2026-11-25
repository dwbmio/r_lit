# `distill` 本地客户端与本地 AI（MCP）使用指南

> **适用对象**：产品、测试、运营、设计等跨职能使用者（只需拿到 `distill` 单个可执行文件 + 管理员给的 `Server EndpointId`，**无需 SSH 账号、无需 Git 权限、无需下载源码**）。

---

## 1. 安装 `distill` 二进制

### 一键安装（推荐，从内部 MinIO 拉取，与 `starlink-nebula` 同源）

macOS / Linux：

```bash
bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
```

Windows（PowerShell）：

```powershell
irm https://synology-minio.youxi123.com/starlink-artifacts/distill/install.ps1 | iex
```

默认安装到 `$HOME/.snappywatt/bin/distill`（可通过 `INSTALL_DIR=/custom/bin` 覆盖），安装后验证：

```bash
distill --version
```

### 从源码自行构建（可选）

```bash
cd distill
cargo build --release -p distill-client
cp target/release/distill "$HOME/.local/bin/distill"
```

---

## 2. 首次初始化：生成身份并申请加白

1. **生成并查看本机客户端 ID**：
   ```bash
   distill print-id
   ```
   - 首次运行会自动在 `$HOME/.distill-client/client.key` 生成本机专属 Ed25519 私钥（请勿外泄）；
   - 将终端输出的 **64 位 `EndpointId`** 发给管理员加入服务端白名单。

2. **写入服务端连接信息**（待管理员回复 `Server EndpointId` 并完成加白后执行）：
   ```bash
   distill configure --server-id <管理员提供的 SERVER_ENDPOINT_ID>

   # 如果管理员同时给了内网直连地址（可选，同内网可加速直连）：
   # distill configure --server-id <SERVER_ENDPOINT_ID> --direct-addr 10.254.144.213:<PORT>
   ```
   配置保存在 `$HOME/.distill-client/client.toml`（可用环境变量 `DISTILL_CLIENT_HOME` 或 `--home` 覆盖目录）。

3. **测试加密通道连通性**：
   ```bash
   distill ping
   ```
   看到 `"Pong"` 即表示本机与远端 `distilld` 已建立端到端加密 QUIC 通道并通过白名单认证。

---

## 3. 交互式 AI 对话 TUI（推荐，直接输入 `distill` 启动）

完成配置后，在终端直接运行 `distill`（或 `distill tui`）即可进入交互式分屏 AI 问答终端：

```bash
distill
```

### 3.1 本地 AI 配置（类似 `pi /model`，内置 12 个主流 Provider）

支持在 **TUI 内直接输入 `/ai`（或 `/model` / 按 `Ctrl+P`）打开交互式选择面板** 热切换，也支持命令行：

```bash
# 打开交互式配置向导（选择 Provider、模型与 API Key）
distill ai

# 查看所有内置 Provider 及当前就绪状态
distill ai list

# 一键切换到常见 Provider（自动套用默认 Base URL 与模型）
distill ai set codex                                        # 本机 Codex CLI（免 Key）
distill ai set claude                                       # 本机 Claude Code CLI（免 Key）
distill ai set openai --api-key sk-xxx                      # OpenAI (gpt-4o-mini)
distill ai set anthropic --api-key sk-ant-xxx               # Anthropic Claude
distill ai set deepseek --api-key sk-xxx                    # DeepSeek (deepseek-chat)
distill ai set gemini --api-key AIza...                     # Google Gemini (gemini-2.5-flash)
distill ai set openrouter --api-key sk-or-xxx               # OpenRouter
distill ai set qwen --api-key sk-xxx                        # 通义千问 DashScope
distill ai set moonshot --api-key sk-xxx                    # Kimi / Moonshot
distill ai set zhipu --api-key xxx                          # 智谱 GLM
distill ai set ollama --model qwen2.5:14b                   # 本地离线 Ollama（免 Key）
distill ai set custom --base-url http://... --model ...     # 自定义 OpenAI 兼容网关
```

### 3.2 界面与复制交互

- **默认开启「选字模式」**：启动后可直接用鼠标自由划选任意文字复制；按 **`Ctrl+S`**（或 `/select`）可随时切换「鼠标按钮点击模式」（点击侧边栏分支 / Fetch / 复制按钮）。
- **一键复制结论**：按 **`Ctrl+Y`**（或 `/copy`）直接将最新 AI 结论写入系统剪贴板；切换到点击模式时也可点击每条回答右上角的 `[📋 复制#N]`。
- **输入框编辑**：支持 `Cmd+U` / `Ctrl+U` / `Cmd+Backspace` 清空输入、`Ctrl+W` 按词回删、`Cmd+V` 粘贴。
- **分支与远端状态**：左侧栏实时显示远端状态与分支列表；按 `Tab` 切到侧边栏 `↑/↓` + `Enter` 切换分支，`Ctrl+F` 触发远端 `git fetch`，`Ctrl+R` 刷新状态。

内置斜杠命令：

| 命令 | 说明 |
|---|---|
| `/vlogs [时间窗] [LogsQL]` | 直查固化 VictoriaLogs（`http://vlogs.hs99.vip:9428`）实时日志（别名 `/logs`），例如 `/vlogs 5m container:twosite-api error` |
| `/ask+log <问题>` | 强制在服务端联合查询 Raw Git 代码 + VictoriaLogs 日志并生成诊断结论（别名 `/diagnose`） |
| `/tree [子目录]` | 快速列出当前分支目录与文件树（全语言通用） |
| `/grep <关键词/正则>` | 高速全文检索当前分支任意文本/代码文件（自动脱敏密钥） |
| `/read <路径> [起行] [止行]` | 直读当前分支任意文本文件（自动脱敏密钥） |
| `/log [条数]` | 查看当前分支最近提交记录（默认 15 条） |
| `/copy [编号]` | 复制最新（或 `#N`）结论到系统剪贴板（快捷键 `Ctrl+Y`） |
| `/select` | 切换鼠标自由划选文字模式（快捷键 `Ctrl+S`） |
| `/raw <关键词>` | 跳过 AI，直接查看原始脱敏语义卡片 |
| `/api <关键词>` | 仅检索 HTTP API 接口契约卡片 |
| `/kind <类型> <关键词>` | 按卡片类型过滤（`api`/`dto`/`error`/`rule`/`entity`/`migration`/`doc`/`constant`） |
| `/diff <基准分支>` | 对比 `<基准分支>` 与当前选中分支的行为与契约差异 |
| `/branch <分支名>` | 通过命令切换对话分支 |
| `/fetch` | 触发远端 `git fetch` 并刷新分支列表 |
| `/status` | 立即刷新远端状态与分支列表 |
| `/clear` | 清空对话记录与多轮记忆 |
| `/help` / `/quit` | 查看帮助 / 退出（也可按 `Esc` 或 `Ctrl+C` 退出） |

> 命令行也可直接提问（服务端自动判断日志意图并并发整合 Raw Git 代码 + VictoriaLogs 日志）：
> `distill ask --repo twosite --branch dev/v1 "最近15分钟 twosite-api 有什么报错？结合源码看下对应逻辑"`
> 或直接查日志：`distill vlogs "container:twosite-api error" --range 15m --raw`

---

## 4. 接入本地 AI（MCP 自动问答）

接入后，你可以直接用自然语言向本地 AI 提问（例如：“帮我查一下 `twosite` 的 `dev/v1` 分支上，价值点（集测1轮）字段 key 是什么、通过方案号怎么查 workItemId，以及相比 `main` 有什么新变化？”），本地 AI 会自动调用 `distill` 获取脱敏语义事实并总结回答。

在你的本地 AI 客户端 MCP 配置中加入：

```json
{
  "mcpServers": {
    "distill": {
      "command": "/Users/<你的用户名>/.local/bin/distill",
      "args": ["mcp"]
    }
  }
}
```

> 若 `distill` 已在系统 PATH 中，`command` 直接写 `"distill"` 即可。

配置完成后，本地 AI 自动获得以下 13 个只读安全工具：

| MCP 工具名 | 用途 |
|---|---|
| `server_status` | 查询远端服务在线状态、运行时间、缓存索引数、VictoriaLogs 端点与仓库同步状态 |
| `list_repos` | 列出你有权查询的仓库清单 |
| `list_branches` | 列出指定仓库下允许查询的分支、最新 commit SHA、提交摘要与索引状态 |
| `fetch_repo` | 触发远端执行 `git fetch --prune` 并返回最新分支列表 |
| `list_tree` | 快速浏览任意分支目录与文件树（全语言通用） |
| `grep_repo` | 在任意分支执行高速全文/正则检索（自动脱敏密钥） |
| `read_file` | 按行区间直读任意分支文本文件（禁止读 `.env`/`.key`/`.pem`，自动脱敏） |
| `get_recent_commits` | 查看指定分支最近提交历史摘要 |
| `query_vlogs` | 查询远端固化 VictoriaLogs（`vlogs.hs99.vip:9428`）实时日志（LogsQL，自动脱敏） |
| `investigate` | 服务端一体化并发整合 Raw Git 代码 + 可选 VictoriaLogs 日志意图证据包 |
| `search_semantics` | 按关键词搜索指定分支的接口、DTO、字段定义、错误码、表结构与文档摘要 |
| `get_api_contracts` | 专项查询指定分支的 HTTP API 路由、方法、鉴权头、权限码与入参类型 |
| `diff_branch_behavior` | 对比同一仓库两个分支（如 `main` vs `dev/v1`）的业务能力与契约差异 |

---

## 5. 直接用 CLI 命令行查询（脚本/调试用）

```bash
# 查看远端服务状态
distill status

# 查看有权查询的仓库
distill repos

# 查看指定仓库可查分支
distill branches --repo twosite

# 触发远端拉取指定仓库最新分支
distill fetch --repo twosite

# 在指定分支按关键词检索语义索引
distill search --repo twosite --branch dev/v1 --query "价值点 集测1轮" --limit 5

# 只筛选某类卡片（api_endpoint / dto_schema / error_contract / entity_schema / migration_fact / doc_section / constant_catalog）
distill search --repo twosite --branch dev/v1 --query "更新工作项" --kind api_endpoint --limit 5

# 对比两个分支之间的业务行为变化
distill diff --repo twosite --base main --target dev/v1
```

---

## 6. 常见问题排查

| 现象 | 原因与处理 |
|---|---|
| `distill ping` 被拒绝或超时 | 1) 确认管理员已把你 `distill print-id` 输出的 ID 加入服务端白名单并重启 `distilld`；2) 核对 `client.toml` 中 `server_id` 是否拼写正确 |
| 返回 `FORBIDDEN` | 当前账号未被授权访问该仓库或该分支，联系管理员调整 `allowed_repos` / `allowed_branches` |
| 返回 `RATE_LIMITED` | 触发单客户端每分钟频率限制，稍等片刻重试 |
| 响应中出现 `[已按源码保护策略隐藏...]` | 属正常安全防护行为，表明出站 DLP 闸门已自动过滤敏感实现细节 |
