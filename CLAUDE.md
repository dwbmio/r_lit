# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

R_LIT 是一个 Rust **短时运行 CLI 工具**集合仓库。每个子目录是**独立的 Cargo crate**（无根 `Cargo.toml` workspace），有各自的依赖和发布周期。本仓库仅存放"运行、完成、退出"型工具，不存放长时运行的服务或守护进程。

| 目录 | 说明 |
|------|------|
| `bulk_upload/` | 从 JSON 批量提取 URL 并上传至 S3 兼容存储 |
| `img_resize/` | 图片缩放与压缩 |
| `ui-trim/` | 清理 AI 生成 UI 素材的伪透明背景并裁成 tight PNG |
| `textexture/` | 艺术字图片生成（阴影/描边/渐变/发光/霓虹） |
| `mj_atlas/` | 精灵图集 / 打包工具，支持增量构建 |
| `maquette/` | Kit 风格 low-poly 建模 + glTF 导出（GUI + headless CLI） |
| `group_vibe_workbench/` | 桌面协作工作台 (GPUI + Murmur P2P) |
| `deskpet/` | 无边框透明置顶 3D 桌面宠物（Bevy + bevy_egui + 系统托盘 / macOS 菜单栏常驻；GUI，非短时 CLI） |
| `gamereel/` | 把游戏协议回放渲染成短视频（cargo workspace；CUDA + ffmpeg + 多游戏协议插件） |
| `crates/murmur/` | P2P 协作库 |

各工具的架构、用法、API 等详细文档见对应子目录内的 README / docs。

## Build and Development Commands

**重要：** 无根 `Cargo.toml`，每个工具需在其自身目录下构建。

```bash
cd <tool_dir> && cargo build --release
cd <tool_dir> && cargo test
```

也可使用 just（从仓库根目录）：

```bash
just build <tool> release
just install_loc <tool> release
```

## Documentation Standards

遵循 `ci-all-in-one/_ai/rules/doc-organization.md` 双语文档规范。每个子工具/库目录必须包含：

| 文件 | 语言 | 受众 | 说明 |
|------|------|------|------|
| `README.md` | English | 人类 | 完整项目说明、示例、使用场景 |
| `README_CN.md` | 中文 | 人类 | 与英文版对应，自然中文表达 |
| `llms.txt` | English | AI/LLM | 结构化 key-value，纯事实性信息 |
| `llms_cn.txt` | 中文 | AI/LLM | 与英文版对应 |

详细规范见: `/Users/admin/data0/private_work/ci-all-in-one/_ai/rules/doc-organization.md`

## Shared Patterns

- **错误处理:** 各 crate 在 `error.rs` 中用 `thiserror` 定义错误枚举 + `Result<T>` 别名。禁止 `unwrap()`，用 `?`。
- **CLI:** `clap` derive 宏 + `--json` 全局标志 + 子命令架构 + 中英双语帮助。
- **日志:** `log` crate；推荐自定义 `Log` 实现把 INFO+ 转 stdout、全级别捕获到内存 buffer，运行结束 flush 到 `<output>.log` 边车文件（参考实现 `mj_atlas/src/runlog.rs`，依赖 `log` + `humantime`）。RFC3339 时间戳。
- **Release profile:** `lto = true, panic = "abort", strip = true, opt-level = "z"`

## Troubleshooting / 排查问题

报问题时**先看 `<output>.log` 边车文件**，再动代码。详见 `_ai/troubleshooting.md`。
mj_atlas 已落地这套；其他短时 CLI 工具新增 / 改造时复用 `mj_atlas/src/runlog.rs` 的双 sink logger。

## AI Debug Loop Context

相关 AI 技能 / 插件上下文：
- `bbc-ai-skill`
- `https://git.7k7k.com/starlink/bbc-ai-skills`
- `https://github.com/dwbmio/gamesci-cc-plugins`

涉及反复查看日志、修复、重跑、AI 自主联调、微信小程序开发工具日志接入、技能 / 插件调试时，优先考虑使用 `looplog/`。它提供本地 loopback HTTP 摄入、SQLite 24 小时短周期落盘、`looplog list/grep/show --json` 查询路径，以及 TypeScript SDK / 微信适配样例。微信小程序即时接入流程见 `looplog/WECHAT_AI_DEBUG_CN.md`。

## Release Process

**唯一发布通道 = 内网 Jenkins 交叉编译服务**（GitHub Actions 发布路径已移除）。
`ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build` 按工具手动触发（dev.gamesci-lite.com）：
选 `TOOL_NAME`、勾平台（Linux x86_64 native / macOS aarch64 native / Windows x86_64 经 cargo-xwin 交叉编译到 `x86_64-pc-windows-msvc`），
构建产物打包成 `<tool>-<target>.tar.gz`（Windows 为 `.zip`），经 `hfrog_publisher.py` 上传 R2
（`r2.gamesci-lite.com/r_lit/<tool>/v<ver>/`）+ 同步 HFrog（[hfrog.gamesci-lite.com](https://hfrog.gamesci-lite.com)）+ 渲染 `install.sh`。

- 工具事实源是该 Jenkinsfile 的 `toolMap`：新增/改工具时同步登记；不可构建的显式 `buildable: false` 并写明 `note`。
- GUI/桌面工具（`deskpet` `maquette` `group_vibe_workbench`）在 `toolMap` 内声明 Linux 构建所需系统依赖 `system_deps`；
  bevy/GPUI GUI 交叉编译到 Windows（即便 cargo-xwin/msvc）不可靠，故这些工具 `support_windows: false`（仅 Linux/macOS）。
- 安装：`curl -fsSL https://r2.gamesci-lite.com/r_lit/<tool>/install.sh | bash`（模板 `scripts/install.sh.template`，
  Jenkins 用的是 ci-all-in-one 内同步副本；二者须保持一致）。
- `scripts/hfrog_publisher.py` 是 publisher 事实源，Jenkins 通过 `sync-publisher.sh` 同步使用——勿当作废弃脚本删除。

> 注：`release-metadata.json` 原为 GitHub Actions 矩阵源，现已无构建消费方，仅作历史元信息参考；
> 工具构建事实源以 Jenkins `toolMap` 为准。

## Just Commands

根 `.justfile` 委派到各工具：
- `just build <tool> <method>`
- `just install_loc <tool> <method>`
- 各工具目录内: `just gen_doc`
