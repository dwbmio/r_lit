# Maquette App Icon

## 拍板结果（2026-04-23）

**方向：#3 迷你小房子（简化版）** · `final-direction-tinyhouse.png`

3 个红色方块底 + 1 个蓝色方块做屋顶，toon 描边 + 三段面着色。
相比 `proposals/icon_03_tiny_house.png` 去掉了黄色点缀方块，
轮廓更干净、32px 下更可读。

下一步（归属 v1.0 图标任务，见 `USER-TODO.md #26`）：
- 生成 1024 / 512 / 256 / 128 / 64 / 32 / 16 全尺寸 PNG
- 打包 `.icns`（macOS）/ `.ico`（Windows）
- 接到 `Cargo.toml` bundle 配置

## 历史候选方案

四版提案都在 `proposals/` 下。先看大图挑方向，小图 32×32 也能读的设计才算数。

每一版的设计语言与参考对标产品：

| 文件 | 风格关键词 | 对标参考 | 适合的理由 |
|---|---|---|---|
| `proposals/icon_01_stacked_blocks.png` | 三色叠方块 / toon 描边 / 阶梯感 | Asset Forge, Kenney Assets | **最契合产品本体** —— 画面=用 Maquette 实际会做的东西。风险：阶梯朝向不够"iconic"。 |
| `proposals/icon_02_single_cube.png` | 单方块 / 粗描边 / 暖色三段 | Minecraft launcher, Blocks app | **最简**，远距离最容易辨识，但"撞型"风险最高（voxel 类工具图标普遍长这样）。 |
| `proposals/icon_03_tiny_house.png` | 迷你房子 / 多色混搭 / 玩具感 | Kenney 示意图, Asset Forge 封面 | 表达"拼出什么"最直接，记忆点强。风险：细节多，32px 下易糊。 |
| `proposals/icon_04_rubiks_voxel.png` | 2×2×2 糖果色方阵 / 深色底 | Figma, Raycast, Linear | **最像现代 IDE / 工具类 App**，深底在 Dock / taskbar 上对比度好。风险：七彩感偏消费，专业感稍弱。 |

## 推荐顺序（我的判断）

1. **#2（单方块）** — 作为首选。原因：看了 Asset Forge / MagicaVoxel / Kenney Shape 的最终 App 图标你会发现它们都倾向极简单形——一个核心几何 + 强识别色。"简单大方"的用户诉求正好对应。
2. **#4（糖果方阵）** — 更有现代 tool 气质，深底对色板强过浅底。
3. **#1（阶梯叠方块）** — 最具产品语义，但结构复杂度中等，需要在 32px 试配色。
4. **#3（迷你房子）** — 场景化太强，更适合 marketing 图而非 App 图标。

## 你要做的只有一件事

在 `USER-TODO.md #26` 打钩前，挑一个编号（或者告诉我"要 #2 但外框换深蓝"之类调整），我就接着：

- 按你的选择重新生成高保真终版；
- 把 1024 / 512 / 256 / 128 / 64 / 32 / 16 的 PNG 导出全套；
- 打包成 `.icns`（macOS）/ `.ico`（Windows）；
- 接到 `Cargo.toml` 的 bundle 配置里。

## 为什么不自动选

图标审美是强个人决策，且一旦定下来就会跟着项目到 v1.0+，作为 Agent 我不给你拍板。
