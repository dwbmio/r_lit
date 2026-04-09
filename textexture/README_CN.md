# textexture

生成带视觉效果的艺术字图片 — 阴影、描边、渐变、发光、霓虹等。

## 快速上手

```bash
# 基础白字黑底
textexture render "你好世界" -o hello.png

# 透明背景
textexture render "标志" -s 120 --transparent -o logo.png

# 霓虹效果
textexture render "赛博" -s 120 --bg "#0a0a2e" \
  -e "neon:color=#00ffff,radius=20" -o neon.png

# 渐变背景 + 描边
textexture render "促销" -s 150 --bg "#ff6b6b,#ffd93d" \
  -e "outline:color=#ffffff,width=2" -o sale.png

# 三色渐变背景 45°
textexture render "彩虹" --bg "#ff0000,#00ff00,#0000ff@45" -o rainbow.png

# 图片背景
textexture render "英雄" -s 100 --bg ./photo.jpg \
  -e "neon:color=#ffffff,radius=15" -o hero.png

# 限定宽度（字号自动缩小）
textexture render "很长的一段文字" -W 300 --transparent -o fitted.png
```

## 智能尺寸

- **自适应**：指定 `-W` 时字号自动缩小，文字不截断。
- **未指定宽度**：画布跟随文字自动增长。
- **上限**：画布宽度上限 **1920px**，超出自动缩字号。
- **下限**：字号最小 8px，保持可读。

## 背景 (`--bg`)

一个参数，自动识别：

| 值 | 效果 |
|----|------|
| `"#ff0000"` | 纯色 |
| `"#ff0000,#0000ff"` | 双色渐变 |
| `"#ff0000,#00ff00,#0000ff@45"` | 多色渐变 + 角度 |
| `./photo.jpg` | 图片（拉伸填充） |
| `--transparent` | 透明（覆盖 `--bg`） |

## 效果

通过 `-e "名称:参数=值,参数=值"` 指定，可叠加多个。

| 效果 | 参数 | 默认值 | 说明 |
|------|------|--------|------|
| `shadow` | `color`, `ox`, `oy`, `blur` | `#00000080`, 4, 4, 8 | 阴影 |
| `outline` | `color`, `width` | `#ffffff`, 2 | 描边 |
| `gradient` | `start`, `end`, `angle` | `#ff0000`, `#0000ff`, 0 | 渐变填充 |
| `glow` | `color`, `radius` | `#00ffff`, 15 | 外发光 |
| `neon` | `color`, `radius` | `#ff00ff`, 20 | 霓虹（三层） |

### 效果管线

按阶段执行，与 CLI 顺序无关：
1. **Pre**（文字后方）：`shadow`
2. **Fill**（替代文字颜色）：`gradient`
3. **Post**（文字前方）：`outline`、`glow`、`neon`

## 参数

| 参数 | 简写 | 默认值 | 说明 |
|------|------|--------|------|
| `--output` | `-o` | `textexture_output.png` | 输出路径 |
| `--font` | `-f` | 系统无衬线 | 字体名或 `.ttf`/`.otf` 路径 |
| `--font-size` | `-s` | `72` | 字号（px），超宽自动缩小 |
| `--color` | `-c` | `#ffffff` | 文字颜色 |
| `--bg` | | `#000000` | 背景：颜色/渐变/图片路径 |
| `--transparent` | | | 透明背景 |
| `--width` | `-W` | 自动 | 宽度（上限 1920，字号自适应） |
| `--height` | `-H` | 自动 | 高度（上限 1920） |
| `--padding` | | `40` | 内边距（px） |
| `--effect` | `-e` | | 效果规格（可重复） |
| `--json` | | | JSON 输出 |

## 子命令

```bash
textexture render <文字> [选项]       # 渲染文字为图片
textexture list-effects              # 列出可用效果
textexture list-fonts [--search Q]   # 列出/搜索字体
```

## 颜色

CSS 语法：`#rgb`、`#rrggbb`、`#rrggbbaa`、颜色名、`rgb()`、`rgba()`、`hsl()`。

## 构建

```bash
cargo build --release
```
