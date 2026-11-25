# Distill

100% Pure Rust **跨职能信息收集与安全蒸馏套件**，拆分为 **2 个独立单二进制** 实现。Distill 将内部 Git 仓库（支持多仓库、多分支并发）中的工程实现、接口契约、字段定义、业务规则与文档**安全蒸馏为脱敏语义事实**，让产品、测试、运营、设计等跨职能同学借助**本地 AI** 自助查证任意分支的真实逻辑，同时**绝不暴露源码原文与 SSH 权限**。

- **`distilld`**（服务端守护进程，部署于内网主机如 `ubuntu-hs-main`）：维护 Git Bare 镜像与按需 detached 只读多 Worktree 池，通过 Pure Rust AST（`oxc_parser`）与 Markdown/JSON 解析器提取无源码的脱敏语义卡，按 `blob_oid` 使用 HKDF-SHA256 + ChaCha20-Poly1305 AEAD 加密缓存跨分支复用，配合 Tantivy 按 `commit_sha` 并发检索、BLAKE3 6-gram 源码指纹 DLP 防泄漏闸门，并通过 `iroh` P2P QUIC（TLS 1.3 + Ed25519 `EndpointId` 白名单 + 限流 + `audit.jsonl` 审计）对外提供并发服务。
- **`distill`**（交互式 TUI / CLI / MCP 桥接，分发给跨职能成员本机）：极轻量单二进制（不含任何 Git 拉取或源码解析代码），通过 `iroh` QUIC 加密直连/穿透服务端，默认启动交互式 **Ratatui 对话 TUI**（左侧实时展示远端服务状态、支持鼠标点击 `[⟳ Fetch]` 拉取与点击切换分支），同时提供 CLI 与标准 **MCP（Model Context Protocol）stdio Server**。

## 分角色操作手册与技能定义

| 受众 | 文档 | 说明 |
|---|---|---|
| AI Agent 技能卡（对齐 `starlink-nebula`） | [SKILL.md](SKILL.md) | 触发词、MinIO 一键安装检查、CLI/TUI/MCP 查证路由与示例 |
| 服务端部署与运维（如 `ubuntu-hs-main`） | [docs/SERVER_DEPLOY_CN.md](docs/SERVER_DEPLOY_CN.md) | 编译/上传 `distilld`、初始化 `$HOME/.distill`、配置仓库与客户端加白、systemd 常驻与审计日志 |
| 本地使用者（非研发 / 本地 AI 接入） | [docs/CLIENT_USAGE_CN.md](docs/CLIENT_USAGE_CN.md) | MinIO 一键安装 `distill`、生成本机 `EndpointId` 申请加白、TUI/CLI/MCP 使用（可直接单独转发） |
| CI/CD 发布流水线 | [Jenkinsfile](Jenkinsfile) / [.justfile](.justfile) | 多平台构建 + gzip 压缩 + 发布到 MinIO `starlink-artifacts/distill/` |

## 一键安装（MinIO，与 `starlink-nebula` 同源）

```bash
bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
```

## 扩展边界（无外部脚本）

- 整套构建、部署与语义抽取链路**不引入任何 Shell / Python / Node 外部脚本**，全部由 Rust 原生实现。
- 后续若需针对特定仓库扩展可编程语义抽取或自定义 DLP 规则，仅允许在进程内引入受限沙箱 **`mlua`（Luau）**（可控内存与指令预算、禁止宿主机 IO/子进程），不外挂脚本解释器。

## 运行根路径约定

- 服务端默认根路径：`$HOME/.distill`（可用 `DISTILL_HOME` 或 `--home` 覆盖）
  - `server.key`：持久化 Ed25519 节点私钥（`0600`）
  - `master.key`：静态语义缓存信封加密主密钥（`0600`）
  - `config.toml`：仓库白名单、分支规则与客户端 `EndpointId` 权限表
  - `repos/`：Bare Git 镜像
  - `worktrees/`：按需只读多 Worktree 池（`Arc` 租约 + LRU 自动回收）
  - `blob-cache/`：按 `blob_oid` 跨分支复用的 AEAD 加密语义缓存
  - `indexes/`：按 `(repo, commit_sha)` 隔离的 Tantivy 倒排索引
  - `audit.jsonl`：全量访问与 DLP 拦截审计日志
- 客户端默认根路径：`$HOME/.distill-client`（可用 `DISTILL_CLIENT_HOME` 或 `--home` 覆盖）

## 构建与测试

```bash
cd distill
cargo test --workspace
cargo build --release
```

产出两个独立可执行文件（位于 `target/release/`）：
- `distilld`
- `distill`

## 快速上手

```bash
# 1. 服务端初始化并启动
distilld init --repo-url https://git.7k7k.com/starlink/twosite.git
distilld serve

# 2. 客户端获取本机 EndpointId 发给管理员加白，配置并启动交互式 TUI
distill print-id
distill configure --server-id <SERVER_ENDPOINT_ID>
distill            # 直接启动交互式 TUI 对话终端（左侧远端状态 + 点击 Fetch / 切换分支）
distill status
distill fetch --repo twosite
distill search --repo twosite --branch dev/v1 --query "价值点 集测1轮"
distill diff --repo twosite --base main --target dev/v1

# 3. 接入本地 AI（Antigravity / Claude Code / Cursor / Codex）
distill mcp
```

本地 AI 的 MCP 配置示例：

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
