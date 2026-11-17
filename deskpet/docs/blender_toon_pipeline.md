# Blender → Toon 动漫黑边 GLB 预处理

deskpet 运行时默认 **Unlit + 烘焙 albedo**，描边来自 GLB 内单独子 mesh（零 shader 成本）。
请在 Blender 中完成风格化，导出后再用 Python 脚本压缩贴图。

## 流程概览

```
Meshy/fal GLB  →  Blender 烘焙 + Solidify 描边  →  glb_strip_pbr.py  →  glb_shrink_texture.py  →  assets/
```

## 1. 导入

1. File → Import → glTF 2.0，选择 `animation.glb` 或 fal 输出。
2. 选中 Armature，确认 idle 动画在 Action 列表中。

## 2. 材质 Toon 烘焙（替代运行时 PBR）

目标：单张 **Base Color** 贴图（128² 足够），无 normal / metallic / roughness。

1. 为角色新建 **Shader to RGB** 或 **Diffuse BSDF**（2–3 档色阶）：
   - 主色：皮肤/主体
   - 阴影色：约主色 × 0.55
   - 可选高光：极小范围 Toon ramp
2. **Bake → Diffuse Color**（仅 Color，不含 Direct/Indirect 时可关 lighting bake，用手调色阶）。
3. 将烘焙结果赋给 **Principled BSDF** 仅保留 **Base Color** 贴图：
   - Metallic = 0
   - Roughness = 1
4. 删除未使用的 normal / ORM 贴图槽。

> 若不想烘焙，也可在 GIMP/Krita 手绘 flat color + 1px 内描边到 albedo。

## 3. 动漫黑边（Outline mesh）

1. 复制角色 mesh（或 Duplicate Linked），命名 **`Body_outline`**（名称含 `outline` 即可被 deskpet 识别）。
2. 添加 **Solidify** modifier：
   - Thickness：`-0.003` ~ `-0.008`（视模型尺度微调）
   - Flip Normals：按需
3. Outline 材质：**纯黑** `(0.015, 0.015, 0.025)`，无贴图。
4. 将 outline mesh **Parent 到 Armature**（Copy Transforms / Armature deform），保证动画同步。

deskpet 会对 `*outline*` / `*ink*` / `*_cel` 命名的 mesh 强制黑色 Unlit。

## 4. 导出

1. File → Export → glTF 2.0 (.glb)
2. 勾选 **Selected Objects**（含 armature + body + outline）
3. Format: **GLB Binary**
4. 贴图格式：**PNG**（deskpet 未启用 jpeg feature）
5. 导出为 `mascot.glb`；行走动画可单独导出 `mascot_walk.glb`（仅 Animation 0）

## 5. 后处理（命令行）

```bash
cd deskpet

# 推荐：参考立绘色板 + 去烘焙光照（最接近 NPR）
python3 tools/glb_toon_texture.py assets/mascot.glb --reference path/to/ref.png

# 剥离 PBR 贴图引用
python3 tools/glb_strip_pbr.py assets/mascot.glb

# 可选：简单色阶量化（旧方案）
python3 tools/glb_posterize_texture.py assets/mascot.glb --steps 5

# 降采样内嵌贴图（桌面宠物 128² 足够）
python3 tools/glb_shrink_texture.py assets/mascot.glb --size 128
```

## 6. 安装到 starlink-dev serve

```bash
just install_loc   # 或 starlink-dev 的 deskpet assets 同步
starlink-serve restart --show
```

## 资源预算参考

| 项 | 建议 |
|----|------|
| Base color 分辨率 | 128²（64² 亦可） |
| 贴图数量 | 1（仅 albedo） |
| 动态光 | Toon 模式下关闭（Unlit） |
| 活跃帧率 | 30 fps（代码默认） |
| MSAA | Off |

## 故障排查

- **模型全黑**：检查 albedo 是否烘焙成功；outline mesh 是否过厚遮住主体。
- **无描边**：确认子 mesh 名称含 `outline`。
- **贴图不显示**：运行 `glb_strip_pbr.py` 后确认 `mimeType` 为 `image/png`。
