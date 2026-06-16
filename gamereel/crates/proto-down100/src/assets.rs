//! 渲染所需的 PNG 资源 — 用 `image` crate 内存生成, 写到 caller 指定目录.
//!
//! 4 类资源:
//!   - `bg.png` (720x1080, 蓝灰渐变 + 标题"DOWN 100" + room_id/players HUD)
//!   - `floor.png` (240x16, 灰色长条 + 顶面高光)
//!   - `player_N.png` × 4 (32x32, 不同颜色圆形, N=0..3)
//!
//! 跟 tools/down100-replay-render/compose_scene.py 的 render_assets() 视觉等价.

use std::path::Path;

use ab_glyph::{FontArc, PxScale};
use image::{ImageBuffer, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

/// 玩家颜色调色板 (跟 translate::PLAYER_COLORS 数量对齐)
const PLAYER_COLORS: &[(u8, u8, u8)] = &[
    (231, 76, 60),   // red
    (52, 152, 219),  // blue
    (39, 174, 96),   // green
    (155, 89, 182),  // purple
];

const CANVAS_W: u32 = 720;
const CANVAS_H: u32 = 1080;

/// 写所有资源到 `dir`. caller 需要保证 dir 存在.
pub fn write_assets(
    dir: &Path,
    room_id: &str,
    player_ids: &[String],
) -> std::io::Result<()> {
    write_bg(dir, room_id, player_ids)?;
    write_floor(dir)?;
    for (i, _) in PLAYER_COLORS.iter().enumerate() {
        write_player(dir, i)?;
    }
    Ok(())
}

fn write_bg(dir: &Path, room_id: &str, player_ids: &[String]) -> std::io::Result<()> {
    let mut img: RgbaImage = ImageBuffer::new(CANVAS_W, CANVAS_H);
    // 上→下蓝灰渐变
    for y in 0..CANVAS_H {
        let ratio = y as f32 / CANVAS_H as f32;
        let r = (30.0 + ratio * 20.0) as u8;
        let g = (30.0 + ratio * 30.0) as u8;
        let b = (50.0 + ratio * 60.0) as u8;
        for x in 0..CANVAS_W {
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
    // 顶部 HUD — 标题 + 房间号 + 玩家
    if let Some(font) = try_load_font() {
        let white = Rgba([255, 255, 255, 220]);
        let gray = Rgba([200, 200, 220, 200]);
        let footer = Rgba([160, 160, 180, 180]);
        draw_text_mut(&mut img, white, 20, 20, PxScale::from(36.0), &font, "DOWN 100");
        let line2 = format!("room: {room_id}");
        draw_text_mut(&mut img, gray, 20, 64, PxScale::from(18.0), &font, &line2);
        let line3 = format!(
            "players: {}",
            player_ids
                .iter()
                .map(|p| &p[..p.len().min(8)])
                .collect::<Vec<_>>()
                .join(", ")
        );
        draw_text_mut(&mut img, gray, 20, 86, PxScale::from(18.0), &font, &line3);
        // 底部 watermark
        draw_text_mut(
            &mut img,
            footer,
            (CANVAS_W - 200) as i32,
            (CANVAS_H - 30) as i32,
            PxScale::from(18.0),
            &font,
            "titan-forge replay",
        );
    }
    img.save(dir.join("bg.png"))
        .map_err(|e| std::io::Error::other(e.to_string()))
}

fn write_floor(dir: &Path) -> std::io::Result<()> {
    let mut img: RgbaImage = ImageBuffer::new(240, 16);
    // 灰色底
    draw_filled_rect_mut(&mut img, Rect::at(0, 0).of_size(240, 16), Rgba([128, 128, 128, 255]));
    // 顶部高光
    draw_filled_rect_mut(&mut img, Rect::at(0, 0).of_size(240, 3), Rgba([180, 180, 180, 255]));
    img.save(dir.join("floor.png"))
        .map_err(|e| std::io::Error::other(e.to_string()))
}

fn write_player(dir: &Path, idx: usize) -> std::io::Result<()> {
    let mut img: RgbaImage = ImageBuffer::new(32, 32);
    let (r, g, b) = PLAYER_COLORS[idx % PLAYER_COLORS.len()];
    draw_filled_circle_mut(&mut img, (16, 16), 15, Rgba([r, g, b, 255]));
    // 高光小圆 (中心偏左上)
    draw_filled_circle_mut(&mut img, (12, 12), 5, Rgba([255, 255, 255, 200]));
    img.save(dir.join(format!("player_{idx}.png")))
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// 尝试加载系统字体. 找不到字体时 HUD 文字降级为空 (画面仍可看, 只是没字).
fn try_load_font() -> Option<FontArc> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/Library/Fonts/Arial.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }
    None
}
