# ui-trim

`ui-trim` 用于把 AI 生成的 UI 素材 PNG 清理成 tight 透明 PNG。

它会移除与图片边缘连通的伪透明背景、白/灰棋盘格像素，以及可选的红色裁切辅助线；随后按最终 alpha 边界框裁切，并保留可配置 padding。

## 为什么需要

图像模型有时不会输出真正透明像素，而是把透明棋盘格、白/灰底、暖色底或红色裁切线画进图片。普通 alpha trim 只能裁掉真实透明像素，无法处理这些伪透明背景。`ui-trim` 是本地确定性工具，适合放进素材流水线，不需要额外 AI 请求。

## 安装

源码构建：

```bash
cargo build --release
```

从仓库根目录构建：

```bash
just build ui-trim release
```

## 用法

```bash
ui-trim \
  --input raw.png \
  --output clean.png \
  --padding 6 \
  --alpha-threshold 4 \
  --feather 2 \
  --max-bg-distance 48 \
  --remove-red-guides \
  --json
```

## 参数

- `--input <PNG>`：输入图片路径。
- `--output <PNG>`：输出图片路径。
- `--padding <PX>`：最终 alpha 边界框外扩像素。默认 `6`。
- `--alpha-threshold <N>`：alpha 小于等于该值时视为背景。默认 `4`。
- `--feather <PX>`：对被移除背景附近的前景 alpha 做轻量软化。范围 `0..3`，默认 `2`。
- `--max-bg-distance <N>`：边缘 matte 聚类的 RGB 距离阈值。默认 `48`。
- `--remove-red-guides`：移除与边缘连通的红色裁切辅助线。
- `--json`：输出机器可读元数据。

## JSON 输出

```json
{
  "ok": true,
  "input_width": 1408,
  "input_height": 480,
  "output_width": 1396,
  "output_height": 468,
  "trim_bbox": [4, 6, 1395, 467],
  "padding_px": 6,
  "removed_pixels": 236690,
  "alpha_ratio": 0.31,
  "throughput_mp_s": 120.5,
  "options": {
    "padding_px": 6,
    "alpha_threshold": 4,
    "feather_px": 2,
    "max_bg_distance": 48.0,
    "remove_red_guides": true,
    "implementation": "pure_rust_cpu",
    "acceleration": "specialized_u8_mask_kernel; png codec dependencies may use CPU intrinsics internally"
  },
  "timings_ms": {
    "decode": 2.1,
    "sample_matte": 0.03,
    "flood_fill": 4.2,
    "morphology": 3.8,
    "alpha_cleanup": 1.6,
    "bbox_crop": 1.1,
    "encode": 5.4,
    "total": 18.3
  },
  "warnings": []
}
```

这些 meta 字段是刻意为 AI / 自动流水线设计的：

- `timings_ms` 用于判断瓶颈在 PNG 编解码还是 mask 算法。
- `options` 记录归一化后的参数与当前实现路径。
- `alpha_ratio`、`removed_pixels`、`warnings` 可作为自动质检信号。

## 算法

1. 使用纯 Rust `image` crate 解码 PNG 到 RGBA8。
2. 从图片边缘采样 matte 颜色聚类。
3. 只 flood-fill 与边缘连通的 matte 像素，避免误删 UI 内部浅色区域。
4. 对背景 mask 做 1px close/open 形态学稳定处理。
5. 清零背景 alpha，并可选软化邻近前景 alpha。
6. 计算最终 alpha bbox，按 padding 外扩后裁切并写出 PNG。

默认实现刻意不引入 OpenCV。本任务热路径是 RGBA 线性扫描和小半径 mask 操作，MVP 阶段不值得承担 OpenCV C++ runtime 与动态库部署成本。

SIMD/GPU 策略：

- SIMD 必须由 benchmark 驱动，只有 morphology/feather 被证明是瓶颈时才加，并放在可选 feature 后。
- 候选 SIMD crate：mask kernel 可看 `fast_morphology`，未来 resize 可看 `fast_image_resize`。
- 默认不用 GPU，因为上传/同步和部署复杂度会吞掉当前单图操作收益。

## Benchmark

运行：

```bash
cargo run --release --example benchmark
```

本机最新 synthetic benchmark：

- `512x512`，30 次迭代：平均算法耗时约 `18.5 ms`，约 `20.0 MP/s`。
- `1024x1024`，15 次迭代：平均算法耗时约 `56.7 ms`，约 `20.7 MP/s`。
- `2048x2048`，5 次迭代：平均算法耗时约 `197.7 ms`，约 `21.5 MP/s`。

当前瓶颈在 mask 处理（`flood_fill`、`morphology`、`alpha_cleanup`）。如果生产调用量需要继续压耗时，第一优先级应该是 morphology/feather 的 SIMD 或专用 mask-kernel 实现。

## 开发

```bash
cargo test
cargo run -- --input raw.png --output clean.png --json
cargo run --example smoke
cargo run --release --example benchmark
```
