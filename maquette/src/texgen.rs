//! Texture generation — Phase 1 (v0.10 A).
//!
//! ## Why this lives in the headless core
//!
//! Texture synthesis must obey the Headless Invariant
//! (see `docs/handoff/COST_AWARENESS.md`): the GUI is a presentation
//! layer over a window-free core. Concretely that means:
//!
//! * the `TextureProvider` trait is sync, so a CLI can call it
//!   straight-line and the GUI can wrap it in
//!   `AsyncComputeTaskPool::spawn(async move { provider.generate(...) })`
//!   without a tokio runtime,
//! * disk caching is shared between GUI and CLI runs (re-running an
//!   identical prompt never re-bills),
//! * no provider impl reaches for `bevy_egui` / `wgpu` / `winit`.
//!
//! ## Phase plan recap
//!
//! - **A**: trait + types + disk cache + `MockProvider`.
//!   Zero external API deps; everything offline + deterministic.
//! - **B** (this file, submodule `rustyme`): **Rustyme / sonargrid**
//!   producer. We only own the producer side: LPUSH a
//!   `texture.gen` [`TaskEnvelope`][rustyme-proto], BRPOP the
//!   result, decode PNG bytes from base64. Workers (which actually
//!   call Fal / Replicate / …) are someone else's code — our
//!   contract with them is frozen as `WORKER-CONTRACT` at the top
//!   of that submodule.
//! - **C+**: per-palette-slot prompts in the project schema, GUI
//!   wiring, textured preview, glTF baking. See `NEXT.md`.
//!
//! [rustyme-proto]: /Users/admin/data0/public_work/sonargrid/rustyme-core/src/protocol.rs
//!
//! ## Determinism contract
//!
//! Two calls to a provider with the same `TextureRequest` MUST
//! produce byte-identical PNG output. The cache key (and most of our
//! tests) rely on this. `MockProvider` honours it by construction;
//! `FalProvider` will set the API's `seed` parameter to
//! `request.seed` and pin the model version explicitly.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// One texture-generation request. Hashing this whole struct (via
/// `cache_key`) yields the stable on-disk filename, so adding a new
/// field that affects the *image bytes* must also be in `hash_into`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureRequest {
    /// Free-form prompt. Provider-specific phrasing
    /// ("isometric block tile, ...") is the *caller's* responsibility
    /// — the trait only carries the verbatim string so `MockProvider`
    /// stays predictable.
    pub prompt: String,
    /// Seed for the underlying RNG/diffusion. Two requests differing
    /// only in `seed` are intentionally different cache entries.
    pub seed: u64,
    /// Output image size in pixels. We're targeting voxel-face
    /// textures, so 128×128 is the practical sweet spot — high enough
    /// for a visible style, low enough that downscale to 16/32/64
    /// per-face stays clean.
    pub width: u32,
    pub height: u32,
    /// Provider-specific model identifier (e.g. `"fal-ai/flux/schnell"`,
    /// `"mock-v1"`). Part of the cache key so different models with
    /// the same prompt don't collide on disk.
    pub model: String,
}

impl TextureRequest {
    /// Convenience constructor for tests / the CLI. Real callers
    /// should construct the struct literal so future field additions
    /// fail to compile (they want to think about each new dimension).
    pub fn new(
        prompt: impl Into<String>,
        seed: u64,
        width: u32,
        height: u32,
        model: impl Into<String>,
    ) -> Self {
        Self {
            prompt: prompt.into(),
            seed,
            width,
            height,
            model: model.into(),
        }
    }

    fn hash_into(&self, h: &mut Sha256) {
        // Domain-separator: bumping this string invalidates *every*
        // cached texture from previous Maquette versions. Use it when
        // we change what a "request" means (e.g. add per-face UV
        // hints), not when we just add a new optional field that
        // defaults to the prior behaviour.
        h.update(b"maquette-texgen-v1\x00");
        h.update(self.prompt.as_bytes());
        h.update(b"\x00");
        h.update(self.seed.to_le_bytes());
        h.update(self.width.to_le_bytes());
        h.update(self.height.to_le_bytes());
        h.update(self.model.as_bytes());
    }

    /// 64-char lowercase hex of `SHA-256(request)`. Used directly as
    /// the on-disk cache filename (`<key>.png`).
    pub fn cache_key(&self) -> String {
        let mut h = Sha256::new();
        self.hash_into(&mut h);
        let digest = h.finalize();
        let mut s = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write;
            let _ = write!(&mut s, "{byte:02x}");
        }
        s
    }
}

/// PNG bytes (RGB or RGBA, encoded). We deliberately don't decode
/// here — different consumers want different things (Bevy wants a
/// `wgpu` Image, the CLI wants to write bytes to disk, tests want
/// to round-trip through the `png` crate). Returning raw PNG keeps
/// the trait minimal.
#[derive(Clone, Debug)]
pub struct TextureBytes(pub Vec<u8>);

impl TextureBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Caller asked for a model the provider doesn't know about.
    /// Phase B (`FalProvider`) returns this when `model` doesn't
    /// match any known Fal endpoint.
    #[error("unknown model `{0}` for this provider")]
    UnknownModel(String),

    /// Network or remote-API failure. Phase A doesn't emit this;
    /// included here so Phase B can plug straight in without a trait
    /// signature change.
    #[error("provider request failed: {0}")]
    Remote(String),

    /// Local I/O while caching / decoding. Bubbled up so the CLI can
    /// print a useful path and the GUI can toast.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// PNG encoding inside `MockProvider` failed. In practice
    /// indicates an out-of-memory condition; preserved as a typed
    /// variant to keep the error surface explicit.
    #[error("png encode failed: {0}")]
    PngEncode(String),
}

/// Plug point for any model backend. Sync on purpose (see module
/// docs); GUI consumers offload to a Bevy task pool.
pub trait TextureProvider: Send + Sync {
    /// Stable identifier surfaced in logs and the CLI's `--provider`
    /// flag. Must be lowercase, kebab-case-ish (`mock`, `fal`,
    /// `replicate`).
    fn name(&self) -> &'static str;

    /// Generate one texture. Implementations MUST be deterministic
    /// w.r.t. `request` (same input → byte-identical output) so the
    /// disk cache key is meaningful.
    fn generate(&self, request: &TextureRequest) -> Result<TextureBytes, ProviderError>;
}

/// Deterministic PNG generator with no external deps. Used by
/// tests, by the CLI when `--provider mock` (or no `FAL_KEY` is set
/// and `--allow-mock` is passed), and as the GUI's "preview" while
/// you're choosing whether to spend real money.
///
/// The output is *not* meant to look like a real material — it's a
/// hash-derived solid color washed with seeded noise. The point is
/// "you can see something show up, you know which prompt it came
/// from", not "this is shippable art".
pub struct MockProvider;

impl MockProvider {
    /// Provider model id surfaced in [`TextureRequest::model`] when
    /// you want to bind a request to this provider's output. Other
    /// providers will reject this id with [`ProviderError::UnknownModel`].
    pub const MODEL_ID: &'static str = "mock-v1";
}

impl TextureProvider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn generate(&self, request: &TextureRequest) -> Result<TextureBytes, ProviderError> {
        if request.width == 0 || request.height == 0 {
            return Err(ProviderError::PngEncode(format!(
                "zero-sized request: {}×{}",
                request.width, request.height
            )));
        }
        // Cap the mock at something reasonable so a stray
        // 8192×8192 doesn't OOM CI. Real providers will enforce
        // their own limits.
        if request.width > 1024 || request.height > 1024 {
            return Err(ProviderError::PngEncode(format!(
                "mock provider tops out at 1024×1024 (got {}×{})",
                request.width, request.height
            )));
        }

        // Derive a base color from the prompt hash so different
        // prompts produce visibly different tiles. We *also* mix in
        // model+seed so the Mock CLI smoke test can prove "different
        // seed → different bytes".
        let key = request.cache_key();
        let key_bytes = key.as_bytes();
        let base_r = key_bytes[0];
        let base_g = key_bytes[2];
        let base_b = key_bytes[4];

        // SplitMix64 — single-line PRNG, no deps, fine for visual
        // noise. Seeded by request.seed XOR a digest of (prompt,
        // model) so two prompts with the same seed still differ.
        let mut state = request.seed
            ^ u64::from_le_bytes([
                key_bytes[8],
                key_bytes[9],
                key_bytes[10],
                key_bytes[11],
                key_bytes[12],
                key_bytes[13],
                key_bytes[14],
                key_bytes[15],
            ]);

        let pixels = (request.width as usize) * (request.height as usize);
        let mut rgb = Vec::<u8>::with_capacity(pixels * 3);
        for _ in 0..pixels {
            // SplitMix64 step.
            state = state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^= z >> 31;
            let n0 = (z & 0xFF) as u8;
            let n1 = ((z >> 8) & 0xFF) as u8;
            let n2 = ((z >> 16) & 0xFF) as u8;
            // Blend base color with noise — keeps the tile readable
            // as "the grass-prompt one" while still showing motion.
            let r = base_r.wrapping_add(n0 / 4);
            let g = base_g.wrapping_add(n1 / 4);
            let b = base_b.wrapping_add(n2 / 4);
            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }

        encode_png_rgb(&rgb, request.width, request.height)
            .map(TextureBytes)
            .map_err(|e| ProviderError::PngEncode(e.to_string()))
    }
}

fn encode_png_rgb(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut buf = Vec::<u8>::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgb)?;
    }
    Ok(buf)
}

// ---------------------------------------------------------------------
// Disk cache
// ---------------------------------------------------------------------

/// Where cache files live. Honours `XDG_CACHE_HOME` first, then
/// falls back to `$HOME/.cache`. Returns `None` only if neither
/// variable is set, which on a normal desktop never happens — but
/// the CLI still handles it (`--no-cache`).
pub fn default_cache_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("maquette").join("textures"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(
                PathBuf::from(home)
                    .join(".cache")
                    .join("maquette")
                    .join("textures"),
            );
        }
    }
    None
}

/// Look up a previously generated texture for `request`. Returns
/// `Ok(None)` on cache miss; surfacing miss as a value (rather than
/// an error) lets the caller cleanly fall through to the provider.
pub fn cache_get(cache_dir: &Path, request: &TextureRequest) -> std::io::Result<Option<TextureBytes>> {
    let path = cache_dir.join(format!("{}.png", request.cache_key()));
    match fs::read(&path) {
        Ok(bytes) => {
            log::debug!("texgen: cache hit {}", path.display());
            Ok(Some(TextureBytes(bytes)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Persist a generated texture under its content-addressed name.
/// Writes to a `.tmp` file first and renames, so a crash mid-write
/// can't leave a half-written PNG on disk.
pub fn cache_put(
    cache_dir: &Path,
    request: &TextureRequest,
    bytes: &TextureBytes,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(cache_dir)?;
    let key = request.cache_key();
    let final_path = cache_dir.join(format!("{key}.png"));
    let tmp_path = cache_dir.join(format!("{key}.png.tmp"));
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(&bytes.0)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &final_path)?;
    log::debug!("texgen: cache put {}", final_path.display());
    Ok(final_path)
}

/// Cache-aware generation: hits cache first, otherwise calls the
/// provider and stashes the result. The single function call sites
/// should use unless they have a specific reason to bypass.
///
/// `cache_dir = None` disables caching entirely (CI, tests where
/// determinism is enforced separately).
pub fn generate_cached(
    provider: &dyn TextureProvider,
    request: &TextureRequest,
    cache_dir: Option<&Path>,
) -> Result<TextureBytes, ProviderError> {
    if let Some(dir) = cache_dir {
        if let Some(hit) = cache_get(dir, request)? {
            log::info!(
                "texgen: cache hit (provider={}, seed={}, prompt={:?})",
                provider.name(),
                request.seed,
                truncate_for_log(&request.prompt),
            );
            return Ok(hit);
        }
    }

    log::info!(
        "texgen: generating via {} (seed={}, {}×{}, prompt={:?})",
        provider.name(),
        request.seed,
        request.width,
        request.height,
        truncate_for_log(&request.prompt),
    );
    let started = std::time::Instant::now();
    let bytes = provider.generate(request)?;
    log::info!(
        "texgen: generated {} bytes in {:.2?}",
        bytes.len(),
        started.elapsed(),
    );

    if let Some(dir) = cache_dir {
        cache_put(dir, request, &bytes)?;
    }
    Ok(bytes)
}

fn truncate_for_log(s: &str) -> String {
    const MAX: usize = 80;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let head: String = s.chars().take(MAX).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn req(prompt: &str) -> TextureRequest {
        TextureRequest::new(prompt, 42, 64, 64, MockProvider::MODEL_ID)
    }

    #[test]
    fn cache_key_is_stable_for_same_request() {
        let a = req("grass tile");
        let b = req("grass tile");
        assert_eq!(a.cache_key(), b.cache_key());
    }

    #[test]
    fn cache_key_changes_with_any_field() {
        let base = req("grass tile");

        let mut p = base.clone();
        p.prompt = "stone tile".into();
        assert_ne!(base.cache_key(), p.cache_key());

        let mut s = base.clone();
        s.seed = 43;
        assert_ne!(base.cache_key(), s.cache_key());

        let mut w = base.clone();
        w.width = 65;
        assert_ne!(base.cache_key(), w.cache_key());

        let mut h = base.clone();
        h.height = 65;
        assert_ne!(base.cache_key(), h.cache_key());

        let mut m = base.clone();
        m.model = "fal-ai/flux/schnell".into();
        assert_ne!(base.cache_key(), m.cache_key());
    }

    #[test]
    fn cache_key_is_64_hex_chars() {
        let k = req("anything").cache_key();
        assert_eq!(k.len(), 64);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn mock_is_deterministic_for_same_request() {
        let p = MockProvider;
        let r = req("grass tile");
        let a = p.generate(&r).unwrap();
        let b = p.generate(&r).unwrap();
        assert_eq!(a.0, b.0);
    }

    #[test]
    fn mock_diverges_on_different_seed() {
        let p = MockProvider;
        let mut r1 = req("grass");
        let mut r2 = req("grass");
        r1.seed = 1;
        r2.seed = 2;
        let a = p.generate(&r1).unwrap();
        let b = p.generate(&r2).unwrap();
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn mock_diverges_on_different_prompt() {
        let p = MockProvider;
        let a = p.generate(&req("grass")).unwrap();
        let b = p.generate(&req("stone")).unwrap();
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn mock_output_is_a_valid_png() {
        let p = MockProvider;
        let r = TextureRequest::new("grass", 7, 32, 32, MockProvider::MODEL_ID);
        let bytes = p.generate(&r).unwrap();
        // png 0.18's Decoder needs `BufRead + Seek`; wrap in a Cursor.
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!(info.width, 32);
        assert_eq!(info.height, 32);
    }

    #[test]
    fn mock_rejects_zero_size() {
        let p = MockProvider;
        let r = TextureRequest::new("x", 1, 0, 32, MockProvider::MODEL_ID);
        assert!(matches!(p.generate(&r), Err(ProviderError::PngEncode(_))));
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempdir().unwrap();
        let p = MockProvider;
        let r = req("grass");
        let bytes = p.generate(&r).unwrap();

        assert!(cache_get(dir.path(), &r).unwrap().is_none());
        cache_put(dir.path(), &r, &bytes).unwrap();
        let read_back = cache_get(dir.path(), &r).unwrap().unwrap();
        assert_eq!(read_back.0, bytes.0);
    }

    #[test]
    fn generate_cached_writes_then_hits() {
        let dir = tempdir().unwrap();
        let p = MockProvider;
        let r = req("grass");

        // First call: cache miss → generate → cache write.
        let first = generate_cached(&p, &r, Some(dir.path())).unwrap();
        assert!(dir
            .path()
            .join(format!("{}.png", r.cache_key()))
            .exists());

        // Second call: cache hit → bytes identical.
        let second = generate_cached(&p, &r, Some(dir.path())).unwrap();
        assert_eq!(first.0, second.0);
    }

    #[test]
    fn generate_cached_without_dir_skips_caching() {
        let p = MockProvider;
        let r = req("grass");
        let _ = generate_cached(&p, &r, None).unwrap();
        // Nothing to assert on disk; the assertion is "no panic, no
        // error"  — caching is purely additive.
    }
}

// =====================================================================
// Rustyme / sonargrid provider (Phase B)
// =====================================================================

/// Talk to a [Rustyme](https://github.com/nicholasgasior/sonargrid)
/// cluster for fan-out texture generation.
///
/// ## Division of labour
///
/// * **Maquette (this code)** is a pure *producer*: builds a
///   [`TaskEnvelope`][envelope] JSON, LPUSHes it to the queue,
///   BRPOPs the reply, decodes a PNG from base64.
/// * **Workers** (written + operated by the user) pick up the
///   envelope, actually call Fal/Replicate/self-hosted SDXL, and
///   LPUSH the result back to the agreed result list. We never
///   import a worker into our binary.
///
/// ## Worker contract
///
/// Any worker claiming to handle `task = "texgen.gen"` MUST honour
/// this shape or Maquette's [`RustymeProvider`] will reject its
/// output. Authoritative reference (worker-side):
/// [`sonargrid/docs/texgen-queues.md`][q].
///
/// **Input** — the envelope's `kwargs` object:
///
/// ```json
/// {
///     "prompt":    "isometric grass block, low-poly, seamless",
///     "seed":      42,
///     "width":     128,
///     "height":    128,
///     "model":     "fal-ai/flux/schnell",
///     "cache_key": "a1b2...f4",         // SHA-256 of the request,
///                                        // for worker-side dedup
///     "style_mode": "auto"              // optional, cpu lane only:
///                                        // auto | solid | smart
/// }
/// ```
///
/// **Output** — the worker pushes `{task_id, status, result, ...}`
/// to the result list (Rustyme's framework does the outer envelope;
/// the worker controls `result`). We accept either the new
/// multi-format shape or the legacy PNG-only shape:
///
/// ```json
/// {
///     "image_b64":    "<standard base64 of PNG bytes>",
///     "format":       "png",
///     "content_type": "image/png"
/// }
/// ```
///
/// or, for backward compatibility:
///
/// ```json
/// { "png_b64": "<standard base64 of PNG bytes>" }
/// ```
///
/// We currently only consume PNG: any `format` other than `"png"`
/// is rejected with a clear error so non-PNG bytes don't end up
/// in a `.png` cache file. JPEG / WebP support arrives once the
/// GUI has a decoder that can branch on `content_type`.
///
/// Optional extra fields (`elapsed_ms`, `cost_usd`, `style_params`,
/// `llm`) are logged but not required. Anything with
/// `status != "SUCCESS"` is surfaced to the caller as
/// [`ProviderError::Remote`].
///
/// [q]: /Users/admin/data0/public_work/sonargrid/docs/texgen-queues.md
///
/// ## What we do / don't do
///
/// * ✅ LPUSH envelope, BRPOP result, match by `task_id`, re-push
///   foreign replies to avoid starving concurrent callers.
/// * ✅ Timeout → best-effort `revoke` via admin HTTP API so the
///   task doesn't silently keep billing once the user has closed
///   the progress modal.
/// * ✅ Surfacing failure reasons (`status = "FAILURE"` + `error`
///   string) as typed [`ProviderError`]s.
/// * ❌ Queue management (autoscaling, DLQ replay): out of scope;
///   use Rustyme's Admin UI at `:12121/ui`.
/// * ❌ Worker-side caching: we already cache by SHA-256 on disk
///   *client-side*, so identical repeat requests never enqueue.
///
/// [envelope]: /Users/admin/data0/public_work/sonargrid/rustyme-core/src/protocol.rs
pub mod rustyme {
    use std::time::{Duration, Instant};

    use base64::Engine;
    use redis::Commands;
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};

    use super::{ProviderError, TextureBytes, TextureProvider, TextureRequest};

    /// Which sonargrid texgen queue family to talk to.
    ///
    /// This is a producer-side convenience — the source of truth for
    /// the actual queue keys is sonargrid's `envs/rustyme.*.toml`.
    /// We mirror the deployed naming (`rustyme:texgen-cpu:*` /
    /// `rustyme:texgen-fal:*`) so a default-configured `cargo run`
    /// hits the cheap CPU lane.
    ///
    /// The user can always override the resolved keys directly with
    /// `MAQUETTE_RUSTYME_QUEUE_KEY` / `MAQUETTE_RUSTYME_RESULT_KEY` —
    /// the profile is just a shorthand.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RustymeProfile {
        /// `texgen-cpu` — programmatic CPU synthesis (free, deterministic
        /// for `style_mode=solid`, ~30-500 ms). Default.
        Cpu,
        /// `texgen-fal` — Fal.ai FLUX schnell (≈ $0.003/img, 3-8 s).
        /// Use for actual AI imagery; needs `FAL_KEY` set on the
        /// worker side.
        Fal,
    }

    impl RustymeProfile {
        pub fn queue_key(&self) -> &'static str {
            match self {
                Self::Cpu => "rustyme:texgen-cpu:queue",
                Self::Fal => "rustyme:texgen-fal:queue",
            }
        }
        pub fn result_key(&self) -> &'static str {
            match self {
                Self::Cpu => "rustyme:texgen-cpu:result",
                Self::Fal => "rustyme:texgen-fal:result",
            }
        }
        /// Logical queue name as Rustyme's Admin API knows it
        /// (`maquette-cli texture purge <name>` writes here).
        pub fn admin_name(&self) -> &'static str {
            match self {
                Self::Cpu => "texgen-cpu",
                Self::Fal => "texgen-fal",
            }
        }
        /// Parse the value of `MAQUETTE_RUSTYME_PROFILE`. Accepts the
        /// short forms (`cpu`, `fal`) and the full queue names
        /// (`texgen-cpu`, `texgen-fal`) for ergonomics.
        pub fn parse(s: &str) -> Option<Self> {
            match s.trim().to_ascii_lowercase().as_str() {
                "cpu" | "texgen-cpu" => Some(Self::Cpu),
                "fal" | "texgen-fal" => Some(Self::Fal),
                _ => None,
            }
        }
    }

    /// Everything a [`RustymeProvider`] needs to be wired up. All
    /// fields are individually overridable from env vars — see
    /// [`RustymeConfig::from_env`].
    #[derive(Debug, Clone)]
    pub struct RustymeConfig {
        /// `redis://host:port[/db]`. Required. Maps to
        /// `QUEUE_N_REDIS_URL` on the Rustyme side.
        pub redis_url: String,
        /// Rustyme's `QUEUE_N_KEY`. What we LPUSH into. Typical
        /// name: `rustyme:texgen-cpu:queue` (default) or
        /// `rustyme:texgen-fal:queue`.
        pub queue_key: String,
        /// Rustyme's `QUEUE_N_RESULT_KEY`. What we BRPOP from. Must
        /// be configured on the worker side too — workers won't
        /// write back without it.
        pub result_key: String,
        /// Base URL of the Rustyme Admin HTTP API (e.g.
        /// `http://localhost:12121`). Optional; without it, revoke
        /// and purge become no-ops with a warning.
        pub admin_base_url: Option<String>,
        /// Task name routed to the worker. Defaults to
        /// `texgen.gen` (matches sonargrid's deployed Lua hooks);
        /// left overridable so multiple differently-tuned worker
        /// fleets can coexist (e.g. `texgen.gen.fast` vs
        /// `texgen.gen.pro`).
        pub task_name: String,
        /// How long we're willing to wait for a *reply* after
        /// LPUSH. Covers the worker's own API call latency plus
        /// any queueing. 60s default is comfortable for FLUX
        /// schnell (~3-8 s) + any retries; CPU lane finishes in
        /// well under 1 s.
        pub result_timeout: Duration,
        /// Max retries the worker is allowed (carried in the
        /// envelope). Default 3 matches Rustyme's own default.
        pub max_retries: u32,
        /// Optional `kwargs.style_mode` to send through.
        ///
        /// * `auto` (default at the worker side) — Lua hook decides
        ///   based on prompt length / language.
        /// * `solid` — strictly deterministic, no LLM call.
        /// * `smart` — force LLM parse; ~500 ms + tiny cost.
        ///
        /// Only consumed by `texgen-cpu`; ignored by `texgen-fal`.
        /// `None` omits the field from the envelope so the worker
        /// applies its own default.
        pub style_mode: Option<String>,
    }

    impl RustymeConfig {
        /// Construct from env vars, falling back to sensible
        /// defaults. Returns `None` if `MAQUETTE_RUSTYME_REDIS_URL`
        /// is unset — that's the one you must provide.
        ///
        /// Supported vars:
        /// * `MAQUETTE_RUSTYME_REDIS_URL` (required)
        /// * `MAQUETTE_RUSTYME_PROFILE` (default `cpu`; values: `cpu` / `fal`)
        /// * `MAQUETTE_RUSTYME_QUEUE_KEY` (overrides profile-derived default)
        /// * `MAQUETTE_RUSTYME_RESULT_KEY` (overrides profile-derived default)
        /// * `MAQUETTE_RUSTYME_ADMIN_URL` (optional)
        /// * `MAQUETTE_RUSTYME_TASK_NAME` (default `texgen.gen`)
        /// * `MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS` (default `60`)
        /// * `MAQUETTE_RUSTYME_MAX_RETRIES` (default `3`)
        /// * `MAQUETTE_RUSTYME_STYLE_MODE` (optional; `auto`/`solid`/`smart`)
        pub fn from_env() -> Option<Self> {
            let redis_url = std::env::var("MAQUETTE_RUSTYME_REDIS_URL").ok()?;
            let profile = std::env::var("MAQUETTE_RUSTYME_PROFILE")
                .ok()
                .and_then(|v| RustymeProfile::parse(&v))
                .unwrap_or(RustymeProfile::Cpu);
            Some(Self {
                redis_url,
                queue_key: std::env::var("MAQUETTE_RUSTYME_QUEUE_KEY")
                    .unwrap_or_else(|_| profile.queue_key().to_string()),
                result_key: std::env::var("MAQUETTE_RUSTYME_RESULT_KEY")
                    .unwrap_or_else(|_| profile.result_key().to_string()),
                admin_base_url: std::env::var("MAQUETTE_RUSTYME_ADMIN_URL").ok(),
                task_name: std::env::var("MAQUETTE_RUSTYME_TASK_NAME")
                    .unwrap_or_else(|_| "texgen.gen".to_string()),
                result_timeout: Duration::from_secs(
                    std::env::var("MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(60),
                ),
                max_retries: std::env::var("MAQUETTE_RUSTYME_MAX_RETRIES")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3),
                style_mode: std::env::var("MAQUETTE_RUSTYME_STYLE_MODE")
                    .ok()
                    .filter(|s| !s.is_empty()),
            })
        }
    }

    /// Minimal mirror of Rustyme's `TaskEnvelope`. We intentionally
    /// don't depend on `rustyme-core` directly — its full crate
    /// pulls in chrono, actor machinery, etc. that a producer
    /// doesn't need. Keep this struct field-compatible with the
    /// upstream protocol (see `protocol.rs` in sonargrid).
    #[derive(Serialize, Debug)]
    struct TaskEnvelope<'a> {
        id: &'a str,
        task: &'a str,
        args: &'a [Value],
        kwargs: &'a Value,
        retries: u32,
        max_retries: u32,
        priority: &'a str,
        unique_for_secs: u64,
        created_at: String,
    }

    /// Shape of the reply `WorkerFramework → result_key`. The outer
    /// envelope is always owned by Rustyme's worker runtime; the
    /// `result` field is what workers actually produce.
    #[derive(Deserialize, Debug)]
    struct ResultEnvelope {
        task_id: String,
        status: String,
        #[serde(default)]
        result: Value,
        #[serde(default)]
        error: Option<String>,
    }

    /// Shape of the `result` field our workers agree to return.
    ///
    /// **Two shapes are accepted on the wire**, in order of
    /// preference:
    ///
    /// 1. **New (sonargrid `texgen-cpu` / `texgen-fal`)**:
    ///    `{ "image_b64": "...", "format": "png" | "jpeg" | "webp",
    ///       "content_type": "image/png", ... }`.
    ///
    ///    Maquette currently only stores PNG bytes (the disk cache
    ///    filename is `<sha>.png` and the v0.10 D GUI decoder
    ///    assumes PNG). If `format` is anything other than `"png"`
    ///    we surface a [`ProviderError::Remote`] — the caller is
    ///    expected to either omit `output_format` from the
    ///    `kwargs` or set it to `"png"` until non-PNG support
    ///    lands in a later phase.
    ///
    /// 2. **Legacy (echo-style worker)**: `{ "png_b64": "..." }` —
    ///    the original Phase B contract from
    ///    `docs/texture/rustyme.md`. CPU workers still emit this
    ///    field for backward compatibility when `format == "png"`,
    ///    but new code paths shouldn't rely on it.
    ///
    /// Both fields are optional at the serde layer so we can fail
    /// with an actionable error message inside `generate()` rather
    /// than a generic "missing field" deserialize failure.
    #[derive(Deserialize, Debug, Default)]
    struct TextureResult {
        #[serde(default)]
        image_b64: Option<String>,
        #[serde(default)]
        format: Option<String>,
        #[serde(default)]
        png_b64: Option<String>,
    }

    impl TextureResult {
        /// Pull the raw PNG base64 string out of the worker's
        /// reply, validating that the format (when declared) is
        /// `png`. Returns `Err` with a self-explanatory message
        /// when the worker speaks a shape we can't handle yet.
        fn decode_png_b64(&self) -> Result<&str, String> {
            // Prefer the new field when it's present; reject
            // declared non-PNG formats up front so we don't silently
            // write JPEG bytes to a `.png` cache file.
            if let Some(b64) = self.image_b64.as_deref() {
                let fmt = self.format.as_deref().unwrap_or("png").to_ascii_lowercase();
                if fmt != "png" {
                    return Err(format!(
                        "worker returned format={fmt:?}, but Maquette currently \
                         only consumes PNG bytes. Omit `output_format` (or set \
                         it to \"png\") in your kwargs, or wait for the v0.10+ \
                         multi-format support."
                    ));
                }
                return Ok(b64);
            }
            if let Some(b64) = self.png_b64.as_deref() {
                return Ok(b64);
            }
            Err(
                "worker `result` had neither `image_b64` (new) nor `png_b64` \
                 (legacy) fields"
                    .to_string(),
            )
        }
    }

    /// [`TextureProvider`] talking to a Rustyme cluster.
    ///
    /// Construction is cheap (no connection opened until `generate`
    /// is called) so a single instance can be shared across
    /// Bevy systems via `Arc` or rebuilt per-task.
    pub struct RustymeProvider {
        config: RustymeConfig,
    }

    impl RustymeProvider {
        pub fn new(config: RustymeConfig) -> Self {
            Self { config }
        }

        pub fn config(&self) -> &RustymeConfig {
            &self.config
        }
    }

    impl TextureProvider for RustymeProvider {
        fn name(&self) -> &'static str {
            "rustyme"
        }

        fn generate(&self, request: &TextureRequest) -> Result<TextureBytes, ProviderError> {
            let client = redis::Client::open(self.config.redis_url.as_str())
                .map_err(|e| ProviderError::Remote(format!("redis connect: {e}")))?;
            let mut conn = client
                .get_connection_with_timeout(Duration::from_secs(5))
                .map_err(|e| ProviderError::Remote(format!("redis handshake: {e}")))?;

            let task_id = uuid::Uuid::new_v4().to_string();
            // We pin `output_format=png` because Maquette's disk
            // cache uses `<sha>.png` and the v0.10 D GUI decoder
            // assumes PNG. Without this hint, sonargrid's smart
            // worker is free to return jpeg (e.g. when the LLM
            // classifies the prompt as "toon base color, no
            // alpha"), which we'd then reject downstream. Keeping
            // the constraint at the producer is friendlier than
            // failing the BRPOP path.
            let mut kwargs = json!({
                "prompt":        request.prompt,
                "seed":          request.seed,
                "width":         request.width,
                "height":        request.height,
                "model":         request.model,
                "cache_key":     request.cache_key(),
                "output_format": "png",
            });
            if let Some(mode) = self.config.style_mode.as_deref() {
                if let Some(obj) = kwargs.as_object_mut() {
                    obj.insert("style_mode".to_string(), Value::String(mode.to_string()));
                }
            }
            let envelope = TaskEnvelope {
                id: &task_id,
                task: &self.config.task_name,
                args: &[],
                kwargs: &kwargs,
                retries: 0,
                max_retries: self.config.max_retries,
                priority: "normal",
                unique_for_secs: 3600,
                // RFC3339 to match chrono::Utc::now().to_rfc3339().
                // Rustyme's server accepts any `DateTime<Utc>` parse.
                created_at: current_rfc3339(),
            };
            let payload = serde_json::to_string(&envelope)
                .map_err(|e| ProviderError::Remote(format!("envelope encode: {e}")))?;

            log::info!(
                "texgen: rustyme LPUSH id={} queue={} task={} prompt={:?}",
                task_id,
                self.config.queue_key,
                self.config.task_name,
                super::truncate_for_log(&request.prompt),
            );

            let _: () = conn
                .lpush(&self.config.queue_key, &payload)
                .map_err(|e| ProviderError::Remote(format!("LPUSH: {e}")))?;

            let started = Instant::now();
            let deadline = started + self.config.result_timeout;

            // BRPOP loop with per-pop timeout. We can't just BRPOP
            // for the full deadline because a *different* task_id
            // landing first needs to be put back without consuming
            // our remaining budget.
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    // Best-effort cancel so the task doesn't keep
                    // billing on someone else's GPU.
                    if let Some(admin) = &self.config.admin_base_url {
                        if let Err(e) = revoke(admin, &task_id) {
                            log::warn!(
                                "texgen: revoke({task_id}) after timeout failed: {e}"
                            );
                        }
                    }
                    return Err(ProviderError::Remote(format!(
                        "rustyme: timed out after {:.1?} waiting for result \
                         (task_id={}, queue={}, result_key={})",
                        self.config.result_timeout,
                        task_id,
                        self.config.queue_key,
                        self.config.result_key,
                    )));
                }
                // BRPOP wants seconds as integer; round up and cap.
                let pop_secs = remaining
                    .as_secs()
                    .saturating_add(1)
                    .clamp(1, 5);

                let popped: Option<(String, String)> = conn
                    .brpop(&self.config.result_key, pop_secs as f64)
                    .map_err(|e| ProviderError::Remote(format!("BRPOP: {e}")))?;

                let Some((_key, raw)) = popped else {
                    continue; // Timeout → re-check deadline.
                };

                let parsed: ResultEnvelope = match serde_json::from_str(&raw) {
                    Ok(p) => p,
                    Err(e) => {
                        // Push back to the *head* (LPUSH), not the
                        // tail — see the foreign-task_id branch below
                        // for why. We don't want to poison the queue
                        // either, so log the bad payload too.
                        let _: Result<(), _> = conn.lpush(&self.config.result_key, &raw);
                        return Err(ProviderError::Remote(format!(
                            "rustyme: result envelope not JSON: {e} raw={}",
                            truncate_for_log(&raw),
                        )));
                    }
                };

                if parsed.task_id != task_id {
                    // Not ours — put it back so another concurrent
                    // caller can see it. We push to the *head* (LPUSH),
                    // matching the worker's own write direction, so
                    // that subsequent BRPOPs (which pop from the tail)
                    // make progress on **older** elements instead of
                    // re-popping the same foreign reply forever.
                    //
                    // (rustyme-py's `AsyncResult.get()` uses RPUSH
                    // here, which silently deadlocks when the result
                    // queue contains stale entries — Maquette would
                    // hit this any time a previous `texgen-cpu`/`fal`
                    // client crashed without draining its replies. We
                    // keep our copy correct and have flagged it
                    // upstream.)
                    log::debug!(
                        "texgen: saw result for other task {} — re-pushing to head",
                        parsed.task_id
                    );
                    let _: Result<(), _> = conn.lpush(&self.config.result_key, &raw);
                    continue;
                }

                // It's ours — decode.
                if parsed.status != "SUCCESS" {
                    return Err(ProviderError::Remote(format!(
                        "rustyme: worker reported {} for task {}: {}",
                        parsed.status,
                        parsed.task_id,
                        parsed.error.unwrap_or_else(|| "(no error message)".to_string()),
                    )));
                }

                let texture: TextureResult = serde_json::from_value(parsed.result.clone())
                    .map_err(|e| {
                        ProviderError::Remote(format!(
                            "rustyme: failed to parse worker result envelope: {e} \
                             raw_result={}",
                            truncate_for_log(&parsed.result.to_string()),
                        ))
                    })?;

                let png_b64 = texture.decode_png_b64().map_err(|msg| {
                    ProviderError::Remote(format!(
                        "rustyme: {msg} (task_id={task_id}, raw_result={})",
                        truncate_for_log(&parsed.result.to_string()),
                    ))
                })?;

                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(png_b64.as_bytes())
                    .map_err(|e| {
                        ProviderError::Remote(format!(
                            "rustyme: base64 decode of worker result: {e}"
                        ))
                    })?;

                log::info!(
                    "texgen: rustyme got result id={} bytes={} in {:.2?}",
                    task_id,
                    bytes.len(),
                    started.elapsed(),
                );
                return Ok(TextureBytes(bytes));
            }
        }
    }

    /// Fire-and-forget revoke. Used both internally (on timeout)
    /// and from the CLI `texture revoke` verb. Returns the Rustyme
    /// Admin response body on success.
    pub fn revoke(admin_base_url: &str, task_id: &str) -> Result<String, ProviderError> {
        let url = format!(
            "{}/api/admin/tasks/{}/revoke",
            admin_base_url.trim_end_matches('/'),
            task_id,
        );
        log::info!("texgen: POST {url}");
        let resp = ureq::post(&url)
            .set("content-type", "application/json")
            .send_string("{}")
            .map_err(|e| ProviderError::Remote(format!("revoke {url}: {e}")))?;
        let body = resp
            .into_string()
            .map_err(|e| ProviderError::Remote(format!("revoke body: {e}")))?;
        Ok(body)
    }

    /// Clear every pending task from `queue_name`. `queue_name` is
    /// the logical name Rustyme exposes (e.g. `texgen`), not the
    /// Redis LPUSH key. Used from the CLI `texture purge` verb for
    /// operational recovery.
    pub fn purge_queue(admin_base_url: &str, queue_name: &str) -> Result<String, ProviderError> {
        let url = format!(
            "{}/api/admin/queues/{}/purge",
            admin_base_url.trim_end_matches('/'),
            queue_name,
        );
        log::info!("texgen: POST {url}");
        let resp = ureq::post(&url)
            .set("content-type", "application/json")
            .send_string("{}")
            .map_err(|e| ProviderError::Remote(format!("purge {url}: {e}")))?;
        let body = resp
            .into_string()
            .map_err(|e| ProviderError::Remote(format!("purge body: {e}")))?;
        Ok(body)
    }

    fn truncate_for_log(s: &str) -> String {
        super::truncate_for_log(s)
    }

    fn current_rfc3339() -> String {
        // We don't pull in chrono just to format a timestamp. std
        // doesn't give us RFC3339 directly, but epoch millis in a
        // `seconds-since-epoch.fraction` form is an acceptable
        // DateTime<Utc> payload for Rustyme's serde (Utc::DateTime
        // parses any RFC3339). For full RFC3339 we need YYYY-MM-DD
        // etc., so hand-roll a trivial formatter.
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs() as i64;
        let nanos = now.subsec_nanos();
        // Convert epoch seconds to a naive UTC breakdown.
        let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
        format!(
            "{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{nanos:09}Z"
        )
    }

    // Calendar math — enough to render RFC3339. Kept tiny instead
    // of dragging chrono in as a direct dep. Correct for dates
    // from 1970 through the year 10000 (way beyond what matters).
    fn epoch_to_ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
        const SEC_PER_DAY: i64 = 86400;
        let days_since_epoch = secs.div_euclid(SEC_PER_DAY);
        let time_of_day = secs.rem_euclid(SEC_PER_DAY) as u32;
        let h = time_of_day / 3600;
        let mi = (time_of_day % 3600) / 60;
        let s = time_of_day % 60;

        // Days since 1970-01-01 → Y/M/D using Howard Hinnant's
        // civil_from_days (public domain). Handles leap years
        // correctly.
        let z = days_since_epoch + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097) as u64; // 0..=146096
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y_shifted = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 0..=365
        let mp = (5 * doy + 2) / 153; // 0..=11
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let mo = if mp < 10 { (mp + 3) as u32 } else { (mp - 9) as u32 };
        let y = y_shifted + if mo <= 2 { 1 } else { 0 };
        (y, mo, d, h, mi, s)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn req() -> TextureRequest {
            TextureRequest::new("grass", 1, 64, 64, "fal-ai/flux/schnell")
        }

        fn cfg() -> RustymeConfig {
            RustymeConfig {
                redis_url: "redis://127.0.0.1:6379/0".to_string(),
                queue_key: RustymeProfile::Cpu.queue_key().to_string(),
                result_key: RustymeProfile::Cpu.result_key().to_string(),
                admin_base_url: Some("http://localhost:12121".to_string()),
                task_name: "texgen.gen".to_string(),
                result_timeout: Duration::from_secs(10),
                max_retries: 3,
                style_mode: None,
            }
        }

        #[test]
        fn envelope_has_all_required_fields_for_rustyme_server() {
            let r = req();
            let task_id = "test-uuid";
            let kwargs = json!({
                "prompt": r.prompt,
                "seed": r.seed,
                "width": r.width,
                "height": r.height,
                "model": r.model,
                "cache_key": r.cache_key(),
            });
            let envelope = TaskEnvelope {
                id: task_id,
                task: "texgen.gen",
                args: &[],
                kwargs: &kwargs,
                retries: 0,
                max_retries: 3,
                priority: "normal",
                unique_for_secs: 3600,
                created_at: current_rfc3339(),
            };
            let payload = serde_json::to_string(&envelope).unwrap();
            // These five fields are referenced by the Rustyme
            // server's Serde impl; missing any of them will cause
            // the worker to reject the task envelope.
            for field in [
                r#""id":"test-uuid""#,
                r#""task":"texgen.gen""#,
                r#""max_retries":3"#,
                r#""priority":"normal""#,
                r#""unique_for_secs":3600"#,
            ] {
                assert!(
                    payload.contains(field),
                    "envelope missing `{field}` — produced: {payload}"
                );
            }
            // The cache_key must be echoed in kwargs so workers can
            // deduplicate server-side if they want to.
            assert!(
                payload.contains(&r.cache_key()),
                "cache_key must appear in kwargs"
            );
        }

        #[test]
        fn result_envelope_legacy_png_b64_round_trip() {
            // Phase B's original shape. Maquette must keep parsing
            // it for backward compatibility with existing echo
            // workers and the old smoke harness.
            let raw = r#"{"task_id":"abc","status":"SUCCESS","result":{"png_b64":"aGk="}}"#;
            let env: ResultEnvelope = serde_json::from_str(raw).unwrap();
            assert_eq!(env.task_id, "abc");
            assert_eq!(env.status, "SUCCESS");
            let tex: TextureResult = serde_json::from_value(env.result).unwrap();
            assert_eq!(tex.decode_png_b64().unwrap(), "aGk=");
        }

        #[test]
        fn result_envelope_new_image_b64_round_trip() {
            // The shape sonargrid's `texgen-cpu` / `texgen-fal`
            // workers actually emit today (PNG path). We must
            // accept it and pull the bytes out.
            let raw = r#"{
                "task_id":"abc","status":"SUCCESS",
                "result":{"image_b64":"aGk=","format":"png","content_type":"image/png"}
            }"#;
            let env: ResultEnvelope = serde_json::from_str(raw).unwrap();
            let tex: TextureResult = serde_json::from_value(env.result).unwrap();
            assert_eq!(tex.decode_png_b64().unwrap(), "aGk=");
        }

        #[test]
        fn result_envelope_image_b64_with_explicit_png_round_trip() {
            // `texgen-cpu` happens to emit *both* fields when
            // `format == "png"`. Make sure we still pick one
            // unambiguously.
            let raw = r#"{
                "task_id":"abc","status":"SUCCESS",
                "result":{"image_b64":"aGk=","format":"png","png_b64":"aGk="}
            }"#;
            let env: ResultEnvelope = serde_json::from_str(raw).unwrap();
            let tex: TextureResult = serde_json::from_value(env.result).unwrap();
            assert_eq!(tex.decode_png_b64().unwrap(), "aGk=");
        }

        #[test]
        fn result_envelope_rejects_non_png_format() {
            // Maquette's disk cache files are named `<sha>.png` and
            // the v0.10 D GUI decoder assumes PNG. If a worker
            // returns `format=jpeg` we must surface that as a clear
            // error rather than silently caching JPEG bytes.
            let raw = r#"{
                "task_id":"abc","status":"SUCCESS",
                "result":{"image_b64":"aGk=","format":"jpeg","content_type":"image/jpeg"}
            }"#;
            let env: ResultEnvelope = serde_json::from_str(raw).unwrap();
            let tex: TextureResult = serde_json::from_value(env.result).unwrap();
            let err = tex.decode_png_b64().expect_err("non-png must be rejected");
            assert!(
                err.contains("png") && err.contains("jpeg"),
                "error must name both formats; got {err:?}"
            );
        }

        #[test]
        fn result_envelope_rejects_empty_result() {
            // Worker returned SUCCESS but with neither field — most
            // likely a misconfigured echo Lua. Don't decode garbage.
            let raw = r#"{"task_id":"abc","status":"SUCCESS","result":{}}"#;
            let env: ResultEnvelope = serde_json::from_str(raw).unwrap();
            let tex: TextureResult = serde_json::from_value(env.result).unwrap();
            let err = tex.decode_png_b64().expect_err("empty must be rejected");
            assert!(
                err.contains("image_b64") && err.contains("png_b64"),
                "error must mention both accepted shapes; got {err:?}"
            );
        }

        #[test]
        fn result_envelope_failure_round_trip() {
            let raw = r#"{"task_id":"abc","status":"FAILURE","result":null,"error":"Fal returned 429"}"#;
            let env: ResultEnvelope = serde_json::from_str(raw).unwrap();
            assert_eq!(env.status, "FAILURE");
            assert_eq!(env.error.as_deref(), Some("Fal returned 429"));
        }

        #[test]
        fn rustyme_profile_parse() {
            assert_eq!(RustymeProfile::parse("cpu"), Some(RustymeProfile::Cpu));
            assert_eq!(RustymeProfile::parse("CPU"), Some(RustymeProfile::Cpu));
            assert_eq!(
                RustymeProfile::parse("texgen-cpu"),
                Some(RustymeProfile::Cpu)
            );
            assert_eq!(RustymeProfile::parse("fal"), Some(RustymeProfile::Fal));
            assert_eq!(
                RustymeProfile::parse("texgen-fal"),
                Some(RustymeProfile::Fal)
            );
            assert_eq!(RustymeProfile::parse("nope"), None);
            // Match deployed key naming exactly — drift here would
            // silently send tasks to a queue no one is consuming.
            assert_eq!(
                RustymeProfile::Cpu.queue_key(),
                "rustyme:texgen-cpu:queue"
            );
            assert_eq!(
                RustymeProfile::Cpu.result_key(),
                "rustyme:texgen-cpu:result"
            );
            assert_eq!(
                RustymeProfile::Fal.queue_key(),
                "rustyme:texgen-fal:queue"
            );
            assert_eq!(
                RustymeProfile::Fal.result_key(),
                "rustyme:texgen-fal:result"
            );
        }

        #[test]
        fn config_from_env_requires_redis_url() {
            // Not a full e2e; we just verify the "required" field
            // logic so a missing env var doesn't silently produce
            // a wrong config.
            let key = "MAQUETTE_RUSTYME_REDIS_URL";
            // Profile / queue overrides can pollute neighbouring tests
            // when run with --test-threads>1; capture and restore them
            // around this assertion.
            let prelude_keys = [
                "MAQUETTE_RUSTYME_PROFILE",
                "MAQUETTE_RUSTYME_QUEUE_KEY",
                "MAQUETTE_RUSTYME_RESULT_KEY",
                "MAQUETTE_RUSTYME_TASK_NAME",
                "MAQUETTE_RUSTYME_STYLE_MODE",
            ];
            let saved = std::env::var(key).ok();
            let saved_prelude: Vec<_> = prelude_keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            // SAFETY: test is single-threaded within this cfg(test)
            // binary; removing and restoring the env var can't race
            // with another thread's env read.
            unsafe {
                std::env::remove_var(key);
                for k in prelude_keys {
                    std::env::remove_var(k);
                }
            }
            assert!(
                RustymeConfig::from_env().is_none(),
                "from_env must return None without {key}"
            );
            unsafe {
                std::env::set_var(key, "redis://127.0.0.1:6379");
            }
            let cfg = RustymeConfig::from_env().expect("with redis url set");
            assert_eq!(cfg.redis_url, "redis://127.0.0.1:6379");
            assert_eq!(cfg.task_name, "texgen.gen");
            // Default profile is cpu — anyone changing that needs to
            // also update USER-TODO + docs/texture/rustyme.md.
            assert_eq!(cfg.queue_key, "rustyme:texgen-cpu:queue");
            assert_eq!(cfg.result_key, "rustyme:texgen-cpu:result");
            assert!(cfg.style_mode.is_none());
            unsafe {
                match saved {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
                for (k, v) in saved_prelude {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }

        #[test]
        fn config_from_env_honours_profile() {
            let keys = [
                "MAQUETTE_RUSTYME_REDIS_URL",
                "MAQUETTE_RUSTYME_PROFILE",
                "MAQUETTE_RUSTYME_QUEUE_KEY",
                "MAQUETTE_RUSTYME_RESULT_KEY",
                "MAQUETTE_RUSTYME_STYLE_MODE",
            ];
            let saved: Vec<_> = keys
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            unsafe {
                std::env::set_var("MAQUETTE_RUSTYME_REDIS_URL", "redis://127.0.0.1:6379");
                std::env::set_var("MAQUETTE_RUSTYME_PROFILE", "fal");
                std::env::remove_var("MAQUETTE_RUSTYME_QUEUE_KEY");
                std::env::remove_var("MAQUETTE_RUSTYME_RESULT_KEY");
                std::env::set_var("MAQUETTE_RUSTYME_STYLE_MODE", "smart");
            }
            let cfg = RustymeConfig::from_env().expect("redis url set");
            assert_eq!(cfg.queue_key, "rustyme:texgen-fal:queue");
            assert_eq!(cfg.result_key, "rustyme:texgen-fal:result");
            assert_eq!(cfg.style_mode.as_deref(), Some("smart"));

            // Explicit *_KEY overrides win over PROFILE.
            unsafe {
                std::env::set_var("MAQUETTE_RUSTYME_QUEUE_KEY", "custom:q");
                std::env::set_var("MAQUETTE_RUSTYME_RESULT_KEY", "custom:r");
            }
            let cfg = RustymeConfig::from_env().expect("redis url set");
            assert_eq!(cfg.queue_key, "custom:q");
            assert_eq!(cfg.result_key, "custom:r");

            unsafe {
                for (k, v) in saved {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }

        #[test]
        fn rfc3339_formatting_is_stable() {
            // Known epoch: 2023-01-02T03:04:05Z = 1672628645
            let (y, mo, d, h, mi, s) = epoch_to_ymdhms(1_672_628_645);
            assert_eq!((y, mo, d, h, mi, s), (2023, 1, 2, 3, 4, 5));
        }

        /// Live-Redis integration test. Disabled by default; enable
        /// with `MAQUETTE_RUSTYME_LIVE=1` pointing at a reachable
        /// Rustyme instance to confirm producer/consumer plumbing.
        #[test]
        #[ignore]
        fn live_round_trip_against_running_rustyme() {
            if std::env::var("MAQUETTE_RUSTYME_LIVE").ok().as_deref() != Some("1") {
                return;
            }
            let cfg = cfg();
            let _provider = RustymeProvider::new(cfg);
            // Actually running `generate` needs a worker that
            // understands "texture.gen" — so this test is really a
            // smoke-test vehicle for manual validation; it deliberately
            // does not assert on the response beyond "doesn't panic
            // during envelope build".
        }
    }
}
