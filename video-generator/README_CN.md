# video-generator

> 基于 FFmpeg 的 Rust 视频生成工具集。

## 子项目

| 目录 | 说明 |
|------|------|
| [movie-maker](movie-maker/) | 核心库 — 基于 FFmpeg 的编程式视频生成，图像合成，补间动画 |
| [demo](demo/) | 演示应用 (hs-mvp) — movie-maker 的使用示例，场景渲染 |

## movie-maker

用代码生成视频的库。支持：

- 基于 FFmpeg 的视频编码
- `image` + `imageproc` 图像合成
- 补间 (tween) 动画系统
- 性能基准测试二进制 (`perf_main`)

## demo (hs-mvp)

使用 `movie-maker` 渲染场景并输出视频的演示应用。

## 构建

```bash
# 构建库
cd movie-maker && cargo build --release

# 构建演示
cd demo && cargo build --release
```

**前置依赖：** 系统需安装 FFmpeg 开发库。

## License

See LICENSE file.
