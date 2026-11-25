# `distilld` 服务端远端部署与运维指南

> **适用对象**：服务端部署与维护人员（内网 Linux 主机，如 `ubuntu-hs-main`）。  
> **切勿将本机的 `server.key`、`master.key` 或 `config.toml` 转发给客户端使用者。**

---

## 1. 运行环境与目录结构

- **主机要求**：Linux `x86_64`（或其他 Rust 支持架构）、已安装 `git`（用于 Bare 仓拉取与多 Worktree）。
- **默认数据根路径**：`$HOME/.distill`（可通过环境变量 `DISTILL_HOME` 或 `--home <DIR>` 覆盖）

```text
$HOME/.distill/
├── bin/
│   └── distilld          # 服务端单二进制
├── server.key            # Ed25519 节点私钥（自动生成，权限 0600，切勿外泄）
├── master.key            # 静态缓存信封加密主密钥（自动生成，权限 0600，切勿外泄）
├── config.toml           # 仓库白名单、分支规则、客户端 EndpointId 权限配置
├── repos/                # Bare Git 镜像（多分支共享对象存储）
├── worktrees/            # 按需 detached 只读多 Worktree 池（LRU 自动回收）
├── blob-cache/           # 按 blob_oid 跨分支复用的 AEAD 加密语义缓存
├── indexes/              # 按 (repo, commit_sha) 隔离的 Tantivy 索引
└── audit.jsonl           # 全量访问与 DLP 拦截审计日志
```

---

## 2. 编译与安装二进制

### 方式 A：直接在目标机（如 `ubuntu-hs-main`）源码编译（推荐）

```bash
# 同步或拉取 r_lit/distill 到远端后执行
cd ~/r_lit/distill
cargo build --release -p distill-server

mkdir -p "$HOME/.distill/bin"
install -m 755 target/release/distilld "$HOME/.distill/bin/distilld"
```

### 方式 B：本机交叉编译 Linux 静态二进制后上传

```bash
cd /Users/admin/data0/private_work/r_lit/distill
cargo build --release -p distill-server --target x86_64-unknown-linux-musl

ssh ubuntu-hs-main 'mkdir -p ~/.distill/bin'
scp target/x86_64-unknown-linux-musl/release/distilld ubuntu-hs-main:~/.distill/bin/distilld
```

---

## 3. 初始化配置与获取服务端 `EndpointId`

在远端主机执行：

```bash
# 接入远端 Git 仓库（会自动生成 server.key、master.key 与 config.toml）
~/.distill/bin/distilld init --repo-url https://git.7k7k.com/starlink/twosite.git

# 或指向远端主机上已存在的本地仓库 / 裸仓路径：
# ~/.distill/bin/distilld init --repo-path /home/hungry/repos/twosite.git
```

输出示例：

```text
Initialized Distill home: /home/hungry/.distill
Server EndpointId: <64位十六进制公钥>
Config path: /home/hungry/.distill/config.toml
```

> 把 **`Server EndpointId`**（以及可选的内网直连 `IP:PORT`）发给客户端使用者即可；任何时候都可用 `~/.distill/bin/distilld print-id` 重新查看。

---

## 4. 配置仓库与给新成员加白（`$HOME/.distill/config.toml`）

客户端执行 `distill print-id` 会得到自己的 `EndpointId`，管理员将其加入 `[[clients]]`：

```toml
max_concurrent_builds = 4
max_worktrees = 16
rate_limit_per_min = 120
allow_any_peer_dev_only = false

[[repos]]
name = "twosite"
description = "Rigel / TwoSite 项目工作管理与流程编排平台"
remote_url = "https://git.7k7k.com/starlink/twosite.git"
default_branch = "main"
allowed_branches = ["main", "dev/*"]
fetch_interval_secs = 30

[[clients]]
endpoint_id = "<使用者执行 distill print-id 得到的公钥>"
label = "pm-zhangsan"
allowed_repos = ["twosite"]
allowed_branches = ["main", "dev/*"]
```

- **撤销某人权限**：删除对应 `[[clients]]` 并重启 `distilld`，立即失效。
- **Git 拉取鉴权**：若 `remote_url` 为私有仓，请确保运行 `distilld` 的系统用户已配好 Git 凭据（如 `.git-credentials` 或只读 Deploy Key）。

---

## 5. 启动与 systemd 常驻托管

### 前台验证启动

```bash
~/.distill/bin/distilld serve
```

启动后会打印：
- `SERVER_ENDPOINT_ID=<公钥>`
- `SERVER_ADDR_JSON={...}`（其中 `Ip` 项为当前监听的内网直连地址，可选填给同内网客户端做加速）

### 配置 systemd 常驻（推荐生产/长期运行）

参考仓库内模板 `deploy/distilld.service`：

```bash
sudo cp deploy/distilld.service /etc/systemd/system/distilld.service
# 如主机用户名不是 hungry，请先修改 User= 与 ExecStart= 路径
sudo systemctl daemon-reload
sudo systemctl enable --now distilld
sudo systemctl status distilld
```

---

## 6. 日常运维、预热与审计排查

```bash
# 查看服务运行日志
journalctl -u distilld -f

# 本地直接验证索引与脱敏结果（不走网络）
~/.distill/bin/distilld local-search \
  --repo twosite --branch dev/v1 --query "价值点 集测1轮" --limit 5

# 查看访问与 DLP 拦截审计日志（JSONL）
tail -n 50 ~/.distill/audit.jsonl
```

每条 `audit.jsonl` 记录包含：`ts`、`peer_id`、`label`、`action`、`repo`、`branch`、`dlp_redactions`、`status`（`OK` / `DENIED` / `RATE_LIMITED`）。
