# textexture

Generate stylized text images with visual effects — shadow, outline, gradient, glow, neon and more.

## Quick Start

```bash
# Basic white text on black
textexture render "Hello World" -o hello.png

# Transparent background
textexture render "LOGO" -s 120 --transparent -o logo.png

# Neon effect on dark background
textexture render "CYBER" -s 120 --bg "#0a0a2e" \
  -e "neon:color=#00ffff,radius=20" -o neon.png

# Gradient background + text effects
textexture render "SALE" -s 150 --bg "#ff6b6b,#ffd93d" \
  -e "outline:color=#ffffff,width=2" -o sale.png

# Rainbow gradient background at 45°
textexture render "RAINBOW" --bg "#ff0000,#00ff00,#0000ff@45" -o rainbow.png

# Image background
textexture render "HERO" -s 100 --bg ./photo.jpg \
  -e "neon:color=#ffffff,radius=15" -o hero.png

# Fit text within 300px width (font auto-shrinks)
textexture render "LONG TEXT HERE" -W 300 --transparent -o fitted.png
```

## Smart Sizing

- **Auto-fit**: when `-W` is set, font size automatically shrinks to fit (never clips).
- **No width set**: canvas grows to fit text naturally.
- **Max cap**: canvas capped at **1920px** width. Text exceeding this triggers auto-shrink.
- **Min font**: auto-fit floors at 8px to keep text legible.

## Background (`--bg`)

One parameter, multiple modes — auto-detected:

| Value | Result |
|-------|--------|
| `"#ff0000"` | Solid color |
| `"#ff0000,#0000ff"` | 2-color gradient |
| `"#ff0000,#00ff00,#0000ff@45"` | Multi-color gradient at 45° |
| `./photo.jpg` | Image (stretched to fit) |
| `--transparent` | Transparent (overrides `--bg`) |

## Effects

Effects are applied with `-e "name:param=val,param=val"`. Multiple effects can be stacked.

| Effect | Params | Default | Description |
|--------|--------|---------|-------------|
| `shadow` | `color`, `ox`, `oy`, `blur` | `#00000080`, 4, 4, 8 | Drop shadow |
| `outline` | `color`, `width` | `#ffffff`, 2 | Text stroke/outline |
| `gradient` | `start`, `end`, `angle` | `#ff0000`, `#0000ff`, 0 | Gradient text fill |
| `glow` | `color`, `radius` | `#00ffff`, 15 | Outer glow (Screen blend) |
| `neon` | `color`, `radius` | `#ff00ff`, 20 | Neon (3-layer: outer + inner + core) |

### Effect Pipeline

Execution order by phase (not CLI order):
1. **Pre** (behind text): `shadow`
2. **Fill** (replaces text color): `gradient`
3. **Post** (on top): `outline`, `glow`, `neon`

## Options

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--output` | `-o` | `textexture_output.png` | Output file path |
| `--font` | `-f` | system sans-serif | Font family or `.ttf`/`.otf` path |
| `--font-size` | `-s` | `72` | Font size in px (auto-shrinks to fit) |
| `--color` | `-c` | `#ffffff` | Text color (CSS format) |
| `--bg` | | `#000000` | Background: color / gradient / image path |
| `--transparent` | | | Transparent background |
| `--width` | `-W` | auto | Image width (max 1920, font auto-shrinks) |
| `--height` | `-H` | auto | Image height (max 1920) |
| `--padding` | | `40` | Padding around text (px) |
| `--effect` | `-e` | | Effect spec (repeatable) |
| `--json` | | | JSON output for scripting |

## Subcommands

```bash
textexture render <TEXT> [OPTIONS]    # Render text to image
textexture list-effects              # Show available effects
textexture list-fonts [--search Q]   # List/search system fonts
```

## Colors

CSS color syntax: `#rgb`, `#rrggbb`, `#rrggbbaa`, named (`red`, `cyan`), `rgb()`, `rgba()`, `hsl()`.

## Build

```bash
cargo build --release
```
