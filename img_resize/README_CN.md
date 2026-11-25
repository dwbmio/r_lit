# img_resize

> 本地离线游戏纹理体积优化与图片缩放工具（PNG / JPG / WebP），零网络依赖。

## 快速开始

### 1. 按渠道配置优化纹理（`optimize $engine_$channel.yml`）

面向 Godot WASM 微信/QQ/抖音/支付宝小游戏等渠道，内置本地 `imagequant`（TinyPNG 同源算法）+ PSNR 质量无感兜底 + `oxipng`、假透明剥离、透明区 RGB 归零与边界扩色、有效像素占比审计，以及可选 JPG/WebP 转码：

```bash
# 查看内置 $engine_$channel 预设
img_resize profile list

# 生成带完整注释的渠道模板（如 godot_wx-minip.yml）
img_resize profile init godot_wx-minip

# 先 dry-run 审计（查看有效像素占比告警与预计压缩率，不写图片）
img_resize optimize godot_wx-minip.yml ./assets -o ./assets_wx --dry-run --json

# 执行优化并落盘 JSON 报告
img_resize optimize godot_wx-minip.yml ./assets -o ./assets_wx --report report.json --json
```

仓库 `profiles/` 目录下已内置开箱即用的渠道模板：
- `profiles/godot_wx-minip.yml` — Godot WASM 微信小游戏
- `profiles/godot_qq-minip.yml` — Godot WASM QQ 小游戏
- `profiles/godot_dy-minip.yml` — Godot WASM 抖音小游戏
- `profiles/godot_zfb-minip.yml` — Godot WASM 支付宝小游戏
- `profiles/godot_mobile.yml` — Godot 原生移动端（iOS/Android；保持源图无损供 ASTC/ETC2 编码）
- `profiles/raw_png-jpg.yml` — 通用 PNG/JPG 渠道

> **命名约定**：配置文件按 `$engine_$channel.yml` 命名（如 `godot_wx-minip.yml`）时，即使 YAML 内省略 `engine`/`channel`，也会自动从文件名推断并继承对应渠道的全部最佳实践默认值，只写需要覆盖的字段即可。

### 2. 基础缩放（`r_resize`）

```bash
# 等比缩放到最大 800px（Lanczos3）
img_resize r_resize -m 800 image.jpg -o out/

# 精确调整到 1920x1080
img_resize r_resize --rw 1920 --rh 1080 image.jpg -o out/

# 批量处理目录
img_resize r_resize -m 1024 images/ -o out/

# 按 YAML 一次生成多个尺寸
img_resize r_resize -c sizes.yaml icon.png
```

## 纹理优化管线说明

1. **有效像素占比与 Alpha 清理**
   - **假透明剥离**（`strip_fake_alpha: true`）：全图不透明（`alpha >= 252`）的 RGBA PNG 自动剥离无用 Alpha 通道变为 RGB8，布局与画质零影响。
   - **透明区 RGB 归零 + 边界扩色**（`zero_transparent_rgb: true`、`edge_bleed_px: 1`）：在保留可见边缘 1px 扩色（避免 GPU 双线性采样黑边）的前提下，将完全透明区域的残留脏 RGB 置零，纯无损大幅提升 PNG 压缩率。
   - **有效像素占比审计**：自动统计 `effective_pixel_ratio` 与 `bbox_area_ratio`，对低占比大户输出告警（建议用 `mj_atlas` 打图集）。为保护 Godot 节点 `pivot`/`anchor`/九宫格/UV，`trim_transparent` **默认关闭**，可按 `rules:` 对安全目录白名单开启。
2. **质量尽量无感的本地离线压缩（替代 TinyPNG）**
   - **PNG**：本地 `imagequant`（质量区间 `75..95`）+ **PSNR 失真兜底**（默认 `min_psnr_db: 37~39 dB`，复杂渐变低于阈值自动回退无损 PNG）+ `oxipng` 最优 filter/deflate 与去元数据。
   - **JPG**：自动去除 EXIF/ICC/XMP 元数据，可配质量（`80..88`）。
   - **WebP**：支持有损 VP8（含 Alpha）与无损 VP8L。
   - **体积不增保护**（`no_regression: true`）：未缩放/未裁切/未转格式时，若压完反而变大或省幅不足 `min_saving_pct`，自动保留原文件。
3. **内置 Godot 资产保护规则（`inherit_default_rules: true`）**
   - 自动保护法线/ORM/遮罩/粗糙度/金属度/高度图（`**/*_{normal,n,norm,orm,roughness,metallic,mask,height,bump,ao}.*`）、字体目录（`**/fonts/**`）与像素风素材（`**/*_pixel*.*`），避免被有损量化。

## 支持格式

- **输入**：PNG、JPEG、WebP、BMP、GIF、TIFF
- **输出**：PNG、JPEG、WebP

## JSON 输出与机器调用契约

```bash
img_resize --json optimize profiles/godot_wx-minip.yml ./assets -o ./out
img_resize __schema
```

遵循 `r_lit` CLI 契约 v1（stdout 单 JSON envelope、退出码 `0..5`、完整 `artifacts` 回执）。

## 从源码构建

```bash
cd img_resize && cargo build --release
```

## 备注

- `tinyfy` 子命令因 OpenSSL/musl 静态编译冲突已禁用；请改用本地离线的 `optimize` 子命令（`imagequant` + `oxipng`）。
- 许可提示：本地 PNG 量化依赖 `imagequant`（GPL-3.0，与 `mj_atlas` 一致）。
