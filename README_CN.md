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
| [ui-trim](ui-trim/) | 清理 AI 生成 UI 素材的伪透明背景并裁成 tight PNG |
| [textexture](textexture/) | 艺术字图片生成（阴影 / 描边 / 渐变 / 发光 / 霓虹） |
| [mj_atlas](mj_atlas/) | 精灵图集 / 打包工具，支持增量构建 |
| [looplog](looplog/) | 本地日志摄入 / 查询工具，服务 AI 辅助联调（微信小程序 MVP；见 [微信 AI 调试指南](looplog/WECHAT_AI_DEBUG_CN.md)） |
| [maquette](maquette/) | Kit 风格 low-poly 建模 + glTF 导出（GUI + headless CLI） |
| [group_vibe_workbench](group_vibe_workbench/) | 桌面协作工作台 (GPUI + P2P) |
| [deskpet](deskpet/) | 无边框透明置顶 3D 桌面宠物（Bevy + 系统托盘 / 菜单栏常驻） |
| [video-generator](video-generator/) | 视频工具集 |
| [crates/murmur](crates/murmur/) | P2P 协作库 (iroh-net + Automerge CRDT) |

详细使用说明请进入各子目录查看对应 README。

## 安装预编译二进制

所有 CLI 工具由内网 Jenkins 交叉编译服务发布到：
- **Cloudflare R2（稳定 URL，主通道）** — `https://r2.gamesci-lite.com/r_lit/<tool>/`
- **HFrog 制品中心** — `https://hfrog.gamesci-lite.com/api/release/softwares/<tool>`

一行安装（自动识别 Linux / macOS / Windows-Git-Bash）：

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/<tool>/install.sh | bash
```

示例：

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/bulk_upload/install.sh | bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/img_resize/install.sh  | INSTALL_DIR=$HOME/.local/bin bash
```

GUI 应用（`maquette`、`group_vibe_workbench`、`deskpet`）按平台以原始
二进制 tar 包发布（Linux/macOS），安装方式相同。

## 构建

```bash
cd <tool_dir> && cargo build --release
```

## 发布

唯一发布通道是内网 **Jenkins 交叉编译服务**
（`ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build`），已无 GitHub Actions 发布路径。
手动触发该任务、选 `TOOL_NAME` 与平台后，它会：

1. 按平台构建（Linux x86_64 原生 / macOS aarch64 原生 / Windows x86_64 经 MinGW
   `x86_64-pc-windows-gnu` 交叉编译）。
2. 打包成 `<tool>-<target>.tar.gz`（Windows 为 `.zip`）。
3. 上传 R2（`s3://prod-hfrog/r_lit/<tool>/v<ver>/`）并刷新 `r_lit/<tool>/install.sh`。
4. 把 `software / version / release / platform` 全套记录写入 HFrog，
   带上真实的 `file_size`、`checksum_sha256`、`source_type`、`install_script_url`。

工具清单的事实源是 Jenkinsfile 的 `toolMap`。

完整流程图见 [`docs/release.md`](docs/release.md)。

## License

See LICENSE file in each project.
