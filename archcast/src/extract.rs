//! Reverse parser: extracts legacy or archcast-rendered HTML + inline SVG overviews
//! into a clean, AI-friendly [`ArchSpec`] using `memchr` SIMD scanning, `roxmltree`,
//! and `wide::f64x4` spatial bounding-box containment.

use crate::error::{ArchcastError, Result};
use crate::schema::{
    ArchSpec, BadgeSpec, CalloutSpec, CalloutStyle, CardSpec, ColumnSpec, DiagramSpec, EdgeKind,
    EdgeSpec, FlowGroupSpec, FlowRowSpec, FlowTableSpec, SPEC_VERSION, StyledLine, SubcardSpec,
    Tone, WarningBannerSpec,
};
use crate::simd_ops::BatchBounds;
use memchr::memmem;
use std::collections::HashSet;
use std::path::Path;

pub fn extract_html_file(path: &Path) -> Result<ArchSpec> {
    let html = std::fs::read_to_string(path).map_err(|source| ArchcastError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    extract_html_str(&html, path)
}

pub fn extract_html_str(html: &str, source_path: &Path) -> Result<ArchSpec> {
    let doc_title = extract_first_tag_inner(html, "<title", "</title>").map(|s| decode_entities(s.trim()));
    let title = extract_first_tag_inner(html, "<h1", "</h1>")
        .map(strip_tags_inline)
        .or_else(|| doc_title.clone())
        .unwrap_or_else(|| {
            source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Architecture Overview")
                .to_string()
        });

    let lead = extract_tag_with_class(html, "p", "lead")
        .map(|raw| split_html_lines(&raw))
        .unwrap_or_default();

    let cards = split_card_divs(html);
    let mut diagrams = Vec::new();
    let mut flow_table = None;

    for (idx, (card_id, card_body)) in cards.into_iter().enumerate() {
        let heading = extract_first_tag_inner(&card_body, "<h2", "</h2>")
            .map(strip_tags_inline)
            .unwrap_or_else(|| format!("Section {}", idx + 1));
        let note = extract_tag_with_class(&card_body, "p", "note")
            .map(|s| html_inline_to_md(&s))
            .unwrap_or_default();

        if let Some(svg_slice) = extract_tag_outer(&card_body, "<svg", "</svg>") {
            let id = if card_id.is_empty() {
                format!("fig{}", diagrams.len() + 1)
            } else {
                card_id
            };
            let diag = parse_svg_diagram(svg_slice, &id, &heading, &note, source_path)?;
            diagrams.push(diag);
        } else if let Some(table_slice) = extract_tag_outer(&card_body, "<table", "</table>") {
            let id = if card_id.is_empty() {
                "flowtable".to_string()
            } else {
                card_id
            };
            let hint = extract_tag_with_class(&card_body, "p", "hint")
                .map(|s| html_inline_to_md(&s));
            let groups = parse_flow_table(table_slice);
            flow_table = Some(FlowTableSpec {
                id,
                heading,
                note,
                groups,
                hint,
            });
        }
    }

    Ok(ArchSpec {
        schema_version: SPEC_VERSION,
        doc_title,
        title,
        lead,
        diagrams,
        flow_table,
    })
}

#[derive(Debug, Clone)]
struct RawRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rx: f64,
    fill: String,
    opacity: f64,
}

#[derive(Debug, Clone)]
struct RawText {
    x: f64,
    y: f64,
    size: f64,
    fill: String,
    bold: bool,
    mono: bool,
    text: String,
}

#[derive(Debug, Clone)]
struct RawLine {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

#[derive(Debug, Clone)]
struct RawPath {
    points: Vec<(f64, f64)>,
}

fn parse_svg_diagram(
    svg_xml: &str,
    id: &str,
    heading: &str,
    note: &str,
    source_path: &Path,
) -> Result<DiagramSpec> {
    let doc = roxmltree::Document::parse(svg_xml).map_err(|e| ArchcastError::XmlParse {
        path: source_path.to_path_buf(),
        message: e.to_string(),
    })?;

    let root = doc.root_element();
    let svg_height = attr_f64(&root, "height").unwrap_or(1600.0);

    let mut svg_title = String::new();
    let mut rects = Vec::new();
    let mut texts = Vec::new();
    let mut lines = Vec::new();
    let mut paths = Vec::new();

    for node in doc.descendants() {
        if !node.is_element() {
            continue;
        }
        match node.tag_name().name() {
            "title" => {
                if let Some(t) = node.text() {
                    svg_title = t.trim().to_string();
                }
            }
            "rect" => {
                let x = attr_f64(&node, "x").unwrap_or(0.0);
                let y = attr_f64(&node, "y").unwrap_or(0.0);
                let w = attr_f64(&node, "width").unwrap_or(0.0);
                let h = attr_f64(&node, "height").unwrap_or(0.0);
                let rx = attr_f64(&node, "rx").unwrap_or(0.0);
                let fill = node.attribute("fill").unwrap_or("").trim().to_ascii_lowercase();
                let opacity = attr_f64(&node, "opacity").unwrap_or(1.0);
                // Ignore full-canvas white background rect.
                if x == 0.0 && y == 0.0 && w >= 1800.0 && h >= 1000.0 {
                    continue;
                }
                rects.push(RawRect {
                    x,
                    y,
                    w,
                    h,
                    rx,
                    fill,
                    opacity,
                });
            }
            "text" => {
                let raw_text = node.text().unwrap_or("").trim();
                if raw_text.is_empty() {
                    continue;
                }
                let x = attr_f64(&node, "x").unwrap_or(0.0);
                let y = attr_f64(&node, "y").unwrap_or(0.0);
                let size = attr_f64(&node, "font-size").unwrap_or(12.0);
                let fill = node.attribute("fill").unwrap_or("").trim().to_ascii_lowercase();
                let bold = node.attribute("font-weight").unwrap_or("") == "bold";
                let mono = node
                    .attribute("font-family")
                    .map(|f| f.to_ascii_lowercase().contains("monospace"))
                    .unwrap_or(false);
                texts.push(RawText {
                    x,
                    y,
                    size,
                    fill,
                    bold,
                    mono,
                    text: raw_text.to_string(),
                });
            }
            "line" => {
                lines.push(RawLine {
                    x1: attr_f64(&node, "x1").unwrap_or(0.0),
                    y1: attr_f64(&node, "y1").unwrap_or(0.0),
                    x2: attr_f64(&node, "x2").unwrap_or(0.0),
                    y2: attr_f64(&node, "y2").unwrap_or(0.0),
                });
            }
            "path" => {
                if let Some(d) = node.attribute("d") {
                    let pts = parse_svg_polyline_points(d);
                    if pts.len() >= 2 {
                        paths.push(RawPath { points: pts });
                    }
                }
            }
            _ => {}
        }
    }

    // 1. Canvas header, subtitles, column headers, footer.
    let mut canvas_title = String::new();
    let mut subtitles = Vec::new();
    let mut col_headers: Vec<(f64, String)> = Vec::new();
    let mut footer = String::new();
    let mut used_text = vec![false; texts.len()];

    for (ti, t) in texts.iter().enumerate() {
        if t.y <= 70.0 && t.size >= 20.0 {
            canvas_title = t.text.clone();
            used_text[ti] = true;
        } else if t.y > 70.0 && t.y < 145.0 && t.x <= 80.0 {
            subtitles.push(StyledLine {
                text: t.text.clone(),
                mono: t.mono,
                bold: t.bold,
                tone: Tone::from_hex(&t.fill),
                size: Some(t.size),
            });
            used_text[ti] = true;
        } else if (145.0..=170.0).contains(&t.y) && t.bold {
            col_headers.push((t.x, t.text.clone()));
            used_text[ti] = true;
        } else if t.y >= svg_height - 30.0 && t.x <= 80.0 {
            footer = t.text.clone();
            used_text[ti] = true;
        }
    }
    col_headers.sort_by(|a, b| a.0.total_cmp(&b.0));

    // 2. Classify rectangles into cards, subcards, badges, callouts, warning banner.
    let mut main_cards: Vec<(RawRect, Tone)> = Vec::new();
    let mut subcard_rects: Vec<RawRect> = Vec::new();
    let mut badge_rects: Vec<RawRect> = Vec::new();
    let mut callout_rects: Vec<(RawRect, Tone, bool)> = Vec::new();
    let mut warning_rect: Option<RawRect> = None;

    for r in &rects {
        if r.w <= 8.0 {
            // Accent bar — attached to its parent rect below.
            continue;
        }
        if r.fill == "#fff8ec" && r.w >= 800.0 {
            warning_rect = Some(r.clone());
        } else if (r.fill == "#fbfcfe" || (r.rx >= 11.5 && r.y >= 1100.0)) && r.w >= 400.0 {
            let accent = find_accent_tone(&rects, r.x, r.y, r.h);
            let has_codebox = rects.iter().any(|ir| {
                ir.fill == "#f8fafc"
                    && ir.x >= r.x
                    && ir.y >= r.y
                    && ir.x + ir.w <= r.x + r.w + 2.0
                    && ir.y + ir.h <= r.y + r.h + 2.0
            });
            callout_rects.push((r.clone(), accent, has_codebox));
        } else if r.fill == "#fdfefe" && r.w >= 180.0 && r.h >= 60.0 {
            let accent = find_accent_tone(&rects, r.x, r.y, r.h);
            main_cards.push((r.clone(), accent));
        } else if r.fill == "#ffffff" && r.rx >= 7.0 && r.w >= 200.0 && r.h >= 40.0 {
            subcard_rects.push(r.clone());
        } else if r.h <= 24.0 && r.rx >= 8.0 && r.opacity < 0.5 && r.w >= 28.0 {
            badge_rects.push(r.clone());
        }
    }

    // Build SIMD BatchBounds for main cards and subcards.
    let mut card_bounds = BatchBounds::with_capacity(main_cards.len());
    for (r, _) in &main_cards {
        card_bounds.push(r.x, r.y, r.w, r.h);
    }
    let mut sub_bounds = BatchBounds::with_capacity(subcard_rects.len());
    for r in &subcard_rects {
        sub_bounds.push(r.x, r.y, r.w, r.h);
    }

    // Match pill badges to their label text, then assign each badge to a subcard or main card.
    let mut card_badges: Vec<Option<BadgeSpec>> = vec![None; main_cards.len()];
    let mut sub_badges: Vec<Option<BadgeSpec>> = vec![None; subcard_rects.len()];

    for br in &badge_rects {
        let mut badge_text = None;
        for (ti, t) in texts.iter().enumerate() {
            if used_text[ti] {
                continue;
            }
            if t.x >= br.x - 4.0
                && t.x <= br.x + br.w + 4.0
                && t.y >= br.y
                && t.y <= br.y + br.h + 4.0
            {
                badge_text = Some((ti, t.text.clone(), Tone::from_hex(&t.fill)));
                break;
            }
        }
        if let Some((ti, text, tone)) = badge_text {
            used_text[ti] = true;
            let cx = br.x + br.w * 0.5;
            let cy = br.y + br.h * 0.5;
            if let Some(si) = sub_bounds.find_containing(cx, cy, 2.0) {
                sub_badges[si] = Some(BadgeSpec { text, tone });
            } else if let Some(ci) = card_bounds.find_containing(cx, cy, 2.0) {
                card_badges[ci] = Some(BadgeSpec { text, tone });
            }
        }
    }

    // Populate subcard texts via SIMD containment.
    let mut sub_texts: Vec<Vec<usize>> = vec![Vec::new(); subcard_rects.len()];
    let mut card_texts: Vec<Vec<usize>> = vec![Vec::new(); main_cards.len()];

    for (ti, t) in texts.iter().enumerate() {
        if used_text[ti] {
            continue;
        }
        if let Some(si) = sub_bounds.find_containing(t.x, t.y, 1.0) {
            sub_texts[si].push(ti);
            used_text[ti] = true;
        } else if let Some(ci) = card_bounds.find_containing(t.x, t.y, 1.0) {
            card_texts[ci].push(ti);
            used_text[ti] = true;
        }
    }

    // Attach subcards to their parent main cards.
    let mut card_sub_indices: Vec<Vec<usize>> = vec![Vec::new(); main_cards.len()];
    for (si, sr) in subcard_rects.iter().enumerate() {
        if let Some(ci) = card_bounds.find_containing(sr.x + 10.0, sr.y + 10.0, 2.0) {
            card_sub_indices[ci].push(si);
        }
    }

    // Build CardSpecs & map to columns.
    if col_headers.is_empty() {
        // Fallback: infer distinct column X coordinates from main cards.
        let mut xs: Vec<f64> = main_cards.iter().map(|(r, _)| (r.x / 20.0).round() * 20.0).collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        xs.dedup();
        for (i, x) in xs.into_iter().enumerate() {
            col_headers.push((x, format!("Column {}", i + 1)));
        }
    }

    let mut columns: Vec<ColumnSpec> = col_headers
        .iter()
        .enumerate()
        .map(|(i, (_, title))| ColumnSpec {
            id: format!("col_{}", i + 1),
            title: title.clone(),
            width: None,
            cards: Vec::new(),
        })
        .collect();

    let mut card_id_by_idx: Vec<String> = vec![String::new(); main_cards.len()];
    let mut card_col_by_idx: Vec<usize> = vec![0; main_cards.len()];
    let mut used_ids: HashSet<String> = HashSet::new();

    // Order cards by column then Y.
    let mut card_order: Vec<usize> = (0..main_cards.len()).collect();
    card_order.sort_by(|&a, &b| {
        let ca = nearest_col(&col_headers, main_cards[a].0.x);
        let cb = nearest_col(&col_headers, main_cards[b].0.x);
        ca.cmp(&cb)
            .then_with(|| main_cards[a].0.y.total_cmp(&main_cards[b].0.y))
    });

    for ci in card_order {
        let (ref rect, accent) = main_cards[ci];
        let col_idx = nearest_col(&col_headers, rect.x);
        card_col_by_idx[ci] = col_idx;

        let mut text_indices = card_texts[ci].clone();
        text_indices.sort_by(|&a, &b| texts[a].y.total_cmp(&texts[b].y));

        let mut card_title = format!("Card {}", ci + 1);
        let mut card_lines = Vec::new();
        let mut card_footer = None;
        let has_subs = !card_sub_indices[ci].is_empty();

        for (pos, &ti) in text_indices.iter().enumerate() {
            let t = &texts[ti];
            if pos == 0 && t.bold && t.size >= 13.5 {
                card_title = t.text.clone();
            } else if has_subs && t.y >= rect.y + rect.h - 22.0 && t.size <= 11.5 {
                card_footer = Some(t.text.clone());
            } else {
                card_lines.push(StyledLine {
                    text: t.text.clone(),
                    mono: t.mono,
                    bold: t.bold,
                    tone: Tone::from_hex(&t.fill),
                    size: None,
                });
            }
        }

        let mut sub_list = card_sub_indices[ci].clone();
        sub_list.sort_by(|&a, &b| subcard_rects[a].y.total_cmp(&subcard_rects[b].y));
        let subcards: Vec<SubcardSpec> = sub_list
            .into_iter()
            .map(|si| {
                let mut st = sub_texts[si].clone();
                st.sort_by(|&a, &b| texts[a].y.total_cmp(&texts[b].y));
                let mut stitle = String::new();
                let mut slines = Vec::new();
                for (pos, &ti) in st.iter().enumerate() {
                    let t = &texts[ti];
                    if pos == 0 && t.bold {
                        stitle = t.text.clone();
                    } else {
                        slines.push(StyledLine {
                            text: t.text.clone(),
                            mono: t.mono,
                            bold: t.bold,
                            tone: Tone::from_hex(&t.fill),
                            size: None,
                        });
                    }
                }
                SubcardSpec {
                    title: stitle,
                    badge: sub_badges[si].clone(),
                    lines: slines,
                }
            })
            .collect();

        let base_id = format!("c{}_{}", col_idx + 1, columns[col_idx].cards.len() + 1);
        let mut card_id = base_id.clone();
        let mut suffix = 2usize;
        while !used_ids.insert(card_id.clone()) {
            card_id = format!("{base_id}_{suffix}");
            suffix += 1;
        }
        card_id_by_idx[ci] = card_id.clone();

        // Detect cross-column horizontal alignment (e.g., Read-side card aligned to an External System card).
        let align_with = if col_idx > 0 {
            (0..main_cards.len())
                .find(|&other| {
                    card_col_by_idx[other] + 1 == col_idx
                        && !card_id_by_idx[other].is_empty()
                        && (main_cards[other].0.y - rect.y).abs() <= 4.0
                })
                .map(|other| card_id_by_idx[other].clone())
        } else {
            None
        };

        if columns[col_idx].width.is_none() {
            columns[col_idx].width = Some(rect.w);
        }

        columns[col_idx].cards.push(CardSpec {
            id: card_id,
            title: card_title,
            accent,
            badge: card_badges[ci].clone(),
            align_with,
            lines: card_lines,
            subcards,
            footer: card_footer,
        });
    }

    // 3. Reconstruct semantic edges from bus spines & paths via SIMD BatchBounds.
    let edges = reconstruct_edges(
        &lines,
        &paths,
        &texts,
        &used_text,
        &card_bounds,
        &card_id_by_idx,
        &main_cards,
    );

    // 4. Bottom callouts.
    callout_rects.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    let mut callouts = Vec::with_capacity(callout_rects.len());
    for (cr, accent, is_codebox) in &callout_rects {
        let mut ctexts: Vec<&RawText> = texts
            .iter()
            .enumerate()
            .filter(|(ti, t)| {
                !used_text[*ti]
                    && t.x >= cr.x
                    && t.x <= cr.x + cr.w
                    && t.y >= cr.y
                    && t.y <= cr.y + cr.h
            })
            .map(|(_, t)| t)
            .collect();
        ctexts.sort_by(|a, b| a.y.total_cmp(&b.y));

        let mut ctitle = String::new();
        let mut clines = Vec::new();
        for (pos, t) in ctexts.iter().enumerate() {
            if pos == 0 && t.bold {
                ctitle = t.text.clone();
            } else {
                clines.push(StyledLine {
                    text: t.text.clone(),
                    mono: t.mono || *is_codebox,
                    bold: t.bold,
                    tone: Tone::from_hex(&t.fill),
                    size: None,
                });
            }
        }
        callouts.push(CalloutSpec {
            title: ctitle,
            accent: *accent,
            style: if *is_codebox {
                CalloutStyle::CodeBox
            } else {
                CalloutStyle::Bullets
            },
            lines: clines,
        });
    }

    // 5. Optional warning banner.
    let warning = warning_rect.and_then(|wr| {
        let mut wtexts: Vec<&RawText> = texts
            .iter()
            .enumerate()
            .filter(|(ti, t)| {
                !used_text[*ti]
                    && t.x >= wr.x
                    && t.x <= wr.x + wr.w
                    && t.y >= wr.y
                    && t.y <= wr.y + wr.h
            })
            .map(|(_, t)| t)
            .collect();
        wtexts.sort_by(|a, b| a.y.total_cmp(&b.y));
        if wtexts.is_empty() {
            return None;
        }
        let title = wtexts[0].text.clone();
        let body = wtexts
            .iter()
            .skip(1)
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Some(WarningBannerSpec { title, body })
    });

    Ok(DiagramSpec {
        id: id.to_string(),
        heading: heading.to_string(),
        note: note.to_string(),
        svg_title,
        canvas_title,
        subtitles,
        columns,
        edges,
        callouts,
        warning,
        footer,
    })
}

fn reconstruct_edges(
    lines: &[RawLine],
    paths: &[RawPath],
    texts: &[RawText],
    used_text: &[bool],
    card_bounds: &BatchBounds,
    card_ids: &[String],
    main_cards: &[(RawRect, Tone)],
) -> Vec<EdgeSpec> {
    let mut edges = Vec::new();
    let mut consumed_path = vec![false; paths.len()];

    // First group vertical bus spines.
    let mut bus_spines: Vec<&RawLine> = lines
        .iter()
        .filter(|l| (l.x1 - l.x2).abs() < 1.5 && (l.y2 - l.y1).abs() > 20.0)
        .collect();
    bus_spines.sort_by(|a, b| a.x1.total_cmp(&b.x1));

    for spine in &bus_spines {
        let bx = spine.x1;
        let min_y = spine.y1.min(spine.y2) - 10.0;
        let max_y = spine.y1.max(spine.y2) + 10.0;

        let mut from_ids = Vec::new();
        let mut to_ids = Vec::new();
        let mut max_from_right = 0.0_f64;
        let mut min_to_left = f64::INFINITY;

        for (pi, p) in paths.iter().enumerate() {
            if consumed_path[pi] || p.points.len() != 2 {
                continue;
            }
            let (sx, sy) = p.points[0];
            let (ex, ey) = p.points[1];
            if (ex - bx).abs() <= 3.0 && ey >= min_y && ey <= max_y {
                if let Some(ci) = card_bounds.find_containing(sx, sy, 8.0) {
                    let cid = &card_ids[ci];
                    if !from_ids.contains(cid) {
                        from_ids.push(cid.clone());
                    }
                    max_from_right = max_from_right.max(main_cards[ci].0.x + main_cards[ci].0.w);
                    consumed_path[pi] = true;
                }
            } else if (sx - bx).abs() <= 3.0 && sy >= min_y && sy <= max_y
                && let Some(ci) = card_bounds.find_containing(ex, ey, 8.0)
            {
                let cid = &card_ids[ci];
                if !to_ids.contains(cid) {
                    to_ids.push(cid.clone());
                }
                min_to_left = min_to_left.min(main_cards[ci].0.x);
                consumed_path[pi] = true;
            }
        }

        if !from_ids.is_empty() && !to_ids.is_empty() {
            // Look for a floating bus label near bx.
            let label = texts
                .iter()
                .enumerate()
                .filter(|(ti, t)| !used_text[*ti] && t.bold && (t.x - bx).abs() <= 30.0)
                .min_by(|(_, a), (_, b)| (a.x - bx).abs().total_cmp(&(b.x - bx).abs()))
                .map(|(_, t)| t.text.clone());

            let bus_slot = if min_to_left > max_from_right + 1.0 {
                let ratio = ((bx - max_from_right) / (min_to_left - max_from_right)).clamp(0.15, 0.85);
                Some((ratio * 100.0).round() / 100.0)
            } else {
                None
            };

            edges.push(EdgeSpec {
                kind: EdgeKind::Bus,
                from: from_ids,
                to: to_ids,
                label,
                bus_slot,
            });
        }
    }

    // Remaining paths → Direct / LoopLeft / BottomRoute.
    for (pi, p) in paths.iter().enumerate() {
        if consumed_path[pi] {
            continue;
        }
        let Some(&(sx, sy)) = p.points.first() else {
            continue;
        };
        let Some(&(ex, ey)) = p.points.last() else {
            continue;
        };
        let Some(src_ci) = card_bounds.find_containing(sx, sy, 8.0) else {
            continue;
        };
        let Some(dst_ci) = card_bounds.find_containing(ex, ey, 8.0) else {
            continue;
        };
        if src_ci == dst_ci {
            continue;
        }
        let max_py = p.points.iter().map(|(_, y)| *y).fold(0.0_f64, f64::max);
        let min_px = p.points.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
        let kind = if max_py > 1180.0 {
            EdgeKind::BottomRoute
        } else if p.points.len() >= 4
            && min_px < main_cards[src_ci].0.x - 4.0
            && (main_cards[src_ci].0.x - main_cards[dst_ci].0.x).abs() < 10.0
        {
            EdgeKind::LoopLeft
        } else {
            EdgeKind::Direct
        };

        edges.push(EdgeSpec {
            kind,
            from: vec![card_ids[src_ci].clone()],
            to: vec![card_ids[dst_ci].clone()],
            label: None,
            bus_slot: None,
        });
    }

    edges
}

fn find_accent_tone(rects: &[RawRect], x: f64, y: f64, h: f64) -> Tone {
    rects
        .iter()
        .find(|r| r.w <= 8.0 && (r.x - x).abs() <= 1.5 && (r.y - y).abs() <= 1.5 && (r.h - h).abs() <= 2.0)
        .map(|r| Tone::from_hex(&r.fill))
        .unwrap_or(Tone::Blue)
}

fn nearest_col(headers: &[(f64, String)], x: f64) -> usize {
    headers
        .iter()
        .enumerate()
        .min_by(|(_, (hx1, _)), (_, (hx2, _))| (hx1 - x).abs().total_cmp(&(hx2 - x).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn attr_f64(node: &roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)?.trim().parse::<f64>().ok()
}

fn parse_svg_polyline_points(d: &str) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for token in d.split(|c: char| c.is_ascii_whitespace()) {
        let trimmed = token.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        if let Some((xs, ys)) = trimmed.split_once(',')
            && let (Ok(x), Ok(y)) = (xs.parse::<f64>(), ys.parse::<f64>())
        {
            pts.push((x, y));
        }
    }
    pts
}

// ---------------------------------------------------------------------------
// SIMD-backed HTML slicing & Flow Table parsing
// ---------------------------------------------------------------------------

fn extract_first_tag_inner<'a>(html: &'a str, open_prefix: &str, close_tag: &str) -> Option<&'a str> {
    let start = memmem::find(html.as_bytes(), open_prefix.as_bytes())?;
    let after_open = html[start..].find('>')? + start + 1;
    let end = memmem::find(&html.as_bytes()[after_open..], close_tag.as_bytes())? + after_open;
    Some(&html[after_open..end])
}

fn extract_tag_outer<'a>(html: &'a str, open_prefix: &str, close_tag: &str) -> Option<&'a str> {
    let start = memmem::find(html.as_bytes(), open_prefix.as_bytes())?;
    let end = memmem::find(&html.as_bytes()[start..], close_tag.as_bytes())? + start + close_tag.len();
    Some(&html[start..end])
}

fn extract_tag_with_class(html: &str, tag: &str, class_name: &str) -> Option<String> {
    let patterns = [
        format!("<{tag} class=\"{class_name}\">"),
        format!("<{tag} class='{class_name}'>"),
    ];
    let close = format!("</{tag}>");
    for pat in &patterns {
        if let Some(start) = memmem::find(html.as_bytes(), pat.as_bytes()) {
            let content_start = start + pat.len();
            if let Some(rel_end) = memmem::find(&html.as_bytes()[content_start..], close.as_bytes()) {
                return Some(html[content_start..content_start + rel_end].to_string());
            }
        }
    }
    None
}

fn split_card_divs(html: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let needle = b"<div class=\"card\"";
    let mut pos = 0usize;
    let bytes = html.as_bytes();

    while let Some(rel) = memmem::find(&bytes[pos..], needle) {
        let start = pos + rel;
        let Some(tag_end_rel) = html[start..].find('>') else {
            break;
        };
        let open_tag = &html[start..start + tag_end_rel + 1];
        let card_id = extract_attr(open_tag, "id").unwrap_or_default();
        let body_start = start + tag_end_rel + 1;

        // Next card div or end of body.
        let body_end = memmem::find(&bytes[body_start..], needle)
            .map(|r| body_start + r)
            .or_else(|| memmem::find(&bytes[body_start..], b"</body>").map(|r| body_start + r))
            .unwrap_or(html.len());

        results.push((card_id, html[body_start..body_end].to_string()));
        pos = body_start;
    }

    results
}

fn parse_flow_table(table_html: &str) -> Vec<FlowGroupSpec> {
    let mut groups: Vec<FlowGroupSpec> = Vec::new();
    let bytes = table_html.as_bytes();
    let mut pos = 0usize;

    while let Some(tr_rel) = memmem::find(&bytes[pos..], b"<tr") {
        let tr_start = pos + tr_rel;
        let Some(tr_end_rel) = memmem::find(&bytes[tr_start..], b"</tr>") else {
            break;
        };
        let tr_slice = &table_html[tr_start..tr_start + tr_end_rel + 5];
        pos = tr_start + tr_end_rel + 5;

        if tr_slice.contains("<th") {
            continue;
        }
        if tr_slice.contains("class='grp'") || tr_slice.contains("class=\"grp\"") {
            let tone = extract_inline_hex(tr_slice, "background:")
                .map(|h| Tone::from_hex(&h))
                .unwrap_or(Tone::Blue);
            // Strip `<span class='cnt'>...</span>` before reading group name.
            let without_cnt = strip_span_with_class(tr_slice, "cnt");
            let name = strip_tags_inline(&without_cnt);
            groups.push(FlowGroupSpec {
                name,
                tone,
                rows: Vec::new(),
            });
            continue;
        }

        let tds = extract_tds(tr_slice);
        if tds.len() < 7 {
            continue;
        }
        if groups.is_empty() {
            groups.push(FlowGroupSpec {
                name: "Flows".to_string(),
                tone: Tone::Blue,
                rows: Vec::new(),
            });
        }
        let version_tone = extract_inline_hex(&tds[3], "color:")
            .map(|h| Tone::from_hex(&h))
            .unwrap_or(Tone::Muted);

        let row = FlowRowSpec {
            action: strip_tags_inline(&tds[1]),
            trigger: strip_tags_inline(&tds[2]),
            version: strip_tags_inline(&tds[3]),
            version_tone,
            entry: split_html_plain_lines(&tds[4]),
            downstream: split_html_plain_lines(&tds[5]),
            note: html_inline_to_md(&tds[6]),
        };
        if let Some(last) = groups.last_mut() {
            last.rows.push(row);
        }
    }

    groups
}

fn extract_tds(tr_html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = tr_html.as_bytes();
    let mut pos = 0usize;
    while let Some(rel) = memmem::find(&bytes[pos..], b"<td") {
        let start = pos + rel;
        let Some(gt) = tr_html[start..].find('>') else {
            break;
        };
        let inner_start = start + gt + 1;
        let Some(end_rel) = memmem::find(&bytes[inner_start..], b"</td>") else {
            break;
        };
        out.push(tr_html[inner_start..inner_start + end_rel].to_string());
        pos = inner_start + end_rel + 5;
    }
    out
}

fn strip_span_with_class(html: &str, cls: &str) -> String {
    let mut s = html.to_string();
    for pat in [format!("<span class='{cls}'>"), format!("<span class=\"{cls}\">")] {
        while let Some(idx) = s.find(&pat) {
            if let Some(end) = s[idx..].find("</span>") {
                s.replace_range(idx..idx + end + 7, "");
            } else {
                break;
            }
        }
    }
    s
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pat = format!("{attr}={quote}");
        if let Some(idx) = tag.find(&pat) {
            let start = idx + pat.len();
            if let Some(end) = tag[start..].find(quote) {
                return Some(tag[start..start + end].to_string());
            }
        }
    }
    None
}

fn extract_inline_hex(s: &str, key: &str) -> Option<String> {
    let idx = s.find(key)?;
    let rest = s[idx + key.len()..].trim_start();
    if !rest.starts_with('#') || rest.len() < 7 {
        return None;
    }
    Some(rest[..7].to_string())
}

fn split_html_lines(raw: &str) -> Vec<String> {
    raw.replace("<br/>", "<br>")
        .replace("<br />", "<br>")
        .split("<br>")
        .map(html_inline_to_md)
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_html_plain_lines(raw: &str) -> Vec<String> {
    raw.replace("<br/>", "<br>")
        .replace("<br />", "<br>")
        .split("<br>")
        .map(strip_tags_inline)
        .filter(|s| !s.is_empty())
        .collect()
}

fn html_inline_to_md(raw: &str) -> String {
    let s = raw
        .replace("<b>", "**")
        .replace("</b>", "**")
        .replace("<strong>", "**")
        .replace("</strong>", "**")
        .replace("<code>", "`")
        .replace("</code>", "`");
    strip_tags_inline(&s)
}

fn strip_tags_inline(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_entities(out.trim())
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}
