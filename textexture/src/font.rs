use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct FontInfo {
    pub family: String,
    pub style: String,
}

/// List available system fonts, optionally filtered by search query.
pub fn list_fonts(search: Option<&str>) -> Vec<FontInfo> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut results: Vec<FontInfo> = db
        .faces()
        .filter_map(|face| {
            let family = face.families.first()?.0.clone();
            let style = format!("{:?}", face.style);
            if let Some(q) = search {
                let q_lower = q.to_lowercase();
                if !family.to_lowercase().contains(&q_lower) {
                    return None;
                }
            }
            Some(FontInfo { family, style })
        })
        .collect();

    results.sort_by(|a, b| a.family.cmp(&b.family).then(a.style.cmp(&b.style)));
    results.dedup_by(|a, b| a.family == b.family && a.style == b.style);
    results
}

/// Resolve a font family name for cosmic-text. Returns the family name string.
/// If `font_spec` is a file path, loads it into the database and returns its family name.
pub fn resolve_font_family(
    font_system: &mut cosmic_text::FontSystem,
    font_spec: Option<&str>,
) -> String {
    match font_spec {
        Some(spec) if std::path::Path::new(spec).exists() => {
            // Load font file into the font system's database
            font_system.db_mut().load_font_file(spec).ok();
            // Try to extract family name from the loaded font
            if let Ok(data) = std::fs::read(spec) {
                if let Some(face) = fontdb::Database::new()
                    .faces()
                    .next()
                {
                    // Fallback: just try loading and use what we get
                    let _ = face;
                }
                // Parse family from font data directly
                if let Some(face) = ttf_parser_family(&data) {
                    return face;
                }
            }
            "sans-serif".to_string()
        }
        Some(name) => name.to_string(),
        None => "sans-serif".to_string(),
    }
}

fn ttf_parser_family(data: &[u8]) -> Option<String> {
    let mut db = fontdb::Database::new();
    db.load_font_data(data.to_vec());
    let result = db.faces().next().and_then(|f| f.families.first().map(|p| p.0.clone()));
    result
}
