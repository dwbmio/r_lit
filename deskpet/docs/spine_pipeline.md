# deskpet → Spine 2D 管线

参照 [spine2d-animation-mcp](https://github.com/ampersante/spine2d-animation-mcp)（已 clone 到 `tools/spine2d-animation-mcp`）。

## 必要拆图么？

**必须拆。** Spine 是 2D 骨骼动画，不是整张图做 mesh 变形。

| 形式 | 是否可用 |
|------|----------|
| 单张 PNG / 预览立绘 | ❌ 不能直接 rig |
| **PSD 分层**（每层一个部位，透明底） | ✅ MCP 唯一输入 |
| 拆成多张 PNG 再合成 PSD | ✅ 可以，最终必须是 PSD |

不拆图只能做「整图上下浮动」类特效，做不了思考（手托腮）、信件送达（举手）、询问（歪头）等动作。

---

## MCP 现状（重要）

| 能力 | 状态 |
|------|------|
| PSD → Spine 4.2 骨骼 + atlas + png | ✅ Phase 0 已实现 |
| 自动生成 6 个动作 | ❌ **尚未实现**（README 写明 animation 在后续 phase） |
| 导入 Spine Editor / Unity | ✅ 生成物可直接 import |

因此当前流程是：

1. **美术拆 PSD**（按下方图层清单）
2. **`import_psd_to_spine`** 生成 rig
3. **Spine Editor 里 K 帧** 6 个 animation（或等 MCP 后续版本）

---

## 推荐 PSD 图层（对应预览图萌宠）

### MCP 自动识别的标准层（必填，英文名）

MCP 只认 [conventions.py](../tools/spine2d-animation-mcp/src/spine2d_mcp/psd/conventions.py) 里的人类骨骼命名：

| PSD 图层名 | 萌宠对应 | 说明 |
|------------|----------|------|
| `head` | 红宝石兜帽 + 脸（可先合并） | 枢轴在底部（脖子） |
| `torso` 或 `body` | 米白内衬 + 胸针区域 | 躯干 |
| `arm_left` | 左袖（上臂段） | 挂于 chest |
| `arm_right` | 右袖（上臂段） | 挥手/持信 |
| `hand_left` | 左手套 | 挂于 arm_left |
| `hand_right` | 右手套 | 挂于 arm_right |
| `leg_left` | 左宝石靴 | 挂于 hip |
| `leg_right` | 右宝石靴 | 挂于 hip |

可选 pivot 微调层：`arm_left[pivot]` 等（1×1 像素标记关节位置）。

### 建议在 Spine Editor 里追加的 slot（MCP 不会自动建）

拆 PSD 时尽量单独分层，import 后手动绑到新 bone：

| 图层名（PSD） | 用途 |
|---------------|------|
| `face` | 脸白底 |
| `eye_left` / `eye_right` | 眨眼、思考/询问眼神 |
| `mouth` | 开心/沮丧嘴型切换 |
| `constellation` | 兜帽星座（可 shimmer） |
| `star` | 漂浮四角星 |
| `cape_back` | 斗篷后摆（待机飘动） |
| `cape_front` | 斗篷前片 |
| `brooch` | 胸针宝石 |
| `letter` | 信件（默认隐藏，送达动画显示） |

> 若 PSD 里用了 MCP 不认识的层名，该层会被忽略；import 后从 atlas 里拖进对应 slot 即可。

---

## 六个动作设计要点

| 动画名（Spine） | 中文 | 关键骨骼/附件 |
|-----------------|------|----------------|
| `idle` | 待机 | root 轻微 Y 起伏；cape 微摆；随机 blink |
| `think` | 思考 | head 右倾；eye 上看；hand_right 贴 chin |
| `sad` | 沮丧 | head 低垂；eye 半闭；cape 下垂；star 降低 |
| `letter` | 信件送达 | hand 举 letter 附件；happy 嘴；star 跳一下 |
| `happy` | 开心 | 整体小跳；mouth 大笑；star 旋转 |
| `ask` | 询问 | head 侧倾；hand 微抬；eye 一边大一边小 |

---

## 命令行导入（无需 MCP 客户端）

```bash
cd deskpet/tools/spine2d-animation-mcp
uv sync

# 生成 rig 到 assets/spine/out/
uv run python -c "
from spine2d_mcp.pipeline import import_psd
import json
r = import_psd_to_spine('/path/to/mascot.psd', 'assets/spine/out')
print(json.dumps(r, indent=2))
"
```

或使用 MCP（见 `tools/spine2d-mcp.cursor.json` 配置片段）。

输出：

- `*.json` — Spine 4.2 skeleton
- `*.atlas` — 纹理 atlas
- `*.png` — atlas 图集

用 [Spine Editor](http://esotericsoftware.com/) 打开 json，K 上述 6 个 animation，导出 runtime（json + atlas + png）。

---

## deskpet 集成（后续）

Bevy 侧可选：

- [`bevy_spine`](https://github.com/occultist-of-ua/bevy_spine) 或自研 sprite 序列
- 替换当前 GLB 3D 管线，RSS/VRAM 通常更低

当前仓库 **尚未** 接 Spine runtime；先完成 PSD + rig + 动画再改 `app.rs`。

---

## 拆图：手工 PSD（推荐，无 AI API）

不用 Google/Gemini，从立绘按**手工区域**切层，生成 Spine 可导入的 PSD。

### 1. 调整区域（可选）

编辑 `assets/spine/mascot_parts.json`：每层 `rect` / `ellipse` + `pivot_markers`。  
预览切分框：`assets/spine/mascot_layers_debug.png`

### 2. 生成 PSD + 部件 PNG

```bash
cd deskpet
pip install psd-tools Pillow numpy
python3 tools/build_mascot_psd.py
```

输出：

| 文件 | 说明 |
|------|------|
| `assets/spine/mascot.psd` | 分层 PSD（含 pivot 标记层） |
| `assets/spine/parts/*.png` | 各部件透明 PNG |
| `assets/spine/mascot_layers_debug.png` | 区域调试叠加图 |

当前层（bottom→top）：`leg_left/right`, `torso`, `arm_left/right`, `hand_left/right`, `head`, `star`

### 3. 导入 Spine rig

```bash
tools/spine_import.sh assets/spine/mascot.psd
# → assets/spine/out/mascot.json + mascot.atlas + mascot.png
```

用 Spine Editor 打开 `mascot.json`，K 帧 6 个 animation。

> 若要在 Photoshop 里精修：打开 `mascot.psd` 直接改图层像素，保存后再 `spine_import.sh`。

---

## 拆图：AI（可选，需 GEMINI_API_KEY）

见 `tools/spine_split_starlink.py`（Genielabs Gemini 流程，易产生像素异常，已不推荐）。

---

## 拆图工具建议（旧节保留）

1. **Photoshop / Affinity / Procreate（导出 PSD）** — 推荐，边缘最干净  
2. 参考图：`assets/reference/mascot_ref.png`（从预览图复制）  
3. 导出前：透明底、合并多余效果层、每层一个部位  
4. 命名严格用小写 + 下划线（`arm_left` 不是 `Arm Left`）

---

## 与 Blender 对比

| | Blender 3D | Spine 2D |
|--|------------|----------|
| 风格匹配立绘 | 难（Meshy 贴图差） | ✅ 直接画/拆 2D |
| 资源占用 | 高（Bevy 3D） | 低（sprite + atlas） |
| 拆图 | 不需要 | **必须** |
| 当前自动化 | glb_toon_texture | MCP 仅 rig，动画手 K |
