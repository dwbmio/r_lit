use image::RgbaImage;

/// Extract the outer contour of every connected (8-neighbor) opaque component.
///
/// Scans the alpha mask, runs flood fill to label components, then traces each
/// component's external boundary with Moore neighbor tracing. Components below
/// `min_area_pixels` are discarded as noise (e.g. anti-aliased dust pixels).
///
/// Returns one polygon (vertices in pixel coords) per component, ordered by
/// scan position so output is deterministic. Holes inside components are
/// **not** extracted — overdraw of a few interior transparent pixels is much
/// cheaper than supporting hole topology in the downstream mesh format.
pub fn extract_components(
    img: &RgbaImage,
    alpha_threshold: u8,
    min_area_pixels: u32,
) -> Vec<Vec<(f32, f32)>> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return vec![];
    }
    let w_i = w as i32;
    let h_i = h as i32;

    // Build a binary opaque-pixel mask.
    let mut opaque = vec![false; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            opaque[(y * w + x) as usize] = img.get_pixel(x, y)[3] > alpha_threshold;
        }
    }

    // Flood-fill labeling (8-connected). Each pixel gets a component id; -1 = empty.
    let mut label: Vec<i32> = vec![-1; (w * h) as usize];
    let mut next_label: i32 = 0;
    let mut areas: Vec<u32> = Vec::new();

    for sy in 0..h_i {
        for sx in 0..w_i {
            let idx = (sy * w_i + sx) as usize;
            if !opaque[idx] || label[idx] != -1 {
                continue;
            }
            // BFS flood fill
            let lbl = next_label;
            next_label += 1;
            let mut area: u32 = 0;
            let mut stack: Vec<(i32, i32)> = vec![(sx, sy)];
            while let Some((x, y)) = stack.pop() {
                if x < 0 || y < 0 || x >= w_i || y >= h_i {
                    continue;
                }
                let i = (y * w_i + x) as usize;
                if !opaque[i] || label[i] != -1 {
                    continue;
                }
                label[i] = lbl;
                area += 1;
                for (dx, dy) in [
                    (-1, 0), (1, 0), (0, -1), (0, 1),
                    (-1, -1), (1, -1), (-1, 1), (1, 1),
                ] {
                    stack.push((x + dx, y + dy));
                }
            }
            areas.push(area);
        }
    }

    if next_label == 0 {
        // Fully transparent — return a single full-rect component (preserves
        // legacy behaviour where transparent sprites still get a fallback mesh).
        return vec![vec![
            (0.0, 0.0),
            (w as f32, 0.0),
            (w as f32, h as f32),
            (0.0, h as f32),
        ]];
    }

    let mut polygons = Vec::with_capacity(next_label as usize);
    for lbl in 0..next_label {
        if areas[lbl as usize] < min_area_pixels {
            continue;
        }
        // Trace this component's external boundary.
        if let Some(poly) = trace_component(&label, w_i, h_i, lbl) {
            if poly.len() >= 3 {
                polygons.push(poly);
            }
        }
    }

    if polygons.is_empty() {
        // All components were below the noise floor — fall back to full rect.
        return vec![vec![
            (0.0, 0.0),
            (w as f32, 0.0),
            (w as f32, h as f32),
            (0.0, h as f32),
        ]];
    }

    polygons
}

/// Trace the boundary of a labeled component using Moore neighbor tracing.
fn trace_component(label: &[i32], w: i32, h: i32, target: i32) -> Option<Vec<(f32, f32)>> {
    // Find the topmost-leftmost pixel of `target`.
    let mut start: Option<(i32, i32)> = None;
    'find: for y in 0..h {
        for x in 0..w {
            if label[(y * w + x) as usize] == target {
                start = Some((x, y));
                break 'find;
            }
        }
    }
    let (sx, sy) = start?;

    let neighbors: [(i32, i32); 8] = [
        (-1, 0),  (-1, -1), (0, -1), (1, -1),
        (1, 0),   (1, 1),   (0, 1),  (-1, 1),
    ];
    let is_target = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && x < w && y < h && label[(y * w + x) as usize] == target
    };

    let mut boundary: Vec<(i32, i32)> = Vec::new();
    let mut cx = sx;
    let mut cy = sy;
    let mut backtrack_dir: usize = 0;

    let max_iters = (w * h * 4) as usize;
    for _ in 0..max_iters {
        boundary.push((cx, cy));
        let start_dir = (backtrack_dir + 1) % 8;
        let mut found = false;
        for step in 0..8 {
            let dir = (start_dir + step) % 8;
            let (dx, dy) = neighbors[dir];
            let nx = cx + dx;
            let ny = cy + dy;
            if is_target(nx, ny) {
                backtrack_dir = (dir + 4) % 8;
                cx = nx;
                cy = ny;
                found = true;
                break;
            }
        }
        if !found || (cx == sx && cy == sy && boundary.len() > 1) {
            break;
        }
    }

    boundary.dedup();
    if boundary.len() < 3 {
        return None;
    }
    Some(
        boundary
            .into_iter()
            .map(|(x, y)| (x as f32 + 0.5, y as f32 + 0.5))
            .collect(),
    )
}

/// Polygon signed area (shoelace formula). Positive = CCW, negative = CW.
pub fn polygon_area(points: &[(f32, f32)]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..points.len() {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % points.len()];
        sum += x1 * y2 - x2 * y1;
    }
    sum.abs() / 2.0
}

/// Compute the convex hull of a set of points using Graham scan.
pub fn convex_hull(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut pts: Vec<(f32, f32)> = points.to_vec();

    // Find bottom-most (then left-most) point
    let mut pivot_idx = 0;
    for (i, p) in pts.iter().enumerate() {
        if p.1 > pts[pivot_idx].1 || (p.1 == pts[pivot_idx].1 && p.0 < pts[pivot_idx].0) {
            pivot_idx = i;
        }
    }
    pts.swap(0, pivot_idx);
    let pivot = pts[0];

    pts[1..].sort_by(|a, b| {
        let angle_a = (a.1 - pivot.1).atan2(a.0 - pivot.0);
        let angle_b = (b.1 - pivot.1).atan2(b.0 - pivot.0);
        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut hull: Vec<(f32, f32)> = Vec::new();
    for &p in &pts {
        while hull.len() >= 2 {
            let a = hull[hull.len() - 2];
            let b = hull[hull.len() - 1];
            if cross(a, b, p) <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(p);
    }

    hull
}

/// Compute the minimum area oriented bounding box (OBB) of a convex polygon.
/// Returns (center_x, center_y, half_width, half_height, angle_radians).
pub fn min_area_obb(hull: &[(f32, f32)]) -> (f32, f32, f32, f32, f32) {
    if hull.len() < 3 {
        let (min_x, min_y, max_x, max_y) = bounding_aabb(hull);
        let cx = (min_x + max_x) / 2.0;
        let cy = (min_y + max_y) / 2.0;
        return (cx, cy, (max_x - min_x) / 2.0, (max_y - min_y) / 2.0, 0.0);
    }

    let mut best_area = f32::MAX;
    let mut best = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);

    let n = hull.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let edge_x = hull[j].0 - hull[i].0;
        let edge_y = hull[j].1 - hull[i].1;
        let len = (edge_x * edge_x + edge_y * edge_y).sqrt();
        if len < 1e-6 {
            continue;
        }

        let angle = edge_y.atan2(edge_x);
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let mut min_rx = f32::MAX;
        let mut min_ry = f32::MAX;
        let mut max_rx = f32::MIN;
        let mut max_ry = f32::MIN;
        for p in hull {
            let rx = p.0 * cos_a + p.1 * sin_a;
            let ry = -p.0 * sin_a + p.1 * cos_a;
            min_rx = min_rx.min(rx);
            min_ry = min_ry.min(ry);
            max_rx = max_rx.max(rx);
            max_ry = max_ry.max(ry);
        }

        let w = max_rx - min_rx;
        let h = max_ry - min_ry;
        let area = w * h;

        if area < best_area {
            best_area = area;
            let cx_rot = (min_rx + max_rx) / 2.0;
            let cy_rot = (min_ry + max_ry) / 2.0;
            let cx = cx_rot * cos_a - cy_rot * sin_a;
            let cy = cx_rot * sin_a + cy_rot * cos_a;
            best = (cx, cy, w / 2.0, h / 2.0, angle);
        }
    }

    best
}

/// Axis-aligned bounding box: (min_x, min_y, max_x, max_y).
pub fn bounding_aabb(points: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

fn cross(o: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}
