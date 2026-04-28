# mj_atlas

[English →](README.md)

**mj_atlas** 是一个面向游戏引擎的纹理图集打包工具——单二进制、零依赖、命令行优先。给一个 sprite 目录，输出打包好的 atlas PNG 加上引擎能直接吃的元数据：TexturePacker 兼容 JSON、Godot 4 原生 `.tres`，或者你自己接上的下游格式。

mj_atlas 还是少数支持 **UV 稳定增量打包**的开源工具——改完一个 sprite 重新跑，未变的 sprite 在新 atlas 里位置完全不变。已经发布的游戏端不用重新 bake UV，直接换 PNG 即可工作。

```
sprite_dir/  ──────►   atlas.png  +  atlas.json     （TexturePacker 兼容）
                        atlas.manifest.json         （增量打包缓存）
```

## 特性

- **MaxRects 装箱算法**，可选 90° 旋转、边缘 extrude、自定义 spacing/padding、强制 POT 尺寸
- **多图集自动拆分**——单个 atlas 装不下时自动开新图
- **透明像素裁剪**，可调 alpha 阈值（`--trim-threshold`）
- **多边形网格输出** —— `--polygon` 按连通域提取轮廓，做 Douglas-Peucker 简化、earcut 三角化，输出 `vertices` / `verticesUV` / `triangles`，引擎据此可绕过透明像素，减少 30%+ 的 overdraw
- **多连通域网格** —— 一张 sprite 里有 3 个不相连的图标？每个独立提轮廓 + 三角化，合成一份网格输出
- **多边形形状模式** —— `--polygon-shape concave|convex|auto`，配合 `--max-vertices N` 顶点预算
- **PNG 有损量化** —— imagequant 后端（`--quantize`），文件体积通常缩小 60-70%
- **重复 sprite 检测** —— SHA256 + 廉价预筛 cheap-key，重复者作为别名共享一个位置
- **动画自动归组** —— `name_NN.ext` 命名的会自动识别为序列帧（TexturePacker `animations` 字段、Godot `SpriteFrames`）
- **UV 稳定的增量打包**（见下文）
- **可选 GUI**（`--features gui`）—— egui + wgpu，拖拽导入 sprite、内嵌预览、网格 overlay
- **四种输出格式开箱即用** —— TexturePacker JSON Hash / Array、Godot `.tpsheet`（需插件）、Godot 原生 `.tres`（**无需任何插件**）

## 安装

CI 流水线会把预编译二进制发到 [dev.gamesci-lite.com](https://dev.gamesci-lite.com)，覆盖 macOS arm64/x86_64、Linux x86_64/aarch64、Windows x86_64。也可以本地构建：

```bash
cd mj_atlas
cargo build --release                    # 仅 CLI
cargo build --release --features gui     # CLI + GUI
```

二进制位于 `target/release/mj_atlas`。

## 上手

```bash
mj_atlas pack ./sprites -o atlas --trim --pot
```

输出 `./sprites/atlas.png` 和 `./sprites/atlas.json`。

完整 demo（包括程序化生成的 sprite 和所有有趣的参数组合）见 [`examples/run_demo.sh`](examples/run_demo.sh)：

```bash
python3 examples/gen_sprites.py    # 生成 13 个 demo sprite
examples/run_demo.sh               # 5 种典型预设跑一遍 mj_atlas
```

## 输出格式

| 格式             | 后缀       | 适用场景                                                  |
|------------------|------------|-----------------------------------------------------------|
| `json`（默认）   | `.json`    | TexturePacker JSON Hash —— 通用                           |
| `json-array`     | `.json`    | TexturePacker JSON Array（frames 为有序数组）              |
| `godot-tpsheet`  | `.tpsheet` | Godot 4 —— 需要 TexturePacker Godot 插件                  |
| `godot-tres`     | `.tres`    | Godot 4 —— 自动产出 `AtlasTexture` + `SpriteFrames`，**无需插件** |

```bash
mj_atlas pack ./sprites -o atlas --format godot-tres --trim --pot
```

[`sdk/godot/addons/mj_atlas/`](sdk/godot/addons/mj_atlas/) 下还有一个 GDScript loader，用来加载多边形网格 JSON 输出。

## 增量打包（`--incremental`）

mj_atlas 会把打包结果的元信息写进一个 sidecar 文件 `<output>.manifest.json`，里面记录了：完整布局、options 哈希、每个 sprite 的 SHA256 内容哈希、每个 atlas 内的最大空闲矩形集合。带 `--incremental` 重跑时，工具会 diff 输入目录与 manifest，选择最便宜的路径：

| 输入差异                              | 处理方式                                | 耗时       |
|---------------------------------------|-----------------------------------------|------------|
| 完全没变                              | **完全跳过** —— 不解码、不写盘            | ~10 ms     |
| 仅新增，且能塞进现有空闲矩形          | 局部重打 —— 在空闲区域绘制新 sprite       | 极低       |
| 像素被改但 trim 后尺寸不变            | 局部重打 —— 在原位覆盖像素                | 极低       |
| 删除                                  | 局部重打 —— 清空老矩形，回收空闲          | 极低       |
| 改尺寸 / 新 sprite 装不下 / 选项变化  | 全量重打                                 | 完整       |

最关键的不变量是 **UV 稳定性**：每一个未发生变化的 sprite，其 `(x, y, rotated)` 在新旧 atlas 中**完全一致**。已经发布的游戏客户端可以直接把新 atlas PNG 替换上去，旧代码继续用原来 bake 好的 UV 也能工作——只有 metadata sidecar 多出新 sprite 的条目。这让 mj_atlas 适合用在热更新 / 在线资产流水线里。

```bash
# 第一次构建，写入 manifest
mj_atlas pack ./sprites -o atlas --trim --pot --incremental

# 加一个 sprite 再跑——UV 稳定的局部重打
echo "新增 icon_added.png 到 ./sprites"
mj_atlas pack ./sprites -o atlas --trim --pot --incremental

# 强制重打（用来验证确定性 / 怀疑 manifest 损坏时）
mj_atlas pack ./sprites -o atlas --trim --pot --incremental --force
```

`--json` 输出会带上缓存命中状态，方便 CI 短路：

```json
{
  "status": "ok",
  "atlases": 1,
  "cached_atlases": 1,
  "skipped": true,
  "files": [{"image": "atlas.png", "from_cache": true, "...": "..."}]
}
```

manifest schema、失效规则、CI 接入示例见 [`docs/INCREMENTAL.md`](docs/INCREMENTAL.md)。

## 多边形网格

加上 `--polygon` 后，每个 sprite 会输出贴合不透明像素的三角网格。引擎用网格代替矩形渲染，对不规则 sprite 能减少 30%+ 透明 fragment overdraw。

```bash
mj_atlas pack ./sprites -o atlas --trim --pot \
    --polygon --polygon-shape auto --max-vertices 12
```

| 选项                                | 效果                                                              |
|-------------------------------------|-------------------------------------------------------------------|
| `--polygon`                         | 启用网格提取                                                      |
| `--tolerance 1.5`                   | Douglas-Peucker 简化容差（越小越贴合，顶点越多）                    |
| `--polygon-shape concave`（默认）   | 保留简化后的凹轮廓                                                |
| `--polygon-shape convex`            | 每个连通域取凸包（顶点更少）                                       |
| `--polygon-shape auto`              | 凹/凸包面积比 ≥ 0.85 时取凸包，否则保留凹                          |
| `--max-vertices N`                  | 顶点预算 —— 不满足就把容差 ×1.5 重试，最多 8 轮                    |

多连通域 sprite（比如一个 UI 徽章里 3 个不相连的图标）会被分别提轮廓 + 三角化，合并到同一份 `vertices` + `triangles` 输出里。具体范例见 [`docs/POLYGON.md`](docs/POLYGON.md)。

## Manifest 作为一等公民（v0.3）

带 `--incremental` 打包后，manifest sidecar (`<output>.manifest.json`) 就成了你 sprite 库的 content-addressed 视图。v0.3 新增四个直接对 manifest 操作的子命令——**全部不重打**：

| 子命令 | 作用 |
|---|---|
| `mj_atlas inspect <atlas_or_manifest>` | 漂亮打印 manifest：每个 atlas 的尺寸/占用率/空闲矩形数、tag 聚合、sprite 列表 |
| `mj_atlas diff <a> <b>` | 两个 manifest 之间的差异——added / removed / pixel-changed / resized / **moved**（UV 稳定性破坏）/ tag 变化 |
| `mj_atlas verify <atlas>` | 重新计算 atlas PNG 哈希（`--check-sources` 还会校验 sprite 源文件），不一致时退出码非 0 |
| `mj_atlas tag <atlas> <sprite> --add ui,icon --set-attribution "CC0"` | 读写 sprite 元数据：tags、attribution、source_url，跨重打保留 |

四个子命令都接受 manifest 本体、atlas PNG、sidecar metadata、或它们所在的目录路径——自动解析（包括多 bin 的 `_<N>` 后缀）。

Tags / attribution / source_url 存在每个 sprite 条目里，**不参与缓存 key**——改这些字段永远不会让增量缓存失效。

```bash
# 这个 atlas 里都有什么？
mj_atlas inspect ./out/atlas.png

# 两次构建之间布局变了吗？UV 稳定性还在吗？
mj_atlas diff ./build_a/atlas.manifest.json ./build_b/atlas.manifest.json

# 部署前 sanity check
mj_atlas verify ./out/atlas.png --check-sources

# 给 sprite 打标签 / 写来源信息（给下游工具用）
mj_atlas tag ./out/atlas.png walk_01.png --add walk,character --set-attribution "CC0 procedural"
mj_atlas tag ./out/atlas.png hero_idle.png --add hero,idle --set-source-url https://opengameart.org/...
```

每个子命令都支持 `--json` 输出，方便 CI 和 dashboard 消费。

## hfrog 制品镜像（可选）

如果你跑了一个 [hfrog](https://github.com/dingcode-icu/hfrog) 制品仓库，mj_atlas 在本地保存的同时还能把每次的项目 / 导出 atlas / 刷新后的 manifest 同步推一份到 hfrog。默认关闭，需要在 `~/.config/mj_atlas/config.toml` 里手动启用：

```toml
[hfrog]
enabled = true
endpoint = "https://hfrog.example.com"
token = ""                      # 不需要鉴权时留空
default_runtime = "asset-pack"
```

GUI 的 Settings 面板底部有 "hfrog Mirror" 区域可以直接编辑配置。**上传失败永不阻塞本地流水**——错误写到 `<atlas>.log` 供事后查看。线协议、命名规则、失败处理见 [`docs/HFROG.md`](docs/HFROG.md)。

## 运行日志 sidecar

每次调用（CLI 或 GUI）都会在 atlas / manifest 旁边写一份 `<output>.log`，覆盖上次的。文件头部记录完整 argv 和解析后的选项；正文捕获本次运行的全部 INFO/WARN/ERROR/DEBUG 行——包括平时不到 stdout 的 DEBUG 诊断信息。失败的运行反而会留下信息最完整的 sidecar；出问题时把 `./out/atlas.log` 直接发出来就行。

## 命令行参考

```
mj_atlas pack <INPUT_DIR> [OPTIONS]
mj_atlas inspect <ATLAS_OR_MANIFEST>
mj_atlas diff <A> <B>
mj_atlas verify <ATLAS_OR_MANIFEST> [--check-sources]
mj_atlas tag <ATLAS_OR_MANIFEST> [SPRITE] [--add ...] [--remove ...] [--clear]
                                          [--set-attribution ...] [--clear-attribution]
                                          [--set-source-url ...] [--clear-source-url]
                                          [--list]
mj_atlas formats               # 列出所有输出格式
mj_atlas gui                   # GUI（需 --features gui）
mj_atlas preview <ATLAS_FILE>  # 预览已打包的 atlas（需 --features gui）
```

完整选项见 `mj_atlas <subcommand> --help`。所有命令都接受 `--json` 输出机器可读结构（错误以 JSON 写到 stderr）。

LLM / Agent 集成场景下推荐读 [`llms_cn.txt`](llms_cn.txt)，是 token 友好的结构化版本。

## 横向对比

| 特性                              | mj_atlas | TexturePacker | free-tex-packer |
|-----------------------------------|:--------:|:-------------:|:---------------:|
| 开源                              |    ✓     |       ✗       |        ✓        |
| 单静态二进制                      |    ✓     |       ✗       |        ✗        |
| 多边形网格                        |    ✓     |       ✓       |        ~        |
| 多连通域网格                      |    ✓     |       ✗       |        ✗        |
| Godot 原生 `.tres` 输出           |    ✓     |       ~       |        ✗        |
| **增量打包 + UV 稳定**            |  **✓**   |       ✗       |        ✗        |
| Manifest sidecar（资源管理基础）  |    ✓     |       ✗       |        ✗        |
| 有损 PNG（调色板量化）            |    ✓     |       ✓       |        ~        |
| GUI                               |   opt-in |       ✓       |        ✓        |

## 许可

MIT。`--quantize` 引入的 `imagequant` 是 GPL-3.0；只要构建时启用了量化，二进制就受 GPL 约束。默认构建是干净的 MIT。

## 贡献

本 crate 隶属 [`r_lit`](https://github.com/...) Rust CLI 工具集，每个工具独立 Cargo crate、独立发布周期。欢迎 PR——请确保 `cargo test` 通过，且遵守现有模块布局（`src/pack/`、`src/output/`、错误类型在 `src/error.rs`）。
