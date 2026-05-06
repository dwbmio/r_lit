# R_LIT

Rust 独立**短时运行** CLI 工具与库的集合仓库。每个子目录是独立的 Cargo crate，无根 workspace。本仓库专为"运行、完成、退出"型工具设计，不存放长时运行的服务或守护进程。

主站: [https://www.snappywatt.com/](https://www.snappywatt.com/)

可用的预编译 binary 均已托管到 [snappywatt.com](https://www.snappywatt.com/)，可直接下载使用。

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)
[![Release](https://github.com/dwbmio/r_lit/workflows/Release/badge.svg)](https://github.com/dwbmio/r_lit/actions)

## 目录

| 目录 | 说明 |
|------|------|
| [bulk_upload](bulk_upload/) | 从 JSON 批量提取 URL 并上传至 S3 兼容存储 |
| [img_resize](img_resize/) | 图片缩放与压缩 |
| [textexture](textexture/) | 艺术字图片生成（阴影 / 描边 / 渐变 / 发光 / 霓虹） |
| [mj_atlas](mj_atlas/) | 精灵图集 / 打包工具，支持增量构建 |
| [maquette](maquette/) | Kit 风格 low-poly 建模 + glTF 导出（GUI + headless CLI） |
| [group_vibe_workbench](group_vibe_workbench/) | 桌面协作工作台 (GPUI + P2P) |
| [video-generator](video-generator/) | 视频工具集 |
| [crates/murmur](crates/murmur/) | P2P 协作库 (iroh-net + Automerge CRDT) |

详细使用说明请进入各子目录查看对应 README。

## 安装预编译二进制

所有 CLI 工具同时发布到三处：
- **GitHub Releases** — `https://github.com/dwbmio/r_lit/releases`
- **Cloudflare R2（镜像，稳定 URL）** — `https://gamesci-lite.com/r_lit/<tool>/`
- **HFrog 制品中心** — `https://hfrog.gamesci-lite.com/api/release/softwares/<tool>`

一行安装（自动识别 Linux / macOS / Windows-Git-Bash）：

```bash
curl -fsSL https://gamesci-lite.com/r_lit/<tool>/install.sh | bash
```

示例：

```bash
curl -fsSL https://gamesci-lite.com/r_lit/bulk_upload/install.sh | bash
curl -fsSL https://gamesci-lite.com/r_lit/img_resize/install.sh  | INSTALL_DIR=$HOME/.local/bin bash
```

GUI 应用（`maquette`、`group_vibe_workbench`）在 macOS 以
notarized `.dmg` 形式发布，请到 GitHub Release 页面下载。

## 构建

```bash
cd <tool_dir> && cargo build --release
```

## 发布

任何 `<tool>/Cargo.toml` 的 `version` 字段变更并 push 到 `main` 后，
`Release` workflow 会自动：

1. 按 `release-metadata.json` 中该工具声明的 `targets` 构建。
2. 创建 GitHub Release，附带二进制 + `SHA256SUMS` + 每个产物的校验和。
3. 同步到 R2（`s3://prod-gamesci-lite/r_lit/<tool>/v<ver>/`），
   并刷新 `r_lit/<tool>/install.sh` 入口脚本。
4. 把 `software / version / release / platform` 全套记录写入 HFrog，
   带上真实的 `file_size`、`checksum_sha256`、`source_type`、
   `install_script_url`。

新增工具：只需在 `release-metadata.json` 中追加一项
（`description` / `category` / 可选 `gui`、`macos_app_name`、`targets`），
其余文件无需改动。

完整流程图与所需的 GitHub Secrets 见 [`docs/release.md`](docs/release.md)。

## License

See LICENSE file in each project.
