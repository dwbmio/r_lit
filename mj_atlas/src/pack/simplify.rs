/// Ramer-Douglas-Peucker polygon simplification algorithm.
/// Reduces the number of vertices while preserving shape within `tolerance`.
pub fn simplify_polygon(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let result = rdp(points, tolerance);

    // Ensure we have at least 3 points (a valid polygon)
    if result.len() < 3 {
        return points.to_vec();
    }

    result
}

fn rdp(points: &[(f32, f32)], epsilon: f32) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let first = points[0];
    let last = points[points.len() - 1];

    // Find the point farthest from the line segment first-last
    let mut max_dist = 0.0f32;
    let mut max_idx = 0;

    for (i, &p) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let d = perpendicular_distance(p, first, last);
        if d > max_dist {
            max_dist = d;
            max_idx = i;
        }
    }

    if max_dist > epsilon {
        // Recursively simplify both halves
        let mut left = rdp(&points[..=max_idx], epsilon);
        let right = rdp(&points[max_idx..], epsilon);
        // Remove duplicate point at junction
        left.pop();
        left.extend(right);
        left
    } else {
        // All intermediate points are within tolerance — keep only endpoints
        vec![first, last]
    }
}

fn perpendicular_distance(point: (f32, f32), line_start: (f32, f32), line_end: (f32, f32)) -> f32 {
    let dx = line_end.0 - line_start.0;
    let dy = line_end.1 - line_start.1;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-10 {
        // Line start and end are the same point
        let px = point.0 - line_start.0;
        let py = point.1 - line_start.1;
        return (px * px + py * py).sqrt();
    }

    let numerator = ((point.0 - line_start.0) * dy - (point.1 - line_start.1) * dx).abs();
    numerator / len_sq.sqrt()
}
