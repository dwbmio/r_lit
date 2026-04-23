//! Grid → triangle-list geometry.
//!
//! Pure, headless builder. Consumers:
//!
//! * The GUI's `preview_mesh` module turns each bucket into a Bevy
//!   `Mesh` entity wired to the toon material + `OutlineVolume`.
//! * The exporter (`crate::export`) reads the buckets as-is and
//!   packages them into glTF primitives.
//!
//! ## Algorithms available
//!
//! Two builders live side-by-side:
//!
//! * [`build_color_buckets`] — **greedy rectangle meshing** (v0.6,
//!   default). For each of the 6 axis-aligned face directions, walks
//!   the slab planes between voxels; builds a per-slab 2D mask of
//!   visible face colors; collapses coplanar same-color cells into
//!   one rectangle; emits the rectangle as a single quad. Produces
//!   the *same* exterior surface as the culled mesher but with far
//!   fewer triangles — often an order of magnitude fewer on realistic
//!   shapes.
//! * [`build_color_buckets_culled`] — per-face culled mesher (v0.4).
//!   One unit quad per visible voxel face. Retained because it's the
//!   regression oracle: surface *area* must agree with the greedy
//!   output, and its triangle count is a well-understood upper bound.
//!   Not used by any shipping code path; covered by tests only.
//!
//! Both paths share the same per-color bucketing — each palette color
//! actually painted produces one [`MeshBuilder`], which maps 1:1 to a
//! glTF primitive on export and to a separate preview entity in the
//! GUI.
//!
//! ## Why per-color means "no cross-color merging"
//!
//! The greedy merger only collapses same-color rectangles. Different
//! colors stay in separate buckets because they need separate
//! materials on both the GUI (one per-color `ToonMaterial`) and the
//! exporter (one glTF primitive per palette index). This is a
//! deliberate tradeoff: a two-tone checkerboard keeps every voxel as
//! its own quad instead of merging into alternating strips, but the
//! render / export model stays dead simple.

use crate::grid::{Cell, Grid, CELL_SIZE};

/// Default builder — greedy rectangle meshing. Use this for all
/// shipping code paths; the surface it produces is equivalent to
/// [`build_color_buckets_culled`] but uses far fewer quads.
pub fn build_color_buckets(grid: &Grid) -> Vec<(u8, MeshBuilder)> {
    let mut buckets: Vec<Option<MeshBuilder>> = Vec::new();
    let max_y = max_column_height(grid);
    if max_y == 0 {
        return Vec::new();
    }

    emit_x_slabs(grid, max_y, &mut buckets);
    emit_y_slabs(grid, max_y, &mut buckets);
    emit_z_slabs(grid, max_y, &mut buckets);

    collect(buckets)
}

/// Regression oracle: one unit quad per visible voxel face, no
/// rectangle merging. Same surface, many more quads. Retained for
/// tests — `build_color_buckets` (greedy) is the shipping path.
pub fn build_color_buckets_culled(grid: &Grid) -> Vec<(u8, MeshBuilder)> {
    let mut buckets: Vec<Option<MeshBuilder>> = Vec::new();

    for z in 0..grid.h {
        for x in 0..grid.w {
            let Some(cell) = grid.get(x, z) else {
                continue;
            };
            let Some(ci) = cell.color_idx else { continue };
            let h = height_of(cell) as i32;

            for y in 0..h {
                for face in 0..6u8 {
                    if neighbor_filled(grid, x as i32, y, z as i32, face) {
                        continue;
                    }
                    ensure_bucket(&mut buckets, ci as usize).push_unit_face(x, y as u32, z, face);
                }
            }
        }
    }

    collect(buckets)
}

fn collect(buckets: Vec<Option<MeshBuilder>>) -> Vec<(u8, MeshBuilder)> {
    let mut out = Vec::new();
    for (i, slot) in buckets.into_iter().enumerate() {
        if let Some(b) = slot {
            out.push((i as u8, b));
        }
    }
    out
}

fn ensure_bucket(buckets: &mut Vec<Option<MeshBuilder>>, idx: usize) -> &mut MeshBuilder {
    if buckets.len() <= idx {
        buckets.resize_with(idx + 1, || None);
    }
    buckets[idx].get_or_insert_with(MeshBuilder::default)
}

/// Compatibility: v1 project files (pre-height-UI) wrote `height: 0`.
/// Treat those as a 1-cell-tall column instead of rendering nothing.
fn height_of(cell: &Cell) -> u8 {
    if cell.height == 0 {
        1
    } else {
        cell.height
    }
}

fn max_column_height(grid: &Grid) -> usize {
    let mut m = 0;
    for cell in &grid.cells {
        if cell.color_idx.is_some() {
            let h = height_of(cell) as usize;
            if h > m {
                m = h;
            }
        }
    }
    m
}

/// Color of the voxel at `(x, y, z)` if filled; `None` otherwise (also
/// out-of-bounds). This is the single source of truth both meshers
/// consult — they only differ in how they *group* the faces, not in
/// which faces exist.
fn voxel_color(grid: &Grid, x: i64, y: i64, z: i64) -> Option<u8> {
    if x < 0 || y < 0 || z < 0 {
        return None;
    }
    if x as usize >= grid.w || z as usize >= grid.h {
        return None;
    }
    let cell = grid.get(x as usize, z as usize)?;
    let ci = cell.color_idx?;
    if (y as usize) >= height_of(cell) as usize {
        return None;
    }
    Some(ci)
}

// ---------------------------------------------------------------------
// Culled-mesher internals (kept so the regression oracle still works).
// ---------------------------------------------------------------------

fn neighbor_filled(grid: &Grid, x: i32, y: i32, z: i32, face: u8) -> bool {
    let (dx, dy, dz) = FACE_DIRS[face as usize];
    let nx = x + dx;
    let nz = z + dz;
    let ny = y + dy;
    if ny < 0 || nx < 0 || nz < 0 {
        return false;
    }
    let (nxu, nzu) = (nx as usize, nz as usize);
    if nxu >= grid.w || nzu >= grid.h {
        return false;
    }
    let Some(cell) = grid.get(nxu, nzu) else {
        return false;
    };
    if cell.color_idx.is_none() {
        return false;
    }
    (ny as u8) < height_of(cell)
}

const FACE_DIRS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

const FACE_NORMALS: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// Unit-voxel quad corners in [0, 1]³, CCW from outside → outward-facing
/// normals under the default triangulation `[0,1,2,0,2,3]`.
const FACE_QUADS: [[[f32; 3]; 4]; 6] = [
    // +X
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ],
    // -X
    [
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    ],
    // +Y (top)
    [
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ],
    // -Y (bottom)
    [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    // +Z
    [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    // -Z
    [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ],
];

// ---------------------------------------------------------------------
// Greedy mesher internals.
// ---------------------------------------------------------------------

/// Rectangle produced by the 2D greedy merger, in plane-local
/// coordinates. The caller interprets (u, v) according to its axis.
#[derive(Debug, Clone, Copy)]
struct Rect {
    color: u8,
    u0: usize,
    v0: usize,
    u1: usize,
    v1: usize,
}

/// Classic 2D greedy rectangle meshing on a `u_dim × v_dim` mask of
/// optional color indices. Returns maximal rectangles; the mask is
/// read-only so the caller can reuse it if needed.
fn greedy_merge_mask(u_dim: usize, v_dim: usize, mask: &[Option<u8>]) -> Vec<Rect> {
    let mut merged = vec![false; u_dim * v_dim];
    let mut out = Vec::new();
    for u in 0..u_dim {
        for v in 0..v_dim {
            let idx = u * v_dim + v;
            if merged[idx] {
                continue;
            }
            let Some(color) = mask[idx] else { continue };

            // Extend along +v as far as color matches and cells are free.
            let mut v1 = v + 1;
            while v1 < v_dim {
                let i2 = u * v_dim + v1;
                if merged[i2] || mask[i2] != Some(color) {
                    break;
                }
                v1 += 1;
            }

            // Extend along +u: the whole v-range [v..v1) must match.
            let mut u1 = u + 1;
            'outer: while u1 < u_dim {
                for vv in v..v1 {
                    let i2 = u1 * v_dim + vv;
                    if merged[i2] || mask[i2] != Some(color) {
                        break 'outer;
                    }
                }
                u1 += 1;
            }

            for uu in u..u1 {
                for vv in v..v1 {
                    merged[uu * v_dim + vv] = true;
                }
            }
            out.push(Rect {
                color,
                u0: u,
                v0: v,
                u1,
                v1,
            });
        }
    }
    out
}

/// +X / -X slab planes. For each `x_slab` in `0..=w` we build two
/// masks over (z, y): one for `+X`-facing faces (voxel on -X side
/// filled, +X side empty) and one for `-X`-facing faces (mirrored).
fn emit_x_slabs(grid: &Grid, max_y: usize, buckets: &mut Vec<Option<MeshBuilder>>) {
    let w = grid.w as i64;
    let d = grid.h;
    if d == 0 {
        return;
    }
    for x_slab in 0..=w {
        let mut plus = vec![None; d * max_y];
        let mut minus = vec![None; d * max_y];
        for z in 0..d {
            for y in 0..max_y {
                let behind = voxel_color(grid, x_slab - 1, y as i64, z as i64);
                let ahead = voxel_color(grid, x_slab, y as i64, z as i64);
                let idx = z * max_y + y;
                if let Some(c) = behind {
                    if ahead.is_none() {
                        plus[idx] = Some(c);
                    }
                }
                if let Some(c) = ahead {
                    if behind.is_none() {
                        minus[idx] = Some(c);
                    }
                }
            }
        }
        let x_plane = x_slab as f32 * CELL_SIZE;
        for r in greedy_merge_mask(d, max_y, &plus) {
            let (z0, z1) = (r.u0 as f32 * CELL_SIZE, r.u1 as f32 * CELL_SIZE);
            let (y0, y1) = (r.v0 as f32 * CELL_SIZE, r.v1 as f32 * CELL_SIZE);
            ensure_bucket(buckets, r.color as usize).push_quad(
                [
                    [x_plane, y0, z0],
                    [x_plane, y1, z0],
                    [x_plane, y1, z1],
                    [x_plane, y0, z1],
                ],
                [1.0, 0.0, 0.0],
            );
        }
        for r in greedy_merge_mask(d, max_y, &minus) {
            let (z0, z1) = (r.u0 as f32 * CELL_SIZE, r.u1 as f32 * CELL_SIZE);
            let (y0, y1) = (r.v0 as f32 * CELL_SIZE, r.v1 as f32 * CELL_SIZE);
            ensure_bucket(buckets, r.color as usize).push_quad(
                [
                    [x_plane, y0, z1],
                    [x_plane, y1, z1],
                    [x_plane, y1, z0],
                    [x_plane, y0, z0],
                ],
                [-1.0, 0.0, 0.0],
            );
        }
    }
}

/// +Y / -Y slab planes. Slab count is `max_y + 1` (inclusive). Plane
/// axes are (x, z).
fn emit_y_slabs(grid: &Grid, max_y: usize, buckets: &mut Vec<Option<MeshBuilder>>) {
    let w = grid.w;
    let d = grid.h;
    if w == 0 || d == 0 {
        return;
    }
    for y_slab in 0..=max_y as i64 {
        let mut plus = vec![None; w * d];
        let mut minus = vec![None; w * d];
        for x in 0..w {
            for z in 0..d {
                let behind = voxel_color(grid, x as i64, y_slab - 1, z as i64);
                let ahead = voxel_color(grid, x as i64, y_slab, z as i64);
                let idx = x * d + z;
                if let Some(c) = behind {
                    if ahead.is_none() {
                        plus[idx] = Some(c);
                    }
                }
                if let Some(c) = ahead {
                    if behind.is_none() {
                        minus[idx] = Some(c);
                    }
                }
            }
        }
        let y_plane = y_slab as f32 * CELL_SIZE;
        for r in greedy_merge_mask(w, d, &plus) {
            let (x0, x1) = (r.u0 as f32 * CELL_SIZE, r.u1 as f32 * CELL_SIZE);
            let (z0, z1) = (r.v0 as f32 * CELL_SIZE, r.v1 as f32 * CELL_SIZE);
            ensure_bucket(buckets, r.color as usize).push_quad(
                [
                    [x0, y_plane, z0],
                    [x0, y_plane, z1],
                    [x1, y_plane, z1],
                    [x1, y_plane, z0],
                ],
                [0.0, 1.0, 0.0],
            );
        }
        for r in greedy_merge_mask(w, d, &minus) {
            let (x0, x1) = (r.u0 as f32 * CELL_SIZE, r.u1 as f32 * CELL_SIZE);
            let (z0, z1) = (r.v0 as f32 * CELL_SIZE, r.v1 as f32 * CELL_SIZE);
            ensure_bucket(buckets, r.color as usize).push_quad(
                [
                    [x0, y_plane, z0],
                    [x1, y_plane, z0],
                    [x1, y_plane, z1],
                    [x0, y_plane, z1],
                ],
                [0.0, -1.0, 0.0],
            );
        }
    }
}

/// +Z / -Z slab planes. Plane axes are (x, y).
fn emit_z_slabs(grid: &Grid, max_y: usize, buckets: &mut Vec<Option<MeshBuilder>>) {
    let w = grid.w;
    let d = grid.h as i64;
    if w == 0 {
        return;
    }
    for z_slab in 0..=d {
        let mut plus = vec![None; w * max_y];
        let mut minus = vec![None; w * max_y];
        for x in 0..w {
            for y in 0..max_y {
                let behind = voxel_color(grid, x as i64, y as i64, z_slab - 1);
                let ahead = voxel_color(grid, x as i64, y as i64, z_slab);
                let idx = x * max_y + y;
                if let Some(c) = behind {
                    if ahead.is_none() {
                        plus[idx] = Some(c);
                    }
                }
                if let Some(c) = ahead {
                    if behind.is_none() {
                        minus[idx] = Some(c);
                    }
                }
            }
        }
        let z_plane = z_slab as f32 * CELL_SIZE;
        for r in greedy_merge_mask(w, max_y, &plus) {
            let (x0, x1) = (r.u0 as f32 * CELL_SIZE, r.u1 as f32 * CELL_SIZE);
            let (y0, y1) = (r.v0 as f32 * CELL_SIZE, r.v1 as f32 * CELL_SIZE);
            ensure_bucket(buckets, r.color as usize).push_quad(
                [
                    [x1, y0, z_plane],
                    [x1, y1, z_plane],
                    [x0, y1, z_plane],
                    [x0, y0, z_plane],
                ],
                [0.0, 0.0, 1.0],
            );
        }
        for r in greedy_merge_mask(w, max_y, &minus) {
            let (x0, x1) = (r.u0 as f32 * CELL_SIZE, r.u1 as f32 * CELL_SIZE);
            let (y0, y1) = (r.v0 as f32 * CELL_SIZE, r.v1 as f32 * CELL_SIZE);
            ensure_bucket(buckets, r.color as usize).push_quad(
                [
                    [x0, y0, z_plane],
                    [x0, y1, z_plane],
                    [x1, y1, z_plane],
                    [x1, y0, z_plane],
                ],
                [0.0, 0.0, -1.0],
            );
        }
    }
}

/// Raw, un-rendered geometry for one bucket. Public so the exporter
/// can read out position / normal / index arrays without going
/// through the Bevy `Mesh` asset (which is designed for GPU upload,
/// not file I/O).
#[derive(Default)]
pub struct MeshBuilder {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl MeshBuilder {
    /// Shift every vertex by `(ox, oy, oz)`. Used by the exporter
    /// after it has collected bounds, so the mesh ships centred on
    /// its own coordinate-system origin.
    pub fn translate(&mut self, ox: f32, oy: f32, oz: f32) {
        for p in &mut self.positions {
            p[0] += ox;
            p[1] += oy;
            p[2] += oz;
        }
    }

    /// Unit-voxel face push (culled mesher only). Kept as a named
    /// helper so the regression oracle reads naturally.
    fn push_unit_face(&mut self, x: usize, y: u32, z: usize, face: u8) {
        let normal = FACE_NORMALS[face as usize];
        let quad = &FACE_QUADS[face as usize];
        let fx = x as f32 * CELL_SIZE;
        let fy = y as f32 * CELL_SIZE;
        let fz = z as f32 * CELL_SIZE;
        let corners = [
            [
                fx + quad[0][0] * CELL_SIZE,
                fy + quad[0][1] * CELL_SIZE,
                fz + quad[0][2] * CELL_SIZE,
            ],
            [
                fx + quad[1][0] * CELL_SIZE,
                fy + quad[1][1] * CELL_SIZE,
                fz + quad[1][2] * CELL_SIZE,
            ],
            [
                fx + quad[2][0] * CELL_SIZE,
                fy + quad[2][1] * CELL_SIZE,
                fz + quad[2][2] * CELL_SIZE,
            ],
            [
                fx + quad[3][0] * CELL_SIZE,
                fy + quad[3][1] * CELL_SIZE,
                fz + quad[3][2] * CELL_SIZE,
            ],
        ];
        self.push_quad(corners, normal);
    }

    /// Push a single planar quad in CCW order (matches the shared
    /// triangulation `[0,1,2,0,2,3]`). Used directly by the greedy
    /// mesher; the culled mesher routes through `push_unit_face`.
    fn push_quad(&mut self, corners: [[f32; 3]; 4], normal: [f32; 3]) {
        let base = self.positions.len() as u32;
        for p in corners {
            self.positions.push(p);
            self.normals.push(normal);
            // UV is unused by the toon shader; emit zeros so exporters
            // don't have to special-case the absence of the attribute.
            self.uvs.push([0.0, 0.0]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Apply the world-space centre offset (ox, oz) the preview uses
    /// and consume `self`. The GUI consumes this value and wraps it
    /// into a Bevy `Mesh`; the exporter calls [`Self::translate`]
    /// directly with its own bounds-derived offset instead.
    pub fn with_world_origin(mut self, ox: f32, oz: f32) -> Self {
        for p in &mut self.positions {
            p[0] += ox;
            p[2] += oz;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(grid: &mut Grid, x: usize, z: usize, color: u8, height: u8) {
        grid.paint(x, z, color, height);
    }

    // Each MeshBuilder emits 4 vertices per quad, 6 indices per quad.
    fn quads(b: &MeshBuilder) -> usize {
        b.indices.len() / 6
    }

    fn quad_area_sum(b: &MeshBuilder) -> f32 {
        let n_quads = quads(b);
        let mut area = 0.0;
        for q in 0..n_quads {
            let base = q * 4;
            let a = b.positions[base];
            let bb = b.positions[base + 1];
            let d = b.positions[base + 3];
            let ab = [bb[0] - a[0], bb[1] - a[1], bb[2] - a[2]];
            let ad = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            let cross = [
                ab[1] * ad[2] - ab[2] * ad[1],
                ab[2] * ad[0] - ab[0] * ad[2],
                ab[0] * ad[1] - ab[1] * ad[0],
            ];
            area += (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        }
        area
    }

    fn total_quads(buckets: &[(u8, MeshBuilder)]) -> usize {
        buckets.iter().map(|(_, b)| quads(b)).sum()
    }

    fn total_area(buckets: &[(u8, MeshBuilder)]) -> f32 {
        buckets.iter().map(|(_, b)| quad_area_sum(b)).sum()
    }

    // ------------------ culled mesher (regression oracle) --------------

    #[test]
    fn culled_single_cell_emits_six_faces() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 1);
        let buckets = build_color_buckets_culled(&grid);
        assert_eq!(buckets.len(), 1);
        let (ci, b) = &buckets[0];
        assert_eq!(*ci, 0);
        assert_eq!(b.positions.len(), 24);
        assert_eq!(b.indices.len(), 36);
    }

    #[test]
    fn culled_two_adjacent_same_color_cells_cull_interior_faces() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 1);
        paint(&mut grid, 1, 0, 0, 1);
        let buckets = build_color_buckets_culled(&grid);
        let (_, b) = &buckets[0];
        assert_eq!(quads(b), 10);
    }

    #[test]
    fn culled_two_adjacent_different_color_cells_cull_shared_face() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 1);
        paint(&mut grid, 1, 0, 1, 1);
        let buckets = build_color_buckets_culled(&grid);
        assert_eq!(buckets.len(), 2);
        assert_eq!(total_quads(&buckets), 10);
    }

    #[test]
    fn culled_height_extrusion_culls_vertical_stack_interior() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 3);
        let buckets = build_color_buckets_culled(&grid);
        let (_, b) = &buckets[0];
        // 4 side faces × 3 layers + top + bottom = 14.
        assert_eq!(quads(b), 14);
    }

    // ------------------ greedy mesher (shipping path) ------------------

    #[test]
    fn greedy_single_cell_matches_culled() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 1);
        let greedy = build_color_buckets(&grid);
        let culled = build_color_buckets_culled(&grid);
        // Single cell has 6 unit faces, nothing to merge.
        assert_eq!(total_quads(&greedy), 6);
        assert!((total_area(&greedy) - total_area(&culled)).abs() < 1e-3);
    }

    #[test]
    fn greedy_two_adjacent_same_color_merges_top_and_bottom() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 1);
        paint(&mut grid, 1, 0, 0, 1);
        let greedy = build_color_buckets(&grid);
        // Top +Y and bottom -Y each merge into a single 2×1 rectangle.
        // Sides: +X (cell 1), -X (cell 0) stay; +Z, -Z are 2×1
        // rectangles. Total = 1 + 1 + 1 + 1 + 1 + 1 = 6 quads.
        assert_eq!(total_quads(&greedy), 6);
        // Surface area is unchanged — greedy just groups quads.
        let culled = build_color_buckets_culled(&grid);
        assert!((total_area(&greedy) - total_area(&culled)).abs() < 1e-3);
    }

    #[test]
    fn greedy_height_3_stack_merges_sides() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 3);
        let greedy = build_color_buckets(&grid);
        // Each of the 4 sides merges into a single 1×3 rectangle; top
        // and bottom each a single 1×1. Total = 6 quads.
        assert_eq!(total_quads(&greedy), 6);
        let culled = build_color_buckets_culled(&grid);
        assert!((total_area(&greedy) - total_area(&culled)).abs() < 1e-3);
    }

    #[test]
    fn greedy_two_different_colors_do_not_merge() {
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, 1);
        paint(&mut grid, 1, 0, 1, 1);
        let greedy = build_color_buckets(&grid);
        // Different colors → separate buckets → greedy cannot merge
        // across color boundaries. Each cell contributes 5 exterior
        // faces (one internal face culled on both sides).
        assert_eq!(greedy.len(), 2);
        assert_eq!(total_quads(&greedy), 10);
        let culled = build_color_buckets_culled(&grid);
        assert!((total_area(&greedy) - total_area(&culled)).abs() < 1e-3);
    }

    #[test]
    fn greedy_l_shape_surface_area_matches_culled_with_fewer_quads() {
        // L-shape: three cells horizontally + two more stacked at the
        // corner. Picks up merging in both x and z planes.
        let mut grid = Grid::with_size(8, 8);
        paint(&mut grid, 0, 0, 0, 1);
        paint(&mut grid, 1, 0, 0, 1);
        paint(&mut grid, 2, 0, 0, 1);
        paint(&mut grid, 2, 1, 0, 1);
        paint(&mut grid, 2, 2, 0, 1);
        let greedy = build_color_buckets(&grid);
        let culled = build_color_buckets_culled(&grid);
        assert!((total_area(&greedy) - total_area(&culled)).abs() < 1e-3);
        assert!(
            total_quads(&greedy) < total_quads(&culled),
            "greedy should strictly reduce quad count on an L-shape: greedy={} culled={}",
            total_quads(&greedy),
            total_quads(&culled)
        );
    }

    #[test]
    fn greedy_flat_slab_collapses_to_two_quads_per_face_direction() {
        // 4×4 of color 0, height 1. The top and bottom should each
        // collapse to a single 4×4 quad. Each of the 4 sides is a 4×1
        // quad. Total = 6 quads.
        let mut grid = Grid::with_size(4, 4);
        for x in 0..4 {
            for z in 0..4 {
                paint(&mut grid, x, z, 0, 1);
            }
        }
        let greedy = build_color_buckets(&grid);
        assert_eq!(total_quads(&greedy), 6);
        let culled = build_color_buckets_culled(&grid);
        assert!((total_area(&greedy) - total_area(&culled)).abs() < 1e-3);
        // Large reduction: culled has 16 top + 16 bottom + 16 sides = 48.
        assert!(total_quads(&greedy) * 6 <= total_quads(&culled));
    }

    #[test]
    fn greedy_empty_grid_produces_no_buckets() {
        let grid = Grid::with_size(4, 4);
        assert!(build_color_buckets(&grid).is_empty());
    }

    #[test]
    fn greedy_handles_max_height_column() {
        use crate::grid::MAX_HEIGHT;
        let mut grid = Grid::with_size(4, 4);
        paint(&mut grid, 0, 0, 0, MAX_HEIGHT);
        let greedy = build_color_buckets(&grid);
        // Max-height column: 4 sides (each 1×MAX_HEIGHT) + top + bottom.
        assert_eq!(total_quads(&greedy), 6);
    }
}
