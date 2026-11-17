//! Headless CLI frontend for Maquette.
//!
//! Wraps the same headless core (`maquette::{grid, project, mesher,
//! export}`) that the GUI binary drives through menus and rfd
//! dialogs. No window, no wgpu device, no user prompts — every
//! operation is parameterised on the command line so it can be
//! invoked from `make`, a CI runner, or a game-project build hook.
//!
//! See `docs/COST_AWARENESS.md` §The Headless Invariant for
//! the rationale behind this binary's existence.
//!
//! ### Exit codes
//!
//! * `0` — success.
//! * `1` — runtime failure.
//! * `2` — usage error (clap / invalid flags).
//! * `3` — input error (missing or malformed project / palette / block id).
//! * `4` — external service error (hfrog / Rustyme request failure).

use std::fmt;
use std::path::PathBuf;

use bevy::prelude::Color;
use clap::{Parser, Subcommand, ValueEnum};
use maquette::block_meta::{
    self,
    hfrog::{HfrogConfig, HfrogProvider},
    BlockMetaError, BlockMetaProvider, LocalProvider,
};
use maquette::export::{self, ExportError, ExportFormat, ExportOptions, OutlineConfig};
use maquette::grid::{Grid, Palette};
use maquette::palette_io::{self, PaletteIoError};
use maquette::project::{self, ProjectError};
use maquette::render::{self, RenderError, RenderOptions};
use maquette::texgen::{
    self, MockProvider, ProviderError, TextureProvider, TextureRequest,
    rustyme::{RustymeConfig, RustymeProvider},
};
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io};
use serde_json::json;

static CONTRACT: Contract = Contract {
    tool: "maquette-cli",
    version: env!("CARGO_PKG_VERSION"),
    commands: &[
        CommandSpec::new("export", Effects::Pure)
            .io(&[("input", Io::InputFile), ("out", Io::OutputFile)]),
        CommandSpec::new("info", Effects::ReadOnly)
            .io(&[("input", Io::InputFile)]),
        CommandSpec::new("validate", Effects::ReadOnly)
            .io(&[("input", Io::InputFile)]),
        CommandSpec::new("render", Effects::Pure)
            .io(&[("input", Io::InputFile), ("out", Io::OutputFile)]),
        CommandSpec::new("palette export", Effects::Pure)
            .io(&[("input", Io::InputFile), ("out", Io::OutputFile)]),
        CommandSpec::new("palette import", Effects::Pure)
            .io(&[("input", Io::InputFile), ("from", Io::InputFile), ("out", Io::OutputFile)]),
        CommandSpec::new("texture gen", Effects::LocalWrite)
            .io(&[("out", Io::OutputFile)])
            .env(&["MAQUETTE_RUSTYME_REDIS_URL", "MAQUETTE_RUSTYME_MODEL"])
            .note("Uses local texgen cache; --provider rustyme dispatches to sonargrid."),
        CommandSpec::new("texture revoke", Effects::External)
            .env(&["MAQUETTE_RUSTYME_ADMIN_URL"]),
        CommandSpec::new("texture purge", Effects::External)
            .env(&["MAQUETTE_RUSTYME_ADMIN_URL"]),
        CommandSpec::new("block list", Effects::External)
            .env(&["MAQUETTE_HFROG_BASE_URL", "MAQUETTE_HFROG_RUNTIME"])
            .note("--source local or warm cache works offline; --source hfrog hits remote."),
        CommandSpec::new("block get", Effects::External)
            .env(&["MAQUETTE_HFROG_BASE_URL", "MAQUETTE_HFROG_RUNTIME"]),
        CommandSpec::new("block sync", Effects::External)
            .env(&["MAQUETTE_HFROG_BASE_URL", "MAQUETTE_HFROG_RUNTIME"]),
    ],
};

#[derive(Parser, Debug)]
#[command(
    name = "maquette-cli",
    version,
    about = "Headless frontend for the Maquette low-poly asset forge.",
    long_about = "Operate on Maquette `.maq` projects without opening the GUI. \
                  Suitable for build pipelines and CI."
)]
struct Cli {
    /// Emit machine-readable JSON envelope on stdout.
    #[arg(long, global = true, default_value_t = false)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Export a `.maq` project as glTF / GLB.
    Export(ExportArgs),
    /// Print a human- or JSON-readable summary of a `.maq` project.
    Info(InfoArgs),
    /// Load a `.maq` file and verify its structure. Non-zero exit on
    /// any failure.
    Validate(ValidateArgs),
    /// Render an isometric PNG preview of a `.maq` project using the
    /// pure-CPU rasterizer. Useful for CI thumbnails, docs, and
    /// headless regression testing.
    Render(RenderArgs),
    /// Share palettes across projects as `colors.json` documents.
    Palette(PaletteArgs),
    /// Generate textures from prompts. Providers: `mock` (offline
    /// deterministic noise) and `rustyme` (fan-out through a
    /// sonargrid cluster — worker contract in
    /// `docs/texture/rustyme.md`).
    Texture(TextureArgs),
    /// Inspect or sync block-meta records (the `BlockMeta` data the
    /// GUI's "Block Library" reads). Sources are `local` (built-in
    /// 12 blocks) and `hfrog` (artifact server — defaults to
    /// `https://hfrog.gamesci-lite.com`, override with
    /// `MAQUETTE_HFROG_BASE_URL`).
    Block(BlockArgs),
}

#[derive(Parser, Debug)]
struct BlockArgs {
    #[command(subcommand)]
    action: BlockAction,
}

#[derive(Subcommand, Debug)]
enum BlockAction {
    /// List blocks. Default scope is `all` (local + hfrog cache,
    /// merged). Use `--source hfrog` to force a fresh network
    /// fetch, `--source local` to limit to bundled blocks.
    List(BlockListArgs),
    /// Look up a single block by id and print its meta.
    Get(BlockGetArgs),
    /// Pull every Maquette-block record from hfrog and persist to
    /// the local cache. After this, offline `block list --source
    /// hfrog` works.
    Sync(BlockSyncArgs),
}

#[derive(Parser, Debug)]
struct BlockListArgs {
    /// Where to read from. `local` lists the bundled blocks;
    /// `hfrog` reads the disk cache (warm) or hits the network
    /// (cold); `all` merges both, with hfrog taking precedence on
    /// id collisions. Defaults to `all`.
    #[arg(long, value_enum, default_value_t = BlockSource::All)]
    source: BlockSource,
}

#[derive(Parser, Debug)]
struct BlockGetArgs {
    /// Block id (e.g. `grass`, `oak_planks`).
    id: String,
    /// Where to look. Same semantics as `block list`.
    #[arg(long, value_enum, default_value_t = BlockSource::All)]
    source: BlockSource,
}

#[derive(Parser, Debug)]
struct BlockSyncArgs {
    /// hfrog base URL. Falls back to `MAQUETTE_HFROG_BASE_URL`
    /// (default `https://hfrog.gamesci-lite.com`).
    #[arg(long)]
    base_url: Option<String>,
    /// hfrog `runtime` query namespace. Falls back to
    /// `MAQUETTE_HFROG_RUNTIME` (default `maquette-block/v1`).
    #[arg(long)]
    runtime: Option<String>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum BlockSource {
    /// Bundled, never hits the network.
    Local,
    /// Cached if available; otherwise fetches from hfrog.
    Hfrog,
    /// Local + hfrog merged. `hfrog` wins on id collisions.
    All,
}

#[derive(Parser, Debug)]
struct TextureArgs {
    #[command(subcommand)]
    action: TextureAction,
}

#[derive(Subcommand, Debug)]
enum TextureAction {
    /// Generate one texture and write it as a PNG. Uses the disk
    /// cache by default — re-running the same prompt+seed is free.
    Gen(TextureGenArgs),
    /// Cancel an in-flight Rustyme task by id. Best-effort; races
    /// with the worker picking it up — Rustyme only guarantees
    /// removal from the pending queue.
    Revoke(TextureRevokeArgs),
    /// Flush every pending task from a Rustyme queue. Use only when
    /// recovering from a stuck worker fleet; in-flight tasks are
    /// untouched.
    Purge(TexturePurgeArgs),
}

#[derive(Parser, Debug)]
struct TextureGenArgs {
    /// Free-form prompt. Provider-specific phrasing
    /// ("isometric block tile, low-poly minecraft style, …") is up
    /// to the caller — the CLI passes the string through verbatim.
    #[arg(long)]
    prompt: String,
    /// Output PNG path.
    #[arg(short, long)]
    out: PathBuf,
    /// Texture provider. `mock` is offline + deterministic; `rustyme`
    /// routes through the sonargrid task queue (see docs/texture/
    /// rustyme.md for the worker contract).
    #[arg(long, value_enum, default_value_t = ProviderArg::Mock)]
    provider: ProviderArg,
    /// Seed for the underlying RNG / diffusion model. Same prompt
    /// + same seed + same provider = byte-identical bytes.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Output width in pixels.
    #[arg(long, default_value_t = 128)]
    width: u32,
    /// Output height in pixels.
    #[arg(long, default_value_t = 128)]
    height: u32,
    /// Skip the on-disk cache lookup and store. Use when comparing
    /// providers / debugging output drift; in normal use the cache
    /// is what makes iterative prompt tuning cheap.
    #[arg(long, default_value_t = false)]
    no_cache: bool,
}

#[derive(Parser, Debug)]
struct TextureRevokeArgs {
    /// Rustyme task id (UUID v4) to cancel.
    task_id: String,
    /// Rustyme Admin HTTP base URL, e.g. `http://localhost:12121`.
    /// Falls back to `MAQUETTE_RUSTYME_ADMIN_URL` when unset.
    #[arg(long)]
    admin_url: Option<String>,
}

#[derive(Parser, Debug)]
struct TexturePurgeArgs {
    /// Logical queue name as Rustyme knows it (not the Redis LPUSH
    /// key). Typical value: `texgen`.
    queue: String,
    /// Rustyme Admin HTTP base URL. Falls back to
    /// `MAQUETTE_RUSTYME_ADMIN_URL` when unset.
    #[arg(long)]
    admin_url: Option<String>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ProviderArg {
    /// Offline, deterministic, free. Great for tests + bootstrap.
    Mock,
    /// Fan-out through Rustyme/sonargrid. Requires `MAQUETTE_RUSTYME_*`
    /// env vars or a running cluster — see `docs/texture/rustyme.md`.
    Rustyme,
}

#[derive(Parser, Debug)]
struct PaletteArgs {
    #[command(subcommand)]
    action: PaletteAction,
}

#[derive(Subcommand, Debug)]
enum PaletteAction {
    /// Write the palette of a `.maq` project to a `colors.json` file.
    Export(PaletteExportArgs),
    /// Replace the palette of a `.maq` project with one loaded from
    /// a `colors.json` file, writing the result to a new `.maq`.
    Import(PaletteImportArgs),
}

#[derive(Parser, Debug)]
struct PaletteExportArgs {
    /// Input `.maq` project file.
    input: PathBuf,
    /// Output `colors.json` path.
    #[arg(short, long)]
    out: PathBuf,
}

#[derive(Parser, Debug)]
struct PaletteImportArgs {
    /// Input `.maq` project file.
    input: PathBuf,
    /// Palette JSON to import (overwrites the project's current palette).
    #[arg(long)]
    from: PathBuf,
    /// Output `.maq` path — the updated project is written here. The
    /// original `input` is never modified.
    #[arg(short, long)]
    out: PathBuf,
}

#[derive(Parser, Debug)]
struct ExportArgs {
    /// Input `.maq` project file.
    input: PathBuf,
    /// Output path. Extension (`.glb` / `.gltf`) picks the format
    /// unless `--format` is given.
    #[arg(short, long)]
    out: PathBuf,
    /// Force a specific output format (overrides the extension).
    #[arg(long, value_enum)]
    format: Option<FormatArg>,
    /// Disable the inverted-hull outline in the exported model.
    /// (Preview outline is unaffected — this binary doesn't render.)
    #[arg(long, default_value_t = false)]
    no_outline: bool,
    /// Outline thickness, percent of the model's bounding diagonal.
    /// Clamped to 0..=10 by the exporter.
    #[arg(long, default_value_t = 2.5)]
    outline_width: f32,
    /// Outline color in `#RRGGBB` form (no alpha, no names). Defaults
    /// to black.
    #[arg(long, default_value = "#000000")]
    outline_color: String,
}

#[derive(Parser, Debug)]
struct InfoArgs {
    /// Input `.maq` project file.
    input: PathBuf,
}

#[derive(Parser, Debug)]
struct ValidateArgs {
    /// Input `.maq` project file.
    input: PathBuf,
}

#[derive(Parser, Debug)]
struct RenderArgs {
    /// Input `.maq` project file.
    input: PathBuf,
    /// Output PNG path.
    #[arg(short, long)]
    out: PathBuf,
    /// Image width in pixels.
    #[arg(long, default_value_t = render::DEFAULT_SIZE)]
    width: u32,
    /// Image height in pixels.
    #[arg(long, default_value_t = render::DEFAULT_SIZE)]
    height: u32,
    /// Background color in `#RRGGBB` form (sRGB, no alpha, no names).
    #[arg(long, default_value = "#181a1e")]
    background: String,
    /// Ambient luminance floor, 0..=1. Higher values flatten shading.
    #[arg(long, default_value_t = 0.35)]
    ambient: f32,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum FormatArg {
    Glb,
    Gltf,
}

impl From<FormatArg> for ExportFormat {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Glb => ExportFormat::Glb,
            FormatArg::Gltf => ExportFormat::Gltf,
        }
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    External(String),
    Project(ProjectError),
    Export(ExportError),
    Palette(PaletteIoError),
    Render(RenderError),
    BlockMeta(BlockMetaError),
    Provider(ProviderError),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(msg) | Self::External(msg) => f.write_str(msg),
            Self::Project(e) => fmt::Display::fmt(e, f),
            Self::Export(e) => fmt::Display::fmt(e, f),
            Self::Palette(e) => fmt::Display::fmt(e, f),
            Self::Render(e) => fmt::Display::fmt(e, f),
            Self::BlockMeta(e) => fmt::Display::fmt(e, f),
            Self::Provider(e) => fmt::Display::fmt(e, f),
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::Json(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl From<ProjectError> for CliError {
    fn from(e: ProjectError) -> Self { Self::Project(e) }
}
impl From<ExportError> for CliError {
    fn from(e: ExportError) -> Self { Self::Export(e) }
}
impl From<PaletteIoError> for CliError {
    fn from(e: PaletteIoError) -> Self { Self::Palette(e) }
}
impl From<RenderError> for CliError {
    fn from(e: RenderError) -> Self { Self::Render(e) }
}
impl From<BlockMetaError> for CliError {
    fn from(e: BlockMetaError) -> Self { Self::BlockMeta(e) }
}
impl From<ProviderError> for CliError {
    fn from(e: ProviderError) -> Self { Self::Provider(e) }
}
impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}
impl From<serde_json::Error> for CliError {
    fn from(e: serde_json::Error) -> Self { Self::Json(e) }
}

impl CliError {
    fn kind(&self) -> ErrorKind {
        match self {
            Self::Usage(_) => ErrorKind::Usage,
            Self::External(_) => ErrorKind::External,
            Self::Project(e) => match e {
                ProjectError::Io(io) => rlit_cli::io_error_kind(io),
                ProjectError::Json(_)
                | ProjectError::UnsupportedVersion(_)
                | ProjectError::InvalidCellCount { .. } => ErrorKind::Input,
            },
            Self::Export(e) => match e {
                ExportError::Empty => ErrorKind::Input,
                ExportError::Io(io) => rlit_cli::io_error_kind(io),
                ExportError::Json(_) => ErrorKind::Runtime,
            },
            Self::Palette(e) => match e {
                PaletteIoError::Io(io) => rlit_cli::io_error_kind(io),
                PaletteIoError::Json(_)
                | PaletteIoError::UnsupportedVersion { .. }
                | PaletteIoError::TooManySlots { .. }
                | PaletteIoError::BadColor { .. } => ErrorKind::Input,
            },
            Self::Render(e) => match e {
                RenderError::InvalidSize { .. } => ErrorKind::Usage,
                RenderError::Io(io) => rlit_cli::io_error_kind(io),
                RenderError::Png(_) => ErrorKind::Runtime,
            },
            Self::BlockMeta(e) => match e {
                BlockMetaError::NotFound(_) | BlockMetaError::Decode(_) => ErrorKind::Input,
                BlockMetaError::Remote(_) => ErrorKind::External,
                BlockMetaError::Io(io) => rlit_cli::io_error_kind(io),
            },
            Self::Provider(e) => match e {
                ProviderError::UnknownModel(_) => ErrorKind::Usage,
                ProviderError::Remote(_) => ErrorKind::External,
                ProviderError::Io(io) => rlit_cli::io_error_kind(io),
                ProviderError::PngEncode(_) => ErrorKind::Runtime,
            },
            Self::Io(io) => rlit_cli::io_error_kind(io),
            Self::Json(_) => ErrorKind::Input,
        }
    }
}

fn main() -> ! {
    let cli: Cli = CONTRACT.parse();
    let result = run(cli);
    rlit_cli::finish(result, CliError::kind);
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Export(args) => cmd_export(args),
        Command::Info(args) => cmd_info(args),
        Command::Validate(args) => cmd_validate(args),
        Command::Render(args) => cmd_render(args),
        Command::Palette(args) => match args.action {
            PaletteAction::Export(a) => cmd_palette_export(a),
            PaletteAction::Import(a) => cmd_palette_import(a),
        },
        Command::Texture(args) => match args.action {
            TextureAction::Gen(a) => cmd_texture_gen(a),
            TextureAction::Revoke(a) => cmd_texture_revoke(a),
            TextureAction::Purge(a) => cmd_texture_purge(a),
        },
        Command::Block(args) => match args.action {
            BlockAction::List(a) => cmd_block_list(a),
            BlockAction::Get(a) => cmd_block_get(a),
            BlockAction::Sync(a) => cmd_block_sync(a),
        },
    }
}

fn cmd_texture_gen(args: TextureGenArgs) -> Result<(), CliError> {
    let provider: Box<dyn TextureProvider> = match args.provider {
        ProviderArg::Mock => Box::new(MockProvider),
        ProviderArg::Rustyme => {
            let cfg = RustymeConfig::from_env().ok_or_else(|| {
                CliError::External(
                    "rustyme provider selected but MAQUETTE_RUSTYME_REDIS_URL is not set. \
                     See docs/texture/rustyme.md for the full env-var list."
                        .into(),
                )
            })?;
            Box::new(RustymeProvider::new(cfg))
        }
    };
    // Upstream model id — the Mock provider has a fixed one; for
    // Rustyme we let the caller pick via `MAQUETTE_RUSTYME_MODEL`
    // so cache entries don't collide when the worker fleet swaps
    // backends (fal-ai/flux/schnell → replicate/sdxl → …).
    let model_id = match args.provider {
        ProviderArg::Mock => MockProvider::MODEL_ID.to_string(),
        ProviderArg::Rustyme => std::env::var("MAQUETTE_RUSTYME_MODEL")
            .unwrap_or_else(|_| "rustyme:texture.gen".to_string()),
    };
    let request = TextureRequest::new(
        args.prompt,
        args.seed,
        args.width,
        args.height,
        model_id,
    );
    let cache_dir = if args.no_cache {
        None
    } else {
        texgen::default_cache_dir()
    };
    let bytes = texgen::generate_cached(provider.as_ref(), &request, cache_dir.as_deref())?;
    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&args.out, bytes.as_slice())?;
    let cache_key = request.cache_key();
    rlit_cli::add_artifact(&args.out, "texture");
    rlit_cli::set_data(json!({
        "out": args.out,
        "bytes": bytes.len(),
        "provider": provider.name(),
        "cache_key": cache_key,
    }));
    if !rlit_cli::json_mode() {
        println!(
            "wrote {} ({} bytes, provider={}, cache_key={})",
            args.out.display(),
            bytes.len(),
            provider.name(),
            cache_key,
        );
    }
    Ok(())
}

fn resolve_admin_url(cli_arg: Option<String>) -> Result<String, CliError> {
    cli_arg
        .or_else(|| std::env::var("MAQUETTE_RUSTYME_ADMIN_URL").ok())
        .ok_or_else(|| {
            CliError::Usage(
                "Rustyme admin URL missing. Pass --admin-url or set \
                 MAQUETTE_RUSTYME_ADMIN_URL (e.g. http://localhost:12121)."
                    .into(),
            )
        })
}

fn cmd_texture_revoke(args: TextureRevokeArgs) -> Result<(), CliError> {
    let admin = resolve_admin_url(args.admin_url)?;
    let body = texgen::rustyme::revoke(&admin, &args.task_id)?;
    rlit_cli::set_data(json!({
        "task_id": args.task_id,
        "response": body,
    }));
    if !rlit_cli::json_mode() {
        println!("revoked task {} — admin response: {body}", args.task_id);
    }
    Ok(())
}

fn cmd_texture_purge(args: TexturePurgeArgs) -> Result<(), CliError> {
    let admin = resolve_admin_url(args.admin_url)?;
    let body = texgen::rustyme::purge_queue(&admin, &args.queue)?;
    rlit_cli::set_data(json!({
        "queue": args.queue,
        "response": body,
    }));
    if !rlit_cli::json_mode() {
        println!("purged queue {} — admin response: {body}", args.queue);
    }
    Ok(())
}

fn cmd_export(args: ExportArgs) -> Result<(), CliError> {
    let (grid, palette) = project::read_project(&args.input)?;
    let format = resolve_format(&args)?;
    let outline = OutlineConfig {
        enabled: !args.no_outline,
        width_pct: args.outline_width,
        color: parse_hex_color(&args.outline_color).map_err(CliError::Usage)?,
    };
    let out_path = args.out;
    let opts = ExportOptions {
        path: out_path.clone(),
        format,
        outline,
    };
    export::write(&grid, &palette, &opts)?;
    rlit_cli::add_artifact(&out_path, "model");
    rlit_cli::set_data(json!({ "out": out_path }));
    Ok(())
}

fn resolve_format(args: &ExportArgs) -> Result<ExportFormat, CliError> {
    if let Some(f) = args.format {
        return Ok(f.into());
    }
    // Infer from the output extension. If it's missing or unknown,
    // fall back to GLB — GLB is the single-file format most engines
    // prefer. Users who wanted .gltf will notice the extension
    // mismatch and re-run.
    match args
        .out
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("gltf") => Ok(ExportFormat::Gltf),
        _ => Ok(ExportFormat::Glb),
    }
}

fn cmd_info(args: InfoArgs) -> Result<(), CliError> {
    let (grid, palette) = project::read_project(&args.input)?;
    let summary = summarize(&grid, &palette);
    rlit_cli::set_data(summary.to_json_value());
    if !rlit_cli::json_mode() {
        summary.print_human(std::io::stdout().lock())?;
    }
    Ok(())
}

fn cmd_validate(args: ValidateArgs) -> Result<(), CliError> {
    let (_grid, _palette) = project::read_project(&args.input)?;
    rlit_cli::set_data(json!({ "valid": true, "input": args.input }));
    if !rlit_cli::json_mode() {
        println!("ok: {} is a valid Maquette project", args.input.display());
    }
    Ok(())
}

fn cmd_palette_export(args: PaletteExportArgs) -> Result<(), CliError> {
    let (_grid, palette) = project::read_project(&args.input)?;
    palette_io::write_palette_json(&palette, &args.out)?;
    rlit_cli::add_artifact(&args.out, "palette");
    rlit_cli::set_data(json!({ "out": args.out }));
    Ok(())
}

fn cmd_palette_import(args: PaletteImportArgs) -> Result<(), CliError> {
    let (grid, mut palette) = project::read_project(&args.input)?;
    palette_io::import_palette_into(&mut palette, &args.from)?;
    project::write_project(&args.out, &grid, &palette)?;
    rlit_cli::add_artifact(&args.out, "project");
    rlit_cli::set_data(json!({ "out": args.out }));
    Ok(())
}

// ---------------------------------------------------------------------
// `block` subcommand — read-side only (Maquette doesn't publish to
// hfrog; that's an ops job using hfrog's own tooling).
// ---------------------------------------------------------------------

fn cmd_block_list(args: BlockListArgs) -> Result<(), CliError> {
    let blocks = collect_blocks(args.source)?;
    rlit_cli::set_data(&blocks);
    if !rlit_cli::json_mode() {
        if blocks.is_empty() {
            println!("(no blocks — try `maquette-cli block sync` to fetch hfrog catalog)");
            return Ok(());
        }
        // Five columns: id / source / shape / color / hint-snippet.
        println!("{:<14} {:<6} {:<6} {:<10} TEXTURE HINT", "ID", "FROM", "SHAPE", "COLOR");
        for b in &blocks {
            let hex = format!(
                "#{:02x}{:02x}{:02x}",
                (b.default_color.r * 255.0 + 0.5) as u8,
                (b.default_color.g * 255.0 + 0.5) as u8,
                (b.default_color.b * 255.0 + 0.5) as u8,
            );
            let snippet = truncate(&b.texture_hint, 60);
            println!(
                "{:<14} {:<6} {:<6} {:<10} {}",
                b.id,
                b.source.label(),
                shape_label(b.shape_hint),
                hex,
                snippet
            );
        }
        println!("\n{} blocks total", blocks.len());
    }
    Ok(())
}

fn cmd_block_get(args: BlockGetArgs) -> Result<(), CliError> {
    let block = match args.source {
        BlockSource::Local => LocalProvider::new().get(&args.id)?,
        BlockSource::Hfrog => HfrogProvider::new(HfrogConfig::from_env()).get(&args.id)?,
        BlockSource::All => match LocalProvider::new().get(&args.id) {
            Ok(b) => b,
            Err(_) => HfrogProvider::new(HfrogConfig::from_env()).get(&args.id)?,
        },
    };
    rlit_cli::set_data(&block);
    if !rlit_cli::json_mode() {
        println!("id:           {}", block.id);
        println!("name:         {}", block.name);
        println!("source:       {}", block.source.label());
        println!("shape_hint:   {}", shape_label(block.shape_hint));
        let s = block.default_color;
        println!(
            "default_color: rgb({}, {}, {})",
            (s.r * 255.0 + 0.5) as u8,
            (s.g * 255.0 + 0.5) as u8,
            (s.b * 255.0 + 0.5) as u8,
        );
        if !block.tags.is_empty() {
            println!("tags:         {}", block.tags.join(", "));
        }
        if !block.texture_hint.is_empty() {
            println!("texture_hint: {}", block.texture_hint);
        }
        println!("description:  {}", block.description);
    }
    Ok(())
}

fn cmd_block_sync(args: BlockSyncArgs) -> Result<(), CliError> {
    let mut cfg = HfrogConfig::from_env();
    if let Some(b) = args.base_url {
        cfg.base_url = b.trim_end_matches('/').to_string();
    }
    if let Some(r) = args.runtime {
        cfg.runtime = r;
    }
    let provider = HfrogProvider::new(cfg.clone());
    let blocks = provider.sync()?;
    rlit_cli::set_data(&blocks);
    if !rlit_cli::json_mode() {
        let cache = block_meta::default_cache_dir()
            .map(|p| {
                p.join("hfrog")
                    .join(&cfg.runtime)
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| "(no cache dir)".to_string());
        println!(
            "synced {} blocks from {} (runtime={}) → {}",
            blocks.len(),
            cfg.base_url,
            cfg.runtime,
            cache,
        );
    }
    Ok(())
}

fn collect_blocks(
    source: BlockSource,
) -> Result<Vec<maquette::block_meta::BlockMeta>, CliError> {
    use maquette::block_meta::BlockMeta;
    let mut out: Vec<BlockMeta> = match source {
        BlockSource::Local => LocalProvider::new().list()?,
        BlockSource::Hfrog => HfrogProvider::new(HfrogConfig::from_env()).list()?,
        BlockSource::All => {
            let mut local = LocalProvider::new().list()?;
            // Read the cache instead of forcing a network call —
            // `block list --source all` should be fast & offline-ok.
            // The user explicitly opts into a network call by
            // running `block sync` first or `block list --source
            // hfrog`.
            let hfrog = HfrogProvider::new(HfrogConfig::from_env())
                .list()
                .unwrap_or_default();
            // hfrog wins on id collisions — same id from server
            // overrides bundled definition (lets ops correct a
            // block without a Maquette release).
            let hfrog_ids: std::collections::HashSet<String> =
                hfrog.iter().map(|b| b.id.clone()).collect();
            local.retain(|b| !hfrog_ids.contains(&b.id));
            local.extend(hfrog);
            local
        }
    };
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn shape_label(s: maquette::grid::ShapeKind) -> &'static str {
    use maquette::grid::ShapeKind;
    match s {
        ShapeKind::Cube => "cube",
        ShapeKind::Sphere => "sphere",
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let mut t: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        t.push('…');
        t
    }
}

fn cmd_render(args: RenderArgs) -> Result<(), CliError> {
    let (grid, palette) = project::read_project(&args.input)?;
    let bg = parse_hex_color(&args.background)
        .map_err(CliError::Usage)?
        .to_srgba();
    let opts = RenderOptions {
        width: args.width,
        height: args.height,
        background: [
            (bg.red * 255.0 + 0.5) as u8,
            (bg.green * 255.0 + 0.5) as u8,
            (bg.blue * 255.0 + 0.5) as u8,
        ],
        ambient: args.ambient.clamp(0.0, 1.0),
        ..RenderOptions::default()
    };
    render::write_png(&grid, &palette, &opts, &args.out)?;
    rlit_cli::add_artifact(&args.out, "image");
    rlit_cli::set_data(json!({
        "out": args.out,
        "width": opts.width,
        "height": opts.height,
    }));
    Ok(())
}

struct ProjectSummary {
    w: usize,
    h: usize,
    total_cells: usize,
    painted_cells: usize,
    colors_used: Vec<u8>,
    max_height: u8,
    /// Total number of palette slots, including deleted holes.
    palette_size: usize,
    /// Number of live (non-deleted) colors. Since v0.6 the palette is
    /// sparse; `palette_size` and `palette_live` differ once a slot
    /// has been deleted.
    palette_live: usize,
    selected_color: u8,
}

fn summarize(grid: &Grid, palette: &Palette) -> ProjectSummary {
    let mut used = [false; 256];
    let mut painted = 0usize;
    let mut max_h: u8 = 0;
    for cell in &grid.cells {
        if let Some(ci) = cell.color_idx {
            used[ci as usize] = true;
            painted += 1;
            let h = if cell.height == 0 { 1 } else { cell.height };
            if h > max_h {
                max_h = h;
            }
        }
    }
    let colors_used = (0..256u16)
        .filter(|i| used[*i as usize])
        .map(|i| i as u8)
        .collect();
    ProjectSummary {
        w: grid.w,
        h: grid.h,
        total_cells: grid.w * grid.h,
        painted_cells: painted,
        colors_used,
        max_height: max_h,
        palette_size: palette.colors.len(),
        palette_live: palette.live_count(),
        selected_color: palette.selected,
    }
}

impl ProjectSummary {
    fn print_human<W: std::io::Write>(&self, mut w: W) -> std::io::Result<()> {
        writeln!(w, "canvas:         {} × {}", self.w, self.h)?;
        writeln!(
            w,
            "cells:          {} painted / {} total",
            self.painted_cells, self.total_cells
        )?;
        writeln!(w, "max height:     {}", self.max_height)?;
        if self.palette_live == self.palette_size {
            writeln!(
                w,
                "palette:        {} colors (selected: #{})",
                self.palette_live, self.selected_color
            )?;
        } else {
            writeln!(
                w,
                "palette:        {} live / {} slots (selected: #{})",
                self.palette_live, self.palette_size, self.selected_color
            )?;
        }
        writeln!(w, "colors used:    {:?}", self.colors_used)?;
        Ok(())
    }

    fn to_json_value(&self) -> serde_json::Value {
        json!({
            "canvas": { "w": self.w, "h": self.h },
            "cells": { "painted": self.painted_cells, "total": self.total_cells },
            "max_height": self.max_height,
            "palette": {
                "size": self.palette_size,
                "live": self.palette_live,
                "selected": self.selected_color,
            },
            "colors_used": self.colors_used,
        })
    }
}

fn parse_hex_color(s: &str) -> Result<Color, String> {
    let raw = s.strip_prefix('#').unwrap_or(s);
    if raw.len() != 6 {
        return Err(format!(
            "invalid color `{s}` — expected `#RRGGBB` (6 hex digits)"
        ));
    }
    let bytes = u32::from_str_radix(raw, 16)
        .map_err(|_| format!("invalid color `{s}` — not valid hex"))?;
    let r = ((bytes >> 16) & 0xff) as f32 / 255.0;
    let g = ((bytes >> 8) & 0xff) as f32 / 255.0;
    let b = (bytes & 0xff) as f32 / 255.0;
    Ok(Color::srgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_basic() {
        let c = parse_hex_color("#ff0000").unwrap();
        let s = c.to_srgba();
        assert!((s.red - 1.0).abs() < 1e-3);
        assert!(s.green.abs() < 1e-3);
        assert!(s.blue.abs() < 1e-3);
    }

    #[test]
    fn hex_color_rejects_short_form() {
        assert!(parse_hex_color("#f00").is_err());
    }

    #[test]
    fn hex_color_rejects_non_hex() {
        assert!(parse_hex_color("#zzzzzz").is_err());
    }

    #[test]
    fn format_infers_from_extension() {
        let args = ExportArgs {
            input: PathBuf::from("in.maq"),
            out: PathBuf::from("out.gltf"),
            format: None,
            no_outline: false,
            outline_width: 2.5,
            outline_color: "#000000".into(),
        };
        assert!(matches!(resolve_format(&args), Ok(ExportFormat::Gltf)));

        let args2 = ExportArgs {
            out: PathBuf::from("out.glb"),
            ..args
        };
        assert!(matches!(resolve_format(&args2), Ok(ExportFormat::Glb)));
    }

    #[test]
    fn format_defaults_to_glb_on_unknown_extension() {
        let args = ExportArgs {
            input: PathBuf::from("in.maq"),
            out: PathBuf::from("out.xyz"),
            format: None,
            no_outline: false,
            outline_width: 2.5,
            outline_color: "#000000".into(),
        };
        assert!(matches!(resolve_format(&args), Ok(ExportFormat::Glb)));
    }

    #[test]
    fn summary_counts_painted_cells_and_colors() {
        let mut grid = Grid::with_size(4, 4);
        grid.paint(0, 0, 1, 2);
        grid.paint(1, 0, 1, 3);
        grid.paint(2, 0, 3, 1);
        let pal = Palette::default();
        let s = summarize(&grid, &pal);
        assert_eq!(s.painted_cells, 3);
        assert_eq!(s.max_height, 3);
        assert_eq!(s.colors_used, vec![1u8, 3u8]);
        assert_eq!(s.total_cells, 16);
    }

    #[test]
    fn cli_matches_rlit_contract() {
        use clap::CommandFactory;
        let issues = rlit_cli::check_contract(&CONTRACT, &Cli::command());
        assert!(issues.is_empty(), "contract issues:\n{}", issues.join("\n"));
    }
}
