# tex_packer GUI Design Document

## Overview

tex_packer GUI is a native desktop application built with **egui + eframe (wgpu)**, providing a complete texture atlas workflow: project management, sprite import, packing, and inline preview.

## Architecture

```
┌─ TexPackerApp ─────────────────────────────────────────────┐
│                                                             │
│  AppMode::Welcome   — Landing page, recent files            │
│  AppMode::Packer    — Project workspace (default on launch) │
│  AppMode::Viewer    — Standalone atlas preview              │
│                                                             │
│  Overlay: Toast notifications, Modal dialogs                │
│  Global: Menu bar, theme, fonts                             │
└─────────────────────────────────────────────────────────────┘
```

## Screens

### 1. Welcome Screen
- App logo + version
- "New Project" / "Open Project..." / "Open Atlas Preview..." buttons
- Recent files list (clickable, auto-detects .tpproj vs .json/.tpsheet)

### 2. Packer Workspace (Default)
Split into 3 areas:

```
┌─ File | View | Help ─────────── tex_packer v0.1.0 | MyProject* | 16 sprites ─┐
│                                                                                │
│  ┌─ Sprite List (35%) ──┐ ┌─ Preview Canvas (65%) ────────────────────┐       │
│  │ Sprites (16)  [+Add] │ │ 512x256  Zoom:[====] Grid Names Fit      │       │
│  │ ─────────────────── │ │ ┌──────────────────────────────────────┐  │  ┌──┐ │
│  │ ● walk_01.png     x │ │ │ ▓▓▓▓  ▓▓  ▓▓▓▓▓  ▓▓▓              │  │  │S │ │
│  │ ● walk_02.png     x │ │ │ ▓▓    ▓▓▓▓  ▓▓▓  ▓▓▓              │  │  │e │ │
│  │ ● idle_01.png     x │ │ │ ▓▓▓▓▓▓  ▓▓  ▓▓                    │  │  │t │ │
│  │ ● icon_star.png   x │ │ └──────────────────────────────────────┘  │  │t │ │
│  │                      │ │                                           │  │i │ │
│  │ Drop files here...   │ │                                           │  │n │ │
│  └──────────────────────┘ └───────────────────────────────────────────┘  │g │ │
│                                                                          │s │ │
│                                                                          └──┘ │
├────────────────────────────────────────────────────────────────────────────────┤
│ Done! 1 atlas(es), 16 sprites -> /output/dir                                  │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Right settings panel:**
- Output: name, directory
- Packing: max size, spacing, padding, extrude, trim, rotate, POT, polygon, quantize
- Format: JSON Hash / JSON Array / Godot tpsheet / Godot tres
- Preview: Split direction (Left|Right / Top|Bottom), Auto-pack toggle
- Pack! button + Open Preview button

**Central area:**
- Empty: Large drop zone with "Add Sprites" / "Add Folder" buttons
- With sprites, no preview: Full sprite list
- With sprites + preview: Split view (35/65 ratio)
  - Left/Top: Sprite list with search, add, remove, file-exists indicators
  - Right/Bottom: Interactive atlas preview (checkerboard, zoom, pan, grid, names, hover)

**Split direction options:**
- `SplitDir::Horizontal` — sprites left, preview right (default)
- `SplitDir::Vertical` — sprites top, preview bottom

### 3. Viewer Screen
Standalone atlas preview for opening .json/.tpsheet files directly.
- Left panel: sprite list with search, animations
- Central canvas: full atlas preview
- Bottom: zoom, grid, names controls

## Project File (.tpproj)

```json
{
  "version": 1,
  "output_name": "atlas",
  "output_dir": "/path/to/output",
  "sprites": [
    "/abs/path/to/sprite1.png",
    "/abs/path/to/sprite2.png"
  ],
  "settings": {
    "max_size": 4096,
    "spacing": 0,
    "padding": 0,
    "extrude": 0,
    "trim": true,
    "trim_threshold": 0,
    "rotate": true,
    "pot": true,
    "polygon": false,
    "tolerance": 2.0,
    "quantize": false,
    "quantize_quality": 85,
    "format_idx": 0
  }
}
```

## Menu Bar

| Menu | Items |
|------|-------|
| **File** | New Project, Open Project..., Save Project, Save Project As..., Add Sprites..., Open Atlas Preview..., Open Recent >, Export As... |
| **View** | Dark Mode toggle |
| **Help** | About tex_packer, License |

## Toast Notifications
- Right-top overlay, auto-dismiss after 5s with fade-out
- 3 levels: Info (blue), Success (green), Error (red)
- Triggered on: pack success, save, open, errors

## Dialogs
- **About**: Version, tech stack, license
- **License**: Third-party licenses, GPL note for imagequant
- **Export As**: JSON Hash / JSON Array / Godot tpsheet
- **Unsaved Changes**: Save & Continue / Discard / Cancel

## Theme

### Light Theme (Default) — Zed One Light inspired
- Background: `#FAFAFA` / Panel: `#F4F4F6`
- Accent: `#3884F4` (blue)
- Text: `#242933`
- Rounded corners: 6px controls, 10px windows
- Shadow: soft drop shadow on windows

### Dark Theme
- Background: `#1E2026` / Panel: `#24272E`
- Accent: `#569CFF`
- Text: `#DCDEE4`

### Fonts
- **Inter** — UI proportional (14px body, 18px heading)
- **JetBrains Mono** — monospace (13px)
- Embedded in binary via `include_bytes!`

## Drag & Drop
- OS-level file drop supported (via egui `DroppedFile` + `hovered_files`)
- Accepted: PNG, JPG, JPEG, BMP, GIF, TGA, WebP + directories (recursively scanned)
- Duplicates auto-skipped (path comparison)
- Drop works on any area when in Packer mode
- **Drag hover overlay**: Blue tinted full-screen overlay with "Drop sprites here" text and dashed border when files are hovering over the window
- **Auto-pack on drop**: After sprites are added via drag-drop, packing is automatically triggered in background — no need to click Pack button

## Inline Preview
- Generated automatically after Pack! completes (or after drag-drop auto-pack)
- Reads the output atlas PNG back and renders in-app
- Same interactive canvas as Viewer: zoom, pan, grid, hover, names
- Texture invalidated on re-pack (new texture handle created)

## Background Packing
- Pack runs in `std::thread::spawn` — UI never freezes
- **Loading overlay**: Semi-transparent dark overlay with centered spinner + "Packing..." text
- Pack button shows "Packing..." and is disabled during pack
- Result polled via `mpsc::channel` + `try_recv()` each frame
- `ctx.request_repaint()` keeps UI responsive during pack

## Keyboard Shortcuts (planned)
- Cmd/Ctrl+N: New Project
- Cmd/Ctrl+O: Open Project
- Cmd/Ctrl+S: Save Project
- Cmd/Ctrl+Shift+S: Save As
- Cmd/Ctrl+P: Pack

## Changelog

### v0.1.0
- Initial implementation

### v0.1.1 (current)
- Fix: Split view layout broken (horizontal mode mixed sprite list and preview) — switched from `ui.horizontal()` + `allocate_ui()` to proper `SidePanel` / `TopBottomPanel` with resizable splitters
- Fix: Negative width crash on Grid layout (`available_width() - 80.0` going negative)
- Feat: Background packing — Pack runs in separate thread, UI stays responsive
- Feat: Drag hover overlay — blue tinted overlay with "Drop sprites here" when files hover
- Feat: Packing loading overlay — dark semi-transparent overlay with spinner during pack
- Feat: Auto-pack on drop — dragging sprites triggers automatic pack, preview appears without clicking Pack
- Feat: Auto-pack on project open — opening a .tpproj with sprites immediately packs and shows preview
- Feat: Directories in drag-drop — dropping a folder recursively scans for images
- Change: Auto-pack enabled by default (was off)
- Welcome screen, Packer workspace, Viewer
- Menu bar with File/View/Help
- Toast notification system
- Modal dialogs (About, License, Export As, Unsaved Changes)
- Project file (.tpproj) save/load
- Drag & drop sprite import
- Split-screen inline preview (horizontal/vertical)
- Light/Dark theme with Inter + JetBrains Mono fonts
- wgpu render backend (Metal/Vulkan/DX12/GL)
