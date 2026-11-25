//! Deterministic swimlane geometry layout and orthogonal bus edge routing.
//! Uses Rayon for parallel diagram layout and SIMD `BatchBounds` for extent reduction.

use crate::schema::{
    BadgeSpec, CalloutSpec, CardSpec, ColumnSpec, DiagramSpec, EdgeKind, EdgeSpec, SubcardSpec,
    WarningBannerSpec,
};
use crate::simd_ops::{BatchBounds, estimate_text_width_px};
use rayon::prelude::*;
use std::collections::HashMap;

const DEFAULT_COL_WIDTHS: [f64; 4] = [340.0, 440.0, 600.0, 420.0];
const DEFAULT_COL_GAPS: [f64; 3] = [60.0, 80.0, 40.0];
const LEFT_MARGIN: f64 = 60.0;
const RIGHT_MARGIN: f64 = 60.0;
const HEADER_START_Y: f64 = 54.0;
const CARD_GAP_Y: f64 = 12.0;
const COL_HEADER_GAP: f64 = 12.0;

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn cx(&self) -> f64 {
        self.x + self.w * 0.5
    }
    pub fn cy(&self) -> f64 {
        self.y + self.h * 0.5
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

#[derive(Debug, Clone)]
pub struct LaidOutBadge {
    pub rect: Rect,
    pub spec: BadgeSpec,
}

#[derive(Debug, Clone)]
pub struct LaidOutSubcard {
    pub rect: Rect,
    pub badge: Option<LaidOutBadge>,
    pub spec: SubcardSpec,
}

#[derive(Debug, Clone)]
pub struct LaidOutCard {
    pub col_index: usize,
    pub rect: Rect,
    pub badge: Option<LaidOutBadge>,
    pub subcards: Vec<LaidOutSubcard>,
    pub spec: CardSpec,
}

#[derive(Debug, Clone)]
pub struct LaidOutColumn {
    pub header_x: f64,
    pub header_y: f64,
    pub width: f64,
    pub title: String,
    pub cards: Vec<LaidOutCard>,
}

#[derive(Debug, Clone)]
pub struct RoutedEdge {
    /// Source and target card IDs for interactive tracing in HTML.
    pub from_ids: Vec<String>,
    pub to_ids: Vec<String>,
    /// Spine lines without arrowheads (e.g. shared vertical bus segments).
    pub spines: Vec<(f64, f64, f64, f64)>,
    /// Directed polyline paths rendered with arrowhead markers.
    pub paths: Vec<String>,
    /// Exact numeric polylines matching `paths` (used for WebGPU / GIF flow-pulse animation).
    pub polylines: Vec<Vec<(f64, f64)>>,
    /// Optional white-backed text label on the edge/bus.
    pub label: Option<(f64, f64, f64, String)>,
}

#[derive(Debug, Clone)]
pub struct LaidOutCallout {
    pub rect: Rect,
    pub spec: CalloutSpec,
}

#[derive(Debug, Clone)]
pub struct LaidOutWarning {
    pub rect: Rect,
    pub spec: WarningBannerSpec,
}

#[derive(Debug, Clone)]
pub struct LaidOutDiagram {
    pub id: String,
    pub heading: String,
    pub note: String,
    pub svg_title: String,
    pub canvas_title: String,
    pub subtitles: Vec<(f64, crate::schema::StyledLine)>,
    pub width: f64,
    pub height: f64,
    pub columns: Vec<LaidOutColumn>,
    pub edges: Vec<RoutedEdge>,
    pub callouts: Vec<LaidOutCallout>,
    pub warning: Option<LaidOutWarning>,
    pub footer: String,
    pub footer_y: f64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DynamicBounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl LaidOutDiagram {
    /// Compute the minimal dynamic bounding box enclosing all animated edge polylines
    /// and status badge beacons, padded cleanly for radial glow radius.
    /// Ensures neither HTML WebGPU/Canvas2D nor exported GIFs ever waste full-screen area.
    pub fn dynamic_bounds(&self, pad: f64) -> Option<DynamicBounds> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut any = false;

        for edge in &self.edges {
            for poly in &edge.polylines {
                for &(px, py) in poly {
                    min_x = min_x.min(px);
                    min_y = min_y.min(py);
                    max_x = max_x.max(px);
                    max_y = max_y.max(py);
                    any = true;
                }
            }
        }

        for col in &self.columns {
            for card in &col.cards {
                if let Some(b) = &card.badge
                    && matches!(
                        b.spec.tone,
                        crate::schema::Tone::Red
                            | crate::schema::Tone::Emerald
                            | crate::schema::Tone::Green
                    )
                {
                    let bx = b.rect.x;
                    let by = b.rect.cy();
                    min_x = min_x.min(bx);
                    min_y = min_y.min(by);
                    max_x = max_x.max(bx);
                    max_y = max_y.max(by);
                    any = true;
                }
            }
        }

        if !any {
            return None;
        }
        let x0 = (min_x - pad).floor().max(0.0);
        let y0 = (min_y - pad).floor().max(0.0);
        let x1 = (max_x + pad).ceil().min(self.width);
        let y1 = (max_y + pad).ceil().min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(DynamicBounds {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }
}

/// Lay out all diagrams in parallel using Rayon.
pub fn layout_diagrams_parallel(diagrams: &[DiagramSpec]) -> Vec<LaidOutDiagram> {
    if diagrams.len() <= 1 {
        diagrams.iter().map(layout_diagram).collect()
    } else {
        diagrams.par_iter().map(layout_diagram).collect()
    }
}

pub fn layout_diagram(spec: &DiagramSpec) -> LaidOutDiagram {
    // 1. Subtitle block positions.
    let mut subtitles = Vec::with_capacity(spec.subtitles.len());
    let mut header_y = HEADER_START_Y + 32.0;
    for (i, line) in spec.subtitles.iter().enumerate() {
        subtitles.push((header_y, line.clone()));
        header_y += if i == 0 { 22.0 } else { 20.0 };
    }
    let col_header_y = header_y.max(156.0);
    let cards_start_y = col_header_y + COL_HEADER_GAP;

    // 2. Column X offsets and widths.
    let mut col_x = Vec::with_capacity(spec.columns.len());
    let mut col_w = Vec::with_capacity(spec.columns.len());
    let mut cur_x = LEFT_MARGIN;
    for (ci, col) in spec.columns.iter().enumerate() {
        let w = col.width.unwrap_or_else(|| {
            DEFAULT_COL_WIDTHS
                .get(ci)
                .copied()
                .unwrap_or(420.0)
        });
        col_x.push(cur_x);
        col_w.push(w);
        let gap = DEFAULT_COL_GAPS.get(ci).copied().unwrap_or(60.0);
        cur_x += w + gap;
    }

    let natural_width = (cur_x - DEFAULT_COL_GAPS.last().copied().unwrap_or(40.0) + RIGHT_MARGIN)
        .max(2100.0);

    // 3. First pass: compute card heights & vertical stacking, honoring `align_with` hints.
    let mut card_rects: HashMap<String, (usize, Rect)> = HashMap::new();
    let mut laid_columns = Vec::with_capacity(spec.columns.len());
    let mut bounds = BatchBounds::with_capacity(32);

    for (ci, col) in spec.columns.iter().enumerate() {
        let x = col_x[ci];
        let w = col_w[ci];
        let mut cur_y = cards_start_y;
        let mut laid_cards = Vec::with_capacity(col.cards.len());

        for card in &col.cards {
            if let Some(align_id) = &card.align_with
                && let Some((_, target_rect)) = card_rects.get(align_id)
                && target_rect.y > cur_y
            {
                cur_y = target_rect.y;
            }
            let laid = layout_card(ci, x, cur_y, w, card);
            bounds.push(laid.rect.x, laid.rect.y, laid.rect.w, laid.rect.h);
            card_rects.insert(card.id.clone(), (ci, laid.rect.clone()));
            cur_y = laid.rect.bottom() + card_gap_for_col(ci, col);
            laid_cards.push(laid);
        }

        laid_columns.push(LaidOutColumn {
            header_x: x,
            header_y: col_header_y,
            width: w,
            title: col.title.clone(),
            cards: laid_cards,
        });
    }

    // 4. SIMD reduction for main column max extents.
    let (max_card_x, max_card_y) = bounds.global_max_xy();
    let canvas_width = natural_width.max(max_card_x + RIGHT_MARGIN);

    // 5. Route edges cleanly.
    let has_bottom_route = spec
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::BottomRoute));
    let bottom_route_y = (max_card_y + 38.0).max(1250.0);

    let edges: Vec<RoutedEdge> = spec
        .edges
        .iter()
        .filter_map(|e| route_edge(e, &card_rects, bottom_route_y))
        .collect();

    // 6. Bottom callout row (2-column split matching canonical 960 + 1000 layout).
    let mut cursor_y = if has_bottom_route {
        // Place callouts neatly at y >= max_card_y + 38, keeping bottom route below or clear.
        (max_card_y + 38.0).max(1250.0)
    } else {
        (max_card_y + 38.0).max(1250.0)
    };

    let mut laid_callouts = Vec::with_capacity(spec.callouts.len());
    if !spec.callouts.is_empty() {
        let count = spec.callouts.len();
        let inner_w = canvas_width - 100.0;
        let gap = 20.0;
        let callout_h = spec
            .callouts
            .iter()
            .map(estimate_callout_height)
            .fold(196.0_f64, f64::max);

        for (i, c) in spec.callouts.iter().enumerate() {
            let (cx, cw) = if count == 1 {
                (50.0, inner_w)
            } else if count == 2 {
                let left_w = (inner_w - gap) * 0.49;
                let right_w = inner_w - gap - left_w;
                if i == 0 {
                    (50.0, left_w)
                } else {
                    (50.0 + left_w + gap, right_w)
                }
            } else {
                let cw = (inner_w - gap * (count as f64 - 1.0)) / count as f64;
                (50.0 + i as f64 * (cw + gap), cw)
            };
            laid_callouts.push(LaidOutCallout {
                rect: Rect {
                    x: cx,
                    y: cursor_y,
                    w: cw,
                    h: callout_h,
                },
                spec: c.clone(),
            });
        }
        cursor_y += callout_h + 16.0;
    }

    // 7. Optional warning banner.
    let warning = spec.warning.as_ref().map(|w| {
        let h = 72.0;
        let rect = Rect {
            x: 50.0,
            y: cursor_y,
            w: canvas_width - 100.0,
            h,
        };
        cursor_y += h + 20.0;
        LaidOutWarning {
            rect,
            spec: w.clone(),
        }
    });

    let footer_y = (cursor_y + 30.0).max(1584.0);
    let canvas_height = footer_y + 16.0;

    LaidOutDiagram {
        id: spec.id.clone(),
        heading: spec.heading.clone(),
        note: spec.note.clone(),
        svg_title: if spec.svg_title.is_empty() {
            spec.heading.clone()
        } else {
            spec.svg_title.clone()
        },
        canvas_title: if spec.canvas_title.is_empty() {
            spec.heading.clone()
        } else {
            spec.canvas_title.clone()
        },
        subtitles,
        width: canvas_width,
        height: canvas_height,
        columns: laid_columns,
        edges,
        callouts: laid_callouts,
        warning,
        footer: spec.footer.clone(),
        footer_y,
    }
}

fn card_gap_for_col(col_idx: usize, col: &ColumnSpec) -> f64 {
    if col_idx == 2 && col.cards.iter().any(|c| !c.subcards.is_empty()) {
        16.0
    } else {
        CARD_GAP_Y
    }
}

fn layout_card(col_index: usize, x: f64, y: f64, w: f64, spec: &CardSpec) -> LaidOutCard {
    let mut inner_y = y + 40.0;

    let badge = spec.badge.as_ref().map(|b| layout_badge_right(x + w - 12.0, y + 10.0, b));

    // Plain/mono lines before subcards.
    if !spec.lines.is_empty() {
        inner_y += 6.0;
        for line in &spec.lines {
            inner_y += if line.mono { 20.5 } else { 22.0 };
        }
    }

    // Nested subcards (e.g. external system actions).
    let mut subcards = Vec::with_capacity(spec.subcards.len());
    if !spec.subcards.is_empty() {
        inner_y = inner_y.max(y + 42.0);
        for sub in &spec.subcards {
            let sub_h = estimate_subcard_height(sub);
            let sub_rect = Rect {
                x: x + 14.0,
                y: inner_y,
                w: w - 28.0,
                h: sub_h,
            };
            let sub_badge = sub
                .badge
                .as_ref()
                .map(|b| layout_badge_right(sub_rect.right() - 10.0, sub_rect.y + 9.0, b));
            subcards.push(LaidOutSubcard {
                rect: sub_rect,
                badge: sub_badge,
                spec: sub.clone(),
            });
            inner_y += sub_h + 8.0;
        }
    }

    if spec.footer.is_some() {
        inner_y += 18.0;
    }

    let h = (inner_y - y + 10.0).max(88.0);
    LaidOutCard {
        col_index,
        rect: Rect { x, y, w, h },
        badge,
        subcards,
        spec: spec.clone(),
    }
}

fn estimate_subcard_height(sub: &SubcardSpec) -> f64 {
    let line_count = sub.lines.len().max(1);
    if line_count <= 1 {
        64.0
    } else {
        44.0 + line_count as f64 * 16.0
    }
}

fn estimate_callout_height(c: &CalloutSpec) -> f64 {
    let line_step = match c.style {
        crate::schema::CalloutStyle::CodeBox => 18.0,
        crate::schema::CalloutStyle::Bullets => 22.0,
    };
    (68.0 + c.lines.len() as f64 * line_step).max(150.0)
}

fn layout_badge_right(right_edge: f64, top_y: f64, spec: &BadgeSpec) -> LaidOutBadge {
    let text_w = estimate_text_width_px(&spec.text, 10.5, false);
    let w = (text_w + 18.0).clamp(40.0, 180.0);
    LaidOutBadge {
        rect: Rect {
            x: right_edge - w,
            y: top_y,
            w,
            h: 20.0,
        },
        spec: spec.clone(),
    }
}

fn route_edge(
    edge: &EdgeSpec,
    cards: &HashMap<String, (usize, Rect)>,
    bottom_route_y: f64,
) -> Option<RoutedEdge> {
    let from_rects: Vec<&(usize, Rect)> = edge
        .from
        .iter()
        .filter_map(|id| cards.get(id))
        .collect();
    let to_rects: Vec<&(usize, Rect)> = edge.to.iter().filter_map(|id| cards.get(id)).collect();
    if from_rects.is_empty() || to_rects.is_empty() {
        return None;
    }

    match edge.kind {
        EdgeKind::Direct => {
            let (fc, fr) = from_rects[0];
            let (tc, tr) = to_rects[0];
            let pts: Vec<(f64, f64)> = if fc == tc {
                let x = fr.cx();
                if fr.y <= tr.y {
                    vec![(x, fr.bottom()), (x, tr.y)]
                } else {
                    vec![(x, fr.y), (x, tr.bottom())]
                }
            } else if fr.right() <= tr.x {
                if (fr.cy() - tr.cy()).abs() < 24.0 {
                    vec![(fr.right(), fr.cy()), (tr.x, fr.cy())]
                } else {
                    let mid_x = (fr.right() + tr.x) * 0.5;
                    vec![
                        (fr.right(), fr.cy()),
                        (mid_x, fr.cy()),
                        (mid_x, tr.cy()),
                        (tr.x, tr.cy()),
                    ]
                }
            } else {
                let mid_x = (fr.x + tr.right()) * 0.5;
                vec![
                    (fr.x, fr.cy()),
                    (mid_x, fr.cy()),
                    (mid_x, tr.cy()),
                    (tr.right(), tr.cy()),
                ]
            };

            let path = format_polyline(&pts);
            let label = edge.label.as_ref().map(|t| {
                let lw = estimate_text_width_px(t, 11.0, false) + 12.0;
                ((fr.right() + tr.x) * 0.5, (fr.cy() + tr.cy()) * 0.5 - 8.0, lw, t.clone())
            });

            Some(RoutedEdge {
                from_ids: edge.from.clone(),
                to_ids: edge.to.clone(),
                spines: vec![],
                paths: vec![path],
                polylines: vec![pts],
                label,
            })
        }
        EdgeKind::Bus => {
            let max_from_right = from_rects
                .iter()
                .map(|(_, r)| r.right())
                .fold(0.0_f64, f64::max);
            let min_to_left = to_rects
                .iter()
                .map(|(_, r)| r.x)
                .fold(f64::INFINITY, f64::min);
            let slot = edge.bus_slot.unwrap_or(0.5).clamp(0.15, 0.85);
            let bus_x = if min_to_left > max_from_right {
                max_from_right + (min_to_left - max_from_right) * slot
            } else {
                max_from_right + 20.0
            };

            let mut all_ys = Vec::with_capacity(from_rects.len() + to_rects.len());
            let mut paths = Vec::with_capacity(from_rects.len() + to_rects.len());

            for (_, fr) in &from_rects {
                let y = fr.cy();
                all_ys.push(y);
                paths.push(format!("M{:.1},{y:.1} L{bus_x:.1},{y:.1}", fr.right()));
            }
            for (_, tr) in &to_rects {
                let y = tr.cy();
                all_ys.push(y);
                paths.push(format!("M{bus_x:.1},{y:.1} L{:.1},{y:.1}", tr.x));
            }

            // Build continuous animated flow polylines from each source through the bus to each target.
            let mut polylines = Vec::with_capacity(from_rects.len() * to_rects.len());
            for (_, fr) in &from_rects {
                for (_, tr) in &to_rects {
                    polylines.push(vec![
                        (fr.right(), fr.cy()),
                        (bus_x, fr.cy()),
                        (bus_x, tr.cy()),
                        (tr.x, tr.cy()),
                    ]);
                }
            }

            let min_y = all_ys.iter().copied().fold(f64::INFINITY, f64::min);
            let max_y = all_ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let spines = if (max_y - min_y).abs() > 1.0 {
                vec![(bus_x, min_y, bus_x, max_y)]
            } else {
                vec![]
            };

            let label = edge.label.as_ref().map(|t| {
                let lw = (estimate_text_width_px(t, 11.0, false) + 16.0).max(52.0);
                (bus_x, (min_y - 12.0).max(162.0), lw, t.clone())
            });

            Some(RoutedEdge {
                from_ids: edge.from.clone(),
                to_ids: edge.to.clone(),
                spines,
                paths,
                polylines,
                label,
            })
        }
        EdgeKind::LoopLeft => {
            let (_, fr) = from_rects[0];
            let (_, tr) = to_rects[0];
            let loop_x = fr.x.min(tr.x) - 16.0;
            let pts = vec![
                (fr.x, fr.cy()),
                (loop_x, fr.cy()),
                (loop_x, tr.cy()),
                (tr.x, tr.cy()),
            ];
            let path = format_polyline(&pts);
            Some(RoutedEdge {
                from_ids: edge.from.clone(),
                to_ids: edge.to.clone(),
                spines: vec![],
                paths: vec![path],
                polylines: vec![pts],
                label: None,
            })
        }
        EdgeKind::BottomRoute => {
            let (_, fr) = from_rects[0];
            let (_, tr) = to_rects[0];
            let route_y = bottom_route_y - 12.0;
            let pts = vec![
                (fr.cx(), fr.bottom()),
                (fr.cx(), route_y),
                (tr.cx(), route_y),
                (tr.cx(), tr.bottom()),
            ];
            let path = format_polyline(&pts);
            Some(RoutedEdge {
                from_ids: edge.from.clone(),
                to_ids: edge.to.clone(),
                spines: vec![],
                paths: vec![path],
                polylines: vec![pts],
                label: None,
            })
        }
    }
}

fn format_polyline(pts: &[(f64, f64)]) -> String {
    let mut out = String::with_capacity(pts.len() * 16);
    for (i, (x, y)) in pts.iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("M{x:.1},{y:.1}"));
        } else {
            out.push_str(&format!(" L{x:.1},{y:.1}"));
        }
    }
    out
}
