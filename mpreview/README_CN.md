# mpreview

迷你交互式 3D 模型预览器。用游戏同款 Godot `gl_compatibility` 渲染器，手机框视口，动画切换，模型信息面板，一键抓屏；另有一份 headless Blender 抓屏脚本，共用同一机位预设（`cam.json`），实现 Blender-vs-Godot 1:1 对照。

## 和现有工具的分工

- `model-preview` 技能（gd4）：远端 Blender 批量阵列渲染，适合批量，不管交互，不管 Godot 侧。
- `mpreview`：单个模型交互预览，用游戏同款渲染器渲染，并从同一机位预设再出一张 Blender 图做对照。

## 直接跑源码（不用构建）

```bash
# 交互窗口
/Applications/Godot.app/Contents/MacOS/Godot --path ./mpreview -- \
  --model ./beach_character_runtime_r2.glb --anim Idle --rot-x 0

# 自动抓屏（要显示器或 xvfb；--headless 是哑驱动，截不到图）
Godot --path ./mpreview --quit-after 200 -- \
  --model ./beach.glb --anim Walk --shot godot_shot.png --shot-frames 90

# 同机位 Blender 图（同一 cam.json）
/Applications/Blender.app/Contents/MacOS/Blender --background --python blender_shot.py -- \
  ./beach.glb blender_shot.png cam.json --action Walk --size 405x720
```

## CLI 参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `--model` | — | `.glb` 路径（绝对路径或 `res://`），运行时 `GLTFDocument` 加载，免导入 |
| `--anim` | 首个 | 自动播放的动画 |
| `--rot-x` | 0 | 额外 X 旋转（度；此类 Quaternius 文件编辑器导入后要 90，运行时加载 0 即站立） |
| `--cam` | 内置 | `cam.json` 机位（Blender 坐标系 z-up；viewer 内转 Godot） |
| `--phone` | 405x720 | 手机框尺寸 |
| `--shot` / `--shot-frames` | — / 90 | 自动抓屏并退出（自动化用） |
| `--compare` | — | 把对照图叠在视口上（`C` 切换） |

按键：`Space` 播放/暂停，`S` 抓屏，`C` 对照，`R` 复位视角，`Q` 退出。左键拖拽环绕，滚轮缩放。

## 面板信息

文件名/大小，mesh 数+三角面数，骨骼数，材质数，动画数，模型身高（按骨骼跨度算，bind 塌缩的文件也准）。

## 二进制分发

`r_lit` 二进制走内网 Jenkins `toolMap` + R2 + `install.sh`。`mpreview` 不是 cargo，用 Godot 导出模板打：

```bash
godot --headless --path ./mpreview --export-release "macOS" ./dist/mpreview-mac.zip
godot --headless --path ./mpreview --export-release "Linux" ./dist/mpreview-linux.zip
```

Linux CI 抓屏要套 `xvfb-run`（抓屏要真 GL 上下文）。导出体积每平台约 40–70MB。

## 踩坑结论（保留）

- `cam.json` 统一 Blender 坐标，viewer 内转 `(x,y,z)->(x,z,-y)`。别手写 `Transform3D` 行，用 `-Z == fwd` 验算。
- 某些文件（如这个 Quaternius）编辑器导入的实例在 Godot 里躺着，要 `rotation.x = +90°`；同一文件运行时 `GLTFDocument` 加载则 0 即站立。按场景修，别烘进文件。
- GDScript 里属性叫 `rotation`，没有 `rotation_euler`。
