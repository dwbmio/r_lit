# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

R_LIT 是一个 Rust 工具集合仓库。每个子目录是**独立的 Cargo crate**（无根 `Cargo.toml` workspace），有各自的依赖和发布周期。

| 目录 | 说明 |
|------|------|
| `bulk_upload/` | 从 JSON 批量提取 URL 并上传至 S3 兼容存储 |
| `img_resize/` | 图片缩放与压缩 |
| `group_vibe_workbench/` | 桌面协作工作台 (GPUI + Murmur P2P) |
| `pgpour/` | Postgres CDC → Kafka 数据管道（多 topic 扇出，按表路由） |
| `omniplan_covers_ding/` | 内部工具 (有外部路径依赖，不可移植) |
| `video-generator/` | 视频工具集 |
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
- **日志:** `fern` + `log`，RFC3339 时间戳。
- **Release profile:** `lto = true, panic = "abort", strip = true, opt-level = "z"`

## Release Process

修改工具 `Cargo.toml` 版本号并 push 到 main，GitHub Actions 自动构建 7 平台目标并发布到 [dev.gamesci-lite.com](https://dev.gamesci-lite.com)。

CI 仅覆盖 `bulk_upload` 和 `img_resize`。

## Just Commands

根 `.justfile` 委派到各工具：
- `just build <tool> <method>`
- `just install_loc <tool> <method>`
- 各工具目录内: `just gen_doc`
