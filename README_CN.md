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
| [group_vibe_workbench](group_vibe_workbench/) | 桌面协作工作台 (GPUI + P2P) |
| [omniplan_covers_ding](omniplan_covers_ding/) | 内部工具 (有外部路径依赖) |
| [video-generator](video-generator/) | 视频工具集 |
| [crates/murmur](crates/murmur/) | P2P 协作库 (iroh-net + Automerge CRDT) |

详细使用说明请进入各子目录查看对应 README。

## 构建

```bash
cd <tool_dir> && cargo build --release
```

## License

See LICENSE file in each project.
