# Release Pipeline

> **唯一发布通道 = 内网 Jenkins 交叉编译服务。** GitHub Actions 发布路径
> （旧 `.github/workflows/release.yml` + `release-metadata.json` 矩阵 + GitHub Release）**已移除**。
> Pipeline 定义在 `ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build`。

## Pipeline overview

```
Jenkins r_lit-binary-build  (参数: TOOL_NAME / GIT_BRANCH / BUILD_LINUX|MACOS|WINDOWS)
       │  toolMap 是工具事实源 (dir/binary/buildable/description/category/
       │  r2_prefix/cargo_features/system_deps/support_windows)
       ▼
┌──────────────┐  并行构建 (Linux/macOS 原生 / Windows 经 cargo-xwin 交叉编译)
│  build       │  Linux  → x86_64-unknown-linux-gnu   (native)
│              │  macOS  → aarch64-apple-darwin        (native)
│              │  Windows→ x86_64-pc-windows-msvc      (cargo-xwin cross; GUI 工具默认跳过)
└──────┬───────┘  产物: <tool>-<target>.tar.gz  (Windows: <tool>-<target>.zip)
       ▼
┌──────────────┐  hfrog_publisher.py (经 ci-all-in-one jenkins-publish.sh 同步调用)
│  publish     │  R2 (bucket prod-hfrog):
│              │   → r_lit/<tool>/install.sh                (latest，模板渲染)
│              │   → r_lit/<tool>/v<ver>/<assets>           (immutable)
│              │  HFrog (https://hfrog.gamesci-lite.com):
│              │   software / platform / version / release  (file_size, checksum_sha256,
│              │   source_type, install_script_url)；category 可选经 psql patch
└──────────────┘
```

`scripts/hfrog_publisher.py` 是 publisher 事实源；Jenkins 通过 `sync-publisher.sh`
同步成 `ci-all-in-one/scripts/services/hfrog/publisher.py` 使用——**勿当作废弃脚本删除**。
`scripts/install.sh.template` 与 ci-all-in-one 内同步副本须保持字节一致（CI 用后者渲染）。

## 凭证 (Jenkins credentials)

R2 / HFrog 凭证以 Jenkins credentials 形式注入（见 Jenkinsfile `withCredentials`）：
`r2-hfrog-endpoint` / `r2-hfrog-access-key-id` / `r2-hfrog-secret-access-key` /
`hfrog-postgres-url`（可选，启用 `category_id` 的 SQL patch）。
原始值见 `/Users/.../ci-all-in-one/secrets/.credentials.env`。R2 bucket `prod-hfrog`
绑定公网域名 `r2.gamesci-lite.com`。

## 添加 / 发布一个工具

1. 创建 crate（`<tool>/Cargo.toml`），补齐 README/README_CN/llms/llms_cn。
2. 在 `Jenkinsfile.binary-build` 的 `toolMap` 登记：`dir` / `binary` / `buildable` /
   `description` / `category` / `source_type` / `r2_prefix`；GUI 工具补 `cargo_features`
   （如 `gui`）、`system_deps`（Linux 系统库）、`support_windows: false`（bevy/GPUI GUI 交叉编 Windows 不可靠）。
   不可构建的工具显式 `buildable: false` 并写明 `note`。
3. 触发 Jenkins job，选 `TOOL_NAME` + 平台。首次发布会建好 HFrog `software` 行与
   R2 上的 `r_lit/<tool>/install.sh`。
4. 验证：R2 `install.sh` 与 `v<ver>/<tool>-<target>.{tar.gz,zip}`、HFrog metadata 的
   `file_size` / `checksum_sha256`。

## One-shot remediation

`scripts/repatch_hfrog.sh` 用于回填历史 release 的 `install_script_url` /
`file_size` / `checksum_sha256` / `source_type` / `release_notes` / `category_id`，
以及清理 `_probe_*` 残留行（hfrog 无 DELETE，需 raw SQL）。本地 source 凭证后运行，幂等：

```bash
source /Users/admin/data0/private_work/ci-all-in-one/secrets/.credentials.env
bash scripts/repatch_hfrog.sh
```

## Migration note: Nexus → Cloudflare R2

r_lit 二进制分发已从 Nexus（`nexus.gamesci-lite.com`）迁到 Cloudflare R2。
旧 `nexus.gamesci-lite.com/repository/raw-prod/r_lit/...` URL 全部失效。现行 URL：

```
https://r2.gamesci-lite.com/r_lit/<tool>/install.sh
https://r2.gamesci-lite.com/r_lit/<tool>/v<ver>/<tool>-<target>.tar.gz   # Windows: .zip
```

ci-all-in-one 内残留 Nexus 引用的扫描见
[`docs/nexus-deprecation-audit.md`](nexus-deprecation-audit.md)。
