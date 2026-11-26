# archcast

**AI-first deterministic architecture & flow report compiler (SIMD + Rayon + WebGPU).**

`archcast` separates the **AI-friendly single source of truth** (compact YAML/JSON) from **human-facing visual deliverables** (fixed-style HTML with built-in WebGPU/Canvas2D motion, standalone root-level `.svg` diagrams, optional raster `.png` and animated flow-pulse `.gif`) and **engineering wiki documentation** (`Markdown + Mermaid`). It also reverse-extracts existing `HTML + SVG` overview pages back into canonical YAML and Markdown.

## Features

- **1:1 Fixed Visual Style & Structured Themes (`render`, `theme`)**: Compiles YAML/JSON specs into deterministic 4-column swimlane reports (before/after dual diagrams, action subcards, auto-sized pill badges, orthogonal bus routing, payload/gap callouts, warning banners, grouped API/callback tables). Ships with 5 bundled themes (`starlink-light` default, `obsidian-neon`, `paper-editorial`, `blueprint-ink`, `catppuccin-mocha`) plus external YAML/JSON theme CRUD (`archcast theme list|show|create|update|delete|export`) with `extends` inheritance, synced across HTML, SVG, PNG, GIF, and Mermaid `%%{init:...}%%`.
- **Root-Level Standalone SVG + Rich HTML + Markdown**: Default `--format all` writes `<stem>.html`, standalone `<stem>-<diagram_id>.svg` (or `<stem>.svg` for single-diagram specs), and `<stem>.md` directly into `--output-dir`.
- **HTML-First Browser WebGPU Motion Layer**: Generated HTML embeds a zero-dependency WebGPU WGSL instanced particle overlay scoped to each diagram's minimal `dynamic_bounds` (automatically falling back to 60fps Canvas2D) for animated data-flow pulses, click-to-trace upstream/downstream card highlighting, and one-click SVG download. Pass `--static-html` for pure zero-JS HTML.
- **Raster PNG & Layered Animated Flow-Pulse GIF (`--png`, `--gif`, `--format media`)**:
  - Pure-Rust SIMD SVG rasterization (`resvg` + `tiny-skia`) with system CJK font loading.
  - Layered minimal-bounds animated GIF export (`--gif-mode overlay` transparent cropped dynamic overlay by default, or `--gif-mode delta` static base + dirty-rect sub-frames).
  - Default build runs frame compositing in parallel on CPU via Rayon (100% headless CI safe); optional `--features gpu` executes frame glow compositing on a native `wgpu` WGSL compute shader (Metal / Vulkan / DX12).
- **Reverse Extraction (`extract`)**: Parses existing `HTML + SVG` overviews via SIMD scanning (`memchr`), zero-copy XML DOM (`roxmltree`), and 4-wide SIMD spatial containment (`wide::f64x4`) into clean YAML + Markdown.
- **Agent Skill**: See `SKILL.md` for end-to-end AI agent authoring workflows.
- **`rlit_cli` Machine Call Contract v1**: Global `--json` envelopes, deterministic exit codes (`0-5`), SHA-256 `artifacts` receipts, hidden `__schema` export, and `<output_dir>/archcast.log` sidecar logs.

## Build

```bash
cd archcast
cargo build --release
cargo test

# Enable native WebGPU WGSL compute shader backend for animated GIF compositing
cargo build --release --features gpu
```

## Quick Start

```bash
# 1. Generate a starter YAML spec
archcast init starter.yaml --title "Release Pipeline Overview"

# 2. Validate topology and schema
archcast validate starter.yaml --json

# 3. Compile YAML -> HTML (WebGPU/Canvas2D motion) + standalone root SVG(s) + Markdown
archcast render starter.yaml -o out/ --strict --json

# 4. Manage structured external themes (stored in ~/.config/archcast/themes or ARCHCAST_THEMES_DIR)
archcast theme list --json
archcast theme create corp-blue --from starlink-light \
  --set surface.page_bg=#f0f4ff --set chrome.h2_accent=#1e40af --json
archcast theme update corp-blue --set tones.emerald=#047857 --json
archcast render starter.yaml -o out/ --theme corp-blue --strict --json
archcast theme export corp-blue corp-blue.yaml --json
archcast theme delete corp-blue --json

# 5. Also export static PNG + layered animated flow-pulse GIF
archcast render starter.yaml -o out/ --format media --strict --json

# 6. Reverse-extract an existing HTML+SVG overview into YAML + Markdown
archcast extract legacy-overview.html -o extracted/ --json

# 7. Inspect runtime SIMD / Rayon / WebGPU capabilities
archcast info --json
```

## Theme System: Selection, Extension, Updates & Creation

`archcast` models visual styling as a structured `ThemeConfig` (YAML or JSON). **A single theme configuration drives every export target in sync**: HTML CSS, standalone SVG, raster PNG, layered animated GIF (flow pulses and status beacons), and Mermaid `%%{init:...}%%` variables in Markdown.

### 1. Five Bundled Default Themes

| Theme ID | Scheme | Style & Recommended Use |
| :--- | :--- | :--- |
| `starlink-light` (**default**) | `light` | Crisp cool-gray light report theme, 1:1 canonical review layout |
| `obsidian-neon` | `dark` | Deep dark neon aesthetic for large displays and dark-mode reviews |
| `paper-editorial` | `light` | Warm editorial paper palette for printing, PDF export, and long reads |
| `blueprint-ink` | `dark` | High-contrast engineering blueprint theme for architecture reviews |
| `catppuccin-mocha` | `dark` | Soft Catppuccin Mocha dark developer palette |

### 2. Selecting a Theme When Rendering (Resolution Priority)

1. **CLI flag (highest priority)**: Pass either a theme ID or a direct `.yaml`/`.yml`/`.json` file path:
   ```bash
   # Use a bundled or registered external theme ID
   archcast render flow.yaml -o render/ --theme obsidian-neon --strict --json

   # Render directly with a standalone theme file in your repo (no registration needed)
   archcast render flow.yaml -o render/ --theme ./themes/my-brand.yaml --strict --json

   # Point to a custom external themes directory (or set ARCHCAST_THEMES_DIR)
   archcast render flow.yaml -o render/ --theme corp-blue --themes-dir ./team-themes --json
   ```
2. **Pin inside the architecture YAML spec**: Add top-level `theme: blueprint-ink` (or a file path) in the spec; used automatically when `--theme` is omitted.
3. **Default fallback**: Uses `starlink-light` when neither is specified.

**Lookup order**: Direct existing file path -> external themes dir (`--themes-dir` > `ARCHCAST_THEMES_DIR` > `$XDG_CONFIG_HOME/archcast/themes` > `~/.config/archcast/themes`) -> bundled built-in theme.

### 3. Creating & Extending Custom Themes (3 Workflows)

Theme ID rules: `1..=64` chars, starts with ASCII alphanumeric, only `[a-zA-Z0-9_-]`.

- **Workflow A: Derive from any base theme with `--set` overrides**
  ```bash
  archcast theme create corp-blue \
    --from starlink-light \
    --display-name "Corp Blue" \
    --set surface.page_bg=#f0f4ff \
    --set chrome.h2_accent=#1e40af \
    --set tones.emerald=#047857 \
    --set motion.pulse_hex=#1d4ed8,#047857,#d97706,#7c3aed
  ```
- **Workflow B: Export a full template, edit, and register (or use directly)**
  ```bash
  archcast theme export starlink-light ./corp-blue.yaml
  # Edit ./corp-blue.yaml, then register (pass --force to overwrite an existing ID)
  archcast theme create corp-blue --file ./corp-blue.yaml
  ```
- **Workflow C: Minimal self-extension via `extends` (partial YAML override)**
  Specify only the tokens you want to change; everything else is deep-merged from the parent theme (chains up to depth 4):
  ```yaml
  id: team-dark
  display_name: Team Dark
  extends: obsidian-neon
  surface:
    page_bg: "#0b1020"
    svg_bg: "#0f172a"
  chrome:
    h2_accent: "#38bdf8"
    edge_stroke: "#64748b"
  tones:
    emerald: "#10b981"
  motion:
    pulse_hex: ["#38bdf8", "#10b981", "#f59e0b", "#a78bfa"]
  ```
  Use directly via `archcast render flow.yaml -o render/ --theme ./team-dark.yaml` or register with `archcast theme create team-dark --file ./team-dark.yaml`.

### 4. Inspecting, Updating, Overriding Defaults & Deleting

```bash
# List all bundled and external themes (flags overrides_builtin when overriding a built-in)
archcast theme list
archcast theme list --json

# Print full resolved theme (YAML on stdout by default; structured JSON with --json)
archcast theme show corp-blue
archcast theme show corp-blue --json

# Update an external theme (repeatable --set, --display-name, or --file patch)
archcast theme update corp-blue --set surface.card_bg=#ffffff --set chrome.edge_stroke=#334155

# Override a bundled default theme (even default renders without --theme pick up the override)
archcast theme update starlink-light --set chrome.h2_accent=#0f766e

# Delete an external theme (deleting an override restores the bundled default; pure built-ins are protected)
archcast theme delete corp-blue
archcast theme delete starlink-light   # removes external override and restores bundled starlink-light
```

### 5. Complete Token Reference (`--set <dot.path>=<value>` & YAML Keys)

| Section | Configurable Keys | Purpose |
| :--- | :--- | :--- |
| Top-level | `display_name`, `color_scheme` (`light`\|`dark`), `is_dark` (`true`\|`false`), `extends` (YAML) | Theme label, color scheme mode, and parent theme inheritance |
| `surface.*` | `page_bg`, `card_bg`, `card_border`, `card_shadow`, `svg_bg`, `node_bg`, `node_border`, `subcard_bg`, `subcard_border`, `callout_bg`, `callout_border`, `code_bg`, `codebox_bg`, `codebox_border` | Page, card, SVG canvas, node, subcard, callout, and code block surfaces (`svg_bg` must be valid `#RRGGBB`) |
| `ink.*` | `page_fg`, `lead_fg`, `note_fg`, `code_fg`, `svg_title`, `svg_subtitle`, `svg_meta`, `col_header`, `node_title`, `node_text`, `node_mono`, `node_footer`, `subcard_title` | Page prose, notes, diagram headings, column headers, and node/subcard typography colors |
| `chrome.*` | `toc_link`, `h2_accent`, `btn_bg`, `btn_fg`, `btn_border`, `btn_hover_bg`, `pill_bg`, `pill_fg`, `edge_spine`, `edge_stroke`, `edge_label_bg`, `edge_label_fg`, `badge_opacity`, `ver_bg_alpha` | TOC links, section accents, toolbar buttons, pill tags, routed bus/edge strokes, and edge labels |
| `table.*` | `head_bg`, `head_fg`, `border`, `row_border`, `hover_bg`, `grp_bg`, `grp_fg`, `cnt_fg`, `small_fg`, `down_fg` | Flow table headers, grid borders, row hover, group banners, and downstream column text |
| `warning.*` | `hint_bg`, `hint_fg`, `hint_accent`, `banner_bg`, `banner_border`, `banner_accent`, `banner_title`, `banner_body` | Footer hint callout and full-width warning banner palette |
| `tones.*` | `default`, `muted`, `blue`, `orange`, `green`, `emerald`, `amber`, `purple`, `gold`, `red` | 10 semantic tones driving card top bars, badges, colored lines, and table groups |
| `motion.*` | `pulse_hex` (4 comma-separated `#RRGGBB`), `beacon_red`, `beacon_green` | 4-channel flow particle colors and status beacon colors for HTML WebGPU/Canvas2D and GIF |

## Machine Call Contract

- `--json` outputs exactly one `rlit_cli` envelope on stdout; human logs go to stderr and `<output_dir>/archcast.log`.
- `archcast __schema` prints the machine-readable CLI JSON Schema.
- Exit codes: `0` ok, `1` runtime, `2` usage, `3` input/validation, `5` partial batch failure.
