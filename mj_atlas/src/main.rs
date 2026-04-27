mod cmd;
mod error;
mod output;
mod pack;
#[cfg(feature = "gui")]
mod preview;

use clap::{Parser, Subcommand, ValueEnum};
use pack::PolygonShape;
use error::Result;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mj_atlas",
    version,
    author = "mj_atlas contributors",
    about = "MJAtlas — game-ready texture atlas packer (pack sprites into optimized atlases with metadata for game engines)",
    long_about = "mj_atlas packs sprite images into optimized texture atlases.\n\n\
        Core features:\n  \
        - MaxRects bin packing (crunch engine) with rotation support\n  \
        - Transparent pixel trimming with configurable threshold\n  \
        - Edge extrusion to prevent texture bleeding\n  \
        - Duplicate sprite detection (SHA256 + fast pre-rejection)\n  \
        - Polygon mesh output (contour → simplify → earcut triangulation)\n  \
        - PNG quantization (imagequant, ~60-70% file size reduction)\n  \
        - Multi-atlas auto-split when sprites exceed max size\n  \
        - Animation sequence auto-detection from naming patterns\n  \
        - Parallel processing via rayon (multi-core loading & preprocessing)\n\n\
        Output formats:\n  \
        - json: TexturePacker JSON Hash (universal, default)\n  \
        - json-array: TexturePacker JSON Array (universal)\n  \
        - godot-tpsheet: Godot .tpsheet (TexturePacker Godot plugin)\n  \
        - godot-tres: Godot native .tres AtlasTexture + SpriteFrames (zero plugin)\n\n\
        Examples:\n  \
        mj_atlas pack ./sprites -o atlas --trim --pot\n  \
        mj_atlas pack ./sprites -o atlas --trim --rotate --pot --extrude 1\n  \
        mj_atlas pack ./sprites -o atlas --format godot-tres --trim --pot\n  \
        mj_atlas pack ./sprites -o atlas --polygon --tolerance 1.5 --trim\n  \
        mj_atlas pack ./sprites -o atlas --quantize --quantize-quality 70 --json\n  \
        mj_atlas gui                    # interactive GUI (--features gui)\n  \
        mj_atlas preview atlas.json     # preview atlas (--features gui)\n  \
        mj_atlas formats --json         # list formats as JSON",
    after_help = "Machine-readable output:\n  \
        All subcommands support --json for structured JSON output on stdout.\n  \
        Errors output JSON on stderr: {\"status\": \"error\", \"error\": \"...\"}\n\n\
        For AI/LLM integration, see llms.txt in the project root."
)]
struct Cli {
    /// Output machine-readable JSON to stdout (works with all subcommands).
    /// Errors go to stderr as JSON: {"status": "error", "error": "..."}
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pack sprites from a directory into texture atlas(es).
    /// Reads PNG/JPG/BMP/GIF/TGA/WebP files, outputs atlas PNG + metadata.
    /// Subdirectories are scanned recursively by default.
    /// Duplicate sprites are auto-detected and deduplicated.
    /// If sprites don't fit in a single atlas, multiple atlases are created.
    #[command(
        long_about = "Pack all sprite images from INPUT_DIR into optimized texture atlas(es).\n\n\
            The packer reads all image files (PNG, JPG, BMP, GIF, TGA, WebP) from the input\n\
            directory, optionally trims transparent borders, applies extrusion, and packs them\n\
            into one or more atlas images using the crunch bin-packing algorithm.\n\n\
            Output:\n  \
            - <output>.png — the atlas image (RGBA 32-bit)\n  \
            - <output>.json / .tpsheet / .tres — sprite metadata\n\n\
            Sprite naming convention for animations:\n  \
            Files matching pattern `<name>_<number>.<ext>` (e.g. walk_01.png, walk_02.png)\n  \
            are automatically grouped into animation sequences in the metadata."
    )]
    Pack {
        /// Directory containing sprite images to pack.
        /// All PNG/JPG/BMP/GIF/TGA/WebP files are included.
        /// Subdirectories are scanned recursively by default.
        #[arg(value_name = "INPUT_DIR")]
        input: PathBuf,

        /// Base filename for output atlas (without extension).
        /// The atlas image will be <name>.png, metadata will be <name>.json (or .tpsheet/.tres).
        /// For multi-atlas output, files are suffixed: atlas.png, atlas_1.png, atlas_2.png, ...
        #[arg(short, long, value_name = "NAME", default_value = "atlas")]
        output: String,

        /// Directory where output files are written.
        /// Defaults to the same directory as INPUT_DIR.
        #[arg(short = 'd', long, value_name = "DIR")]
        output_dir: Option<PathBuf>,

        /// Maximum atlas width/height in pixels.
        /// If all sprites don't fit, the packer auto-splits into multiple atlases.
        /// Common values: 1024 (mobile), 2048 (standard), 4096 (high-end).
        #[arg(long, default_value = "4096", value_name = "PIXELS")]
        max_size: usize,

        /// Gap between sprites in the atlas, in pixels.
        /// Use 1-2 to prevent visual artifacts from bilinear filtering.
        #[arg(long, default_value = "0", value_name = "PIXELS")]
        spacing: u32,

        /// Inner padding added around each sprite, in pixels.
        /// Extends the sprite's allocated rectangle without affecting the content.
        #[arg(long, default_value = "0", value_name = "PIXELS")]
        padding: u32,

        /// Repeat edge pixels outward by N pixels to prevent texture bleeding.
        /// Essential for tiled/seamless textures. Recommended: 1-2.
        /// The extruded pixels are NOT included in the sprite's frame rectangle in metadata.
        #[arg(long, default_value = "0", value_name = "PIXELS")]
        extrude: u32,

        /// Remove transparent border pixels from each sprite before packing.
        /// Significantly reduces atlas size for sprites with large transparent areas.
        /// Original sprite dimensions are preserved in metadata (sourceSize field).
        /// The trim offset is recorded in metadata (spriteSourceSize field).
        #[arg(long)]
        trim: bool,

        /// Allow 90-degree clockwise rotation for tighter packing.
        /// When a sprite is rotated, metadata includes "rotated": true.
        /// The game engine must handle rotation when rendering.
        #[arg(long)]
        rotate: bool,

        /// Force atlas dimensions to be power-of-2 (e.g., 256, 512, 1024, 2048, 4096).
        /// Required by some older GPU hardware and certain game engines.
        #[arg(long)]
        pot: bool,

        /// Output metadata format. Determines the file extension and data structure.
        /// json = TexturePacker JSON Hash (.json) — universal, default.
        /// json-array = TexturePacker JSON Array (.json) — frame list instead of map.
        /// godot-tpsheet = Godot .tpsheet — import with TexturePacker Godot plugin.
        /// godot-tres = Godot native .tres — generates AtlasTexture + SpriteFrames, zero plugin.
        #[arg(long, value_enum, default_value = "json", value_name = "FORMAT")]
        format: OutputFormat,

        /// Scan subdirectories recursively for images.
        #[arg(long, default_value = "true")]
        recursive: bool,

        /// Enable incremental packing. Reads `<output>.manifest.json` next to
        /// the atlas and skips work for unchanged inputs:
        ///   - All inputs unchanged + matching options ⇒ skip everything (fast cache hit)
        ///   - Pure additions / in-place pixel edits ⇒ partial repack with UV stability
        ///   - Removed sprites or resized sprites ⇒ partial repack (UV-stable, no compaction)
        ///   - Anything that breaks the layout (atlas would need to grow) ⇒ full repack
        /// Sprites that did NOT change keep their exact `(x, y, rotated)` across runs,
        /// so already-deployed game code can drop in a new atlas without rebaking UVs.
        #[arg(long)]
        incremental: bool,

        /// Force a full repack even when the incremental cache would hit.
        /// Use this when you suspect the manifest is corrupt or want to verify
        /// determinism. Has no effect without --incremental.
        #[arg(long)]
        force: bool,

        /// Alpha threshold for trim. Pixels with alpha <= this value are considered transparent.
        /// 0 = only fully transparent pixels are trimmed (default).
        /// Higher values trim semi-transparent edges.
        #[arg(long, default_value = "0", value_name = "0-255")]
        trim_threshold: u8,

        /// Enable lossy PNG quantization (imagequant).
        /// Reduces atlas PNG file size by ~60-70% with minimal visual quality loss.
        /// Uses palette-based encoding (256 colors max with dithering).
        /// Note: imagequant is licensed under GPL-3.0.
        #[arg(long)]
        quantize: bool,

        /// Quality level for PNG quantization. Lower = smaller file, more artifacts.
        /// 100 = best quality. 60-85 = good balance. Below 40 = noticeable artifacts.
        #[arg(long, default_value = "85", value_name = "1-100")]
        quantize_quality: u8,

        /// Enable polygon mesh mode. For each sprite, outputs:
        /// - vertices: polygon contour in sprite-local coordinates
        /// - verticesUV: corresponding atlas UV coordinates
        /// - triangles: earcut triangulation indices
        /// Game engines can render only the non-transparent polygon instead of the full
        /// rectangle, reducing GPU overdraw by 30%+ for irregularly shaped sprites.
        #[arg(long)]
        polygon: bool,

        /// Polygon contour simplification tolerance (Douglas-Peucker algorithm).
        /// Lower = tighter fit to sprite outline, more vertices, less overdraw.
        /// Higher = coarser outline, fewer vertices, more overdraw.
        /// Recommended: 1.0 (tight) to 4.0 (coarse). Default: 2.0.
        #[arg(long, default_value = "2.0", value_name = "TOLERANCE")]
        tolerance: f32,

        /// Polygon shape model. Controls how each connected component is meshed.
        /// concave (default) — keep the simplified outline; tightest fit, most vertices.
        /// convex — replace each component with its convex hull; few vertices, may overdraw.
        /// auto — pick convex when concave-area / hull-area ≥ 0.85, else concave.
        #[arg(long, value_enum, default_value = "concave", value_name = "MODE")]
        polygon_shape: PolygonShapeArg,

        /// Maximum total vertex count per sprite (across all components).
        /// 0 (default) disables the budget — uses --tolerance as-is.
        /// >0 enables iterative tolerance escalation (×1.5 per round, max 8 rounds)
        /// until the total vertex count fits the budget. Useful for hard
        /// per-frame draw call budgets on mobile/web.
        #[arg(long, default_value = "0", value_name = "N")]
        max_vertices: u32,
    },

    /// Launch the interactive GUI application.
    /// Opens a project workspace where you can:
    /// - Drag & drop sprite images/folders
    /// - Configure packing options visually
    /// - See inline atlas preview with zoom/pan
    /// - Save/load .tpproj project files
    /// Requires: cargo build --features gui
    #[cfg(feature = "gui")]
    #[command(long_about = "Launch the mj_atlas GUI application.\n\n\
        The GUI provides a complete visual workflow:\n  \
        1. Drag & drop sprites (or File > Add Sprites)\n  \
        2. Configure packing options in the right panel\n  \
        3. Preview updates automatically (or click Pack!)\n  \
        4. Export via File > Save Project or CLI\n\n\
        Supports .tpproj project files for saving/restoring workspace state.\n\
        Requires building with: cargo build --features gui")]
    Gui,

    /// Open an existing atlas file in the interactive preview viewer.
    /// Supports TexturePacker JSON Hash (.json) and Godot .tpsheet files.
    /// Requires: cargo build --features gui
    #[cfg(feature = "gui")]
    Preview {
        /// Path to atlas metadata file (.json or .tpsheet).
        /// The atlas PNG must be in the same directory.
        #[arg(value_name = "ATLAS_FILE")]
        file: PathBuf,
    },

    /// List all supported output formats with descriptions.
    /// With --json, outputs a JSON array of format objects.
    Formats,

    /// Pretty-print or JSON-dump a packed atlas's manifest.
    /// Accepts the manifest itself, the atlas PNG, the JSON/.tpsheet/.tres
    /// sidecar, or the directory containing them — paths are auto-resolved.
    #[command(
        long_about = "Read the `<output>.manifest.json` sidecar of a packed atlas and \
            print a human-readable summary (or full JSON with --json).\n\n\
            Resolves any of these inputs:\n  \
            - atlas.manifest.json          (direct)\n  \
            - atlas.png / atlas.json       (sibling)\n  \
            - atlas_1.png (multi-bin)      (strips _N suffix)\n  \
            - the directory containing them"
    )]
    Inspect {
        /// Path to the manifest, the atlas PNG, sidecar metadata, or the directory.
        #[arg(value_name = "ATLAS_OR_MANIFEST")]
        input: PathBuf,
    },

    /// Diff two manifests — added / removed / modified / moved sprites,
    /// plus a UV-stability verdict (whether unchanged sprites kept their
    /// position across the two pack runs).
    Diff {
        /// First (older) manifest. Same path-resolution rules as `inspect`.
        #[arg(value_name = "A")]
        a: PathBuf,
        /// Second (newer) manifest.
        #[arg(value_name = "B")]
        b: PathBuf,
    },

    /// Verify that on-disk artifacts match the manifest's hashes.
    /// Always rehashes atlas PNGs. With --check-sources also rehashes every
    /// sprite source file. Exits non-zero on any mismatch.
    Verify {
        /// Path to the manifest, atlas, or its directory.
        #[arg(value_name = "ATLAS_OR_MANIFEST")]
        input: PathBuf,
        /// Also rehash sprite source files (slower but catches accidental
        /// edits to the input library that haven't been repacked yet).
        #[arg(long)]
        check_sources: bool,
    },

    /// Read or edit a sprite's user metadata (tags, attribution, source URL).
    /// Edits the manifest in place — no repack is triggered.
    /// When <SPRITE> is omitted, write ops apply to ALL sprites (use carefully).
    Tag {
        /// Path to the manifest, atlas, or its directory.
        #[arg(value_name = "ATLAS_OR_MANIFEST")]
        input: PathBuf,
        /// Sprite name (relative path key, as shown by `inspect`). Omit to
        /// target every sprite in the manifest.
        #[arg(value_name = "SPRITE")]
        sprite: Option<String>,
        /// Add tags. Comma-separated. Existing tags are preserved.
        #[arg(long, value_name = "TAGS", value_delimiter = ',')]
        add: Vec<String>,
        /// Remove specific tags. Comma-separated.
        #[arg(long, value_name = "TAGS", value_delimiter = ',')]
        remove: Vec<String>,
        /// Drop ALL tags on the targeted sprite(s).
        #[arg(long)]
        clear: bool,
        /// Set the attribution string (free-form; license / author / etc.).
        #[arg(long, value_name = "TEXT")]
        set_attribution: Option<String>,
        /// Clear the attribution string.
        #[arg(long)]
        clear_attribution: bool,
        /// Set the source URL (where this sprite came from).
        #[arg(long, value_name = "URL")]
        set_source_url: Option<String>,
        /// Clear the source URL.
        #[arg(long)]
        clear_source_url: bool,
        /// Read-only: list the current metadata without modifying anything.
        #[arg(long)]
        list: bool,
    },
}

#[derive(Clone, ValueEnum)]
enum PolygonShapeArg {
    Concave,
    Convex,
    Auto,
}

impl From<&PolygonShapeArg> for PolygonShape {
    fn from(a: &PolygonShapeArg) -> Self {
        match a {
            PolygonShapeArg::Concave => PolygonShape::Concave,
            PolygonShapeArg::Convex => PolygonShape::Convex,
            PolygonShapeArg::Auto => PolygonShape::Auto,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    /// TexturePacker JSON Hash — frames as key-value map. Universal format, widest engine support.
    Json,
    /// TexturePacker JSON Array — frames as ordered array. Same data, different structure.
    JsonArray,
    /// Godot .tpsheet — JSON format for TexturePacker Godot plugin. Auto-generates .tres on import.
    GodotTpsheet,
    /// Godot native .tres — generates AtlasTexture + SpriteFrames resources. Zero plugin needed.
    GodotTres,
}

fn init_logger() {
    let _ = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                message
            ))
        })
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .chain(std::io::stdout())
        .apply();
}

#[tokio::main]
async fn main() {
    init_logger();
    let cli = Cli::parse();

    let result = run(&cli);

    if let Err(e) = result {
        if cli.json {
            eprintln!(
                "{}",
                serde_json::json!({"status": "error", "error": e.to_string()})
            );
        } else {
            log::error!("{}", e);
        }
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Commands::Pack {
            input,
            output,
            output_dir,
            max_size,
            spacing,
            padding,
            extrude,
            trim,
            rotate,
            pot,
            format,
            recursive,
            incremental,
            trim_threshold,
            quantize,
            quantize_quality,
            polygon,
            tolerance,
            polygon_shape,
            max_vertices,
            force,
        } => {
            let out_dir = output_dir.clone().unwrap_or_else(|| input.clone());

            let fmt = match format {
                OutputFormat::Json => output::Format::JsonHash,
                OutputFormat::JsonArray => output::Format::JsonArray,
                OutputFormat::GodotTpsheet => output::Format::GodotTpsheet,
                OutputFormat::GodotTres => output::Format::GodotTres,
            };

            let opts = pack::PackOptions {
                input_dir: input.clone(),
                output_name: output.clone(),
                output_dir: out_dir,
                max_size: *max_size,
                spacing: *spacing,
                padding: *padding,
                extrude: *extrude,
                trim: *trim,
                trim_threshold: *trim_threshold,
                rotate: *rotate,
                pot: *pot,
                recursive: *recursive,
                explicit_sprites: None,
                incremental: *incremental,
                force: *force,
                format: fmt,
                quantize: *quantize,
                quantize_quality: *quantize_quality,
                polygon: *polygon,
                tolerance: *tolerance,
                polygon_shape: PolygonShape::from(polygon_shape),
                max_vertices: *max_vertices,
            };

            let results = pack::execute(&opts)?;

            for atlas_result in &results {
                atlas_result.save_to_disk(&opts, fmt)?;
            }

            pack::persist_manifest(&opts, &results)?;

            if cli.json {
                let total_dups: usize = results.iter().map(|r| r.duplicates_removed).sum();
                let cached_count = results.iter().filter(|r| r.from_cache).count();
                let summary = serde_json::json!({
                    "status": "ok",
                    "atlases": results.len(),
                    "total_sprites": results.iter().map(|r| r.sprites.len()).sum::<usize>(),
                    "duplicates_removed": total_dups,
                    "cached_atlases": cached_count,
                    "skipped": cached_count == results.len() && !results.is_empty(),
                    "files": results.iter().map(|r| {
                        serde_json::json!({
                            "image": r.image_path.display().to_string(),
                            "data": r.data_path.display().to_string(),
                            "size": {"w": r.width, "h": r.height},
                            "sprites": r.sprites.len(),
                            "from_cache": r.from_cache,
                        })
                    }).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                for r in &results {
                    log::info!(
                        "Atlas: {} ({}x{}, {} sprites)",
                        r.image_path.display(),
                        r.width,
                        r.height,
                        r.sprites.len()
                    );
                }
                log::info!(
                    "Done! {} atlas(es), {} sprites total.",
                    results.len(),
                    results.iter().map(|r| r.sprites.len()).sum::<usize>()
                );
            }

            Ok(())
        }

        #[cfg(feature = "gui")]
        Commands::Gui => preview::run_gui(),

        #[cfg(feature = "gui")]
        Commands::Preview { file } => preview::run_preview(file),

        Commands::Formats => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "formats": [
                            {
                                "name": "json",
                                "cli_value": "json",
                                "extension": ".json",
                                "description": "TexturePacker JSON Hash — frames as key-value map",
                                "universal": true,
                                "godot_compatible": true,
                                "needs_plugin": false
                            },
                            {
                                "name": "json-array",
                                "cli_value": "json-array",
                                "extension": ".json",
                                "description": "TexturePacker JSON Array — frames as ordered list",
                                "universal": true,
                                "godot_compatible": true,
                                "needs_plugin": false
                            },
                            {
                                "name": "godot-tpsheet",
                                "cli_value": "godot-tpsheet",
                                "extension": ".tpsheet",
                                "description": "Godot TexturePacker plugin format",
                                "universal": false,
                                "godot_compatible": true,
                                "needs_plugin": true
                            },
                            {
                                "name": "godot-tres",
                                "cli_value": "godot-tres",
                                "extension": ".tres",
                                "description": "Godot native AtlasTexture + SpriteFrames resources",
                                "universal": false,
                                "godot_compatible": true,
                                "needs_plugin": false
                            }
                        ]
                    })
                );
            } else {
                println!("Supported output formats:\n");
                println!("  json            TexturePacker JSON Hash (default, universal)");
                println!("  json-array      TexturePacker JSON Array (universal)");
                println!("  godot-tpsheet   Godot .tpsheet (TexturePacker Godot plugin)");
                println!("  godot-tres      Godot native .tres (zero plugin, AtlasTexture + SpriteFrames)");
            }
            Ok(())
        }

        Commands::Inspect { input } => cmd::inspect::run(input, cli.json),

        Commands::Diff { a, b } => cmd::diff::run(a, b, cli.json),

        Commands::Verify {
            input,
            check_sources,
        } => cmd::verify::run(input, *check_sources, cli.json),

        Commands::Tag {
            input,
            sprite,
            add,
            remove,
            clear,
            set_attribution,
            clear_attribution,
            set_source_url,
            clear_source_url,
            list,
        } => {
            let ops = cmd::tag::TagOps {
                add: add.clone(),
                remove: remove.clone(),
                clear: *clear,
                set_attribution: set_attribution.clone(),
                clear_attribution: *clear_attribution,
                set_source_url: set_source_url.clone(),
                clear_source_url: *clear_source_url,
                list_only: *list,
            };
            cmd::tag::run(input, sprite.as_deref(), ops, cli.json)
        }
    }
}
