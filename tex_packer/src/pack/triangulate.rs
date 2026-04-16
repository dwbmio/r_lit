/// Triangulate a simple polygon using the earcut algorithm.
/// Returns triangle indices into the input vertex array.
pub fn triangulate(vertices: &[(f32, f32)]) -> Vec<[usize; 3]> {
    if vertices.len() < 3 {
        return vec![];
    }

    // earcut 0.4 expects Iterator<Item = [f64; 2]>
    let coords: Vec<[f64; 2]> = vertices
        .iter()
        .map(|&(x, y)| [x as f64, y as f64])
        .collect();

    let hole_indices: Vec<usize> = vec![];

    let mut triangulator = earcut::Earcut::new();
    let mut indices: Vec<usize> = Vec::new();
    triangulator.earcut(coords.iter().copied(), &hole_indices, &mut indices);

    if indices.is_empty() || indices.len() % 3 != 0 {
        // Fallback: simple fan triangulation
        let mut tris = Vec::new();
        for i in 1..vertices.len() - 1 {
            tris.push([0, i, i + 1]);
        }
        return tris;
    }

    indices
        .chunks(3)
        .map(|tri| [tri[0], tri[1], tri[2]])
        .collect()
}
