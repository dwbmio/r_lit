use image::RgbaImage;

/// Extract the outer contour polygon from a sprite's alpha channel.
/// Uses Moore neighbor tracing on the binary alpha mask.
/// Returns vertices in pixel coordinates relative to the image.
pub fn extract_contour(img: &RgbaImage, alpha_threshold: u8) -> Vec<(f32, f32)> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return vec![];
    }

    let w = w as i32;
    let h = h as i32;

    let opaque = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= w || y >= h {
            return false;
        }
        img.get_pixel(x as u32, y as u32)[3] > alpha_threshold
    };

    // Find start pixel: first opaque pixel scanning top-to-bottom, left-to-right
    let mut start = None;
    'outer: for y in 0..h {
        for x in 0..w {
            if opaque(x, y) {
                start = Some((x, y));
                break 'outer;
            }
        }
    }

    let (sx, sy) = match start {
        Some(s) => s,
        None => {
            // Fully transparent
            return vec![(0.0, 0.0), (w as f32, 0.0), (w as f32, h as f32), (0.0, h as f32)];
        }
    };

    // Moore neighbor tracing
    // 8-connected neighbors in clockwise order starting from left
    let neighbors: [(i32, i32); 8] = [
        (-1, 0),  // 0: left
        (-1, -1), // 1: top-left
        (0, -1),  // 2: top
        (1, -1),  // 3: top-right
        (1, 0),   // 4: right
        (1, 1),   // 5: bottom-right
        (0, 1),   // 6: bottom
        (-1, 1),  // 7: bottom-left
    ];

    let mut boundary: Vec<(i32, i32)> = Vec::new();
    let mut cx = sx;
    let mut cy = sy;
    // Start direction: came from left, so backtrack_dir = 0 (left neighbor)
    let mut backtrack_dir: usize = 0;

    let max_iters = (w * h * 4) as usize; // safety limit

    for _ in 0..max_iters {
        boundary.push((cx, cy));

        // Start searching from (backtrack_dir + 1) % 8 in clockwise order
        let start_dir = (backtrack_dir + 1) % 8;
        let mut found = false;

        for step in 0..8 {
            let dir = (start_dir + step) % 8;
            let (dx, dy) = neighbors[dir];
            let nx = cx + dx;
            let ny = cy + dy;

            if opaque(nx, ny) {
                // backtrack direction = opposite of the direction we came from
                backtrack_dir = (dir + 4) % 8;
                cx = nx;
                cy = ny;
                found = true;
                break;
            }
        }

        if !found || (cx == sx && cy == sy) {
            break;
        }
    }

    // Remove consecutive duplicates
    boundary.dedup();

    if boundary.len() < 3 {
        return vec![
            (0.0, 0.0),
            (w as f32, 0.0),
            (w as f32, h as f32),
            (0.0, h as f32),
        ];
    }

    // Convert to f32, shift to pixel centers (+0.5)
    boundary
        .iter()
        .map(|&(x, y)| (x as f32 + 0.5, y as f32 + 0.5))
        .collect()
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
