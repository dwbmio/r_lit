//! Plug-in surface for game-specific protocol parsers.
//!
//! Each game's binary message format lives in its own crate
//! (`crates/proto-puzzle`, `crates/proto-bubble`, …). Those crates
//! implement [`ProtocolParser`] for their own type and register a
//! [`ProtocolDescriptor`] with [`inventory::submit!`]. A consuming
//! application (typically `apps/gamereel-cli`) iterates the registered
//! descriptors at startup with [`registered_protocols`] — adding a new
//! game requires zero edits to the core crate or the CLI.
//!
//! Why `inventory` and not Cargo features:
//!   * Adding a new proto-* crate is one `[dependencies]` line in the
//!     CLI; no #[cfg] cascade through the codebase.
//!   * Iteration order is link-time deterministic per OS/linker, but
//!     [`registered_protocols`] returns names sorted alphabetically so
//!     downstream code (CLI listing, tests) is fully stable.
//!
//! ⚠ `inventory` relies on link-time constructors. In our workspace we
//! ship the `release` profile with `lto = "fat"`. The crate's docs
//! describe the LTO interaction; we have a regression test
//! ([`tests/protocol_inventory.rs`]) that fires under `--release` so a
//! future LTO change that strips constructors fails CI loudly rather
//! than silently making the registry empty.

use crate::error::GamereelError;

/// What every game-specific protocol parser must implement.
///
/// `Send + Sync` so a single parser instance can be shared across the
/// M5 actor pool's worker threads (each worker holds an `Arc<dyn
/// ProtocolParser>`).
pub trait ProtocolParser: Send + Sync {
    /// Stable identifier shown to operators (CLI `--protocol <name>`,
    /// log lines, telemetry). Must be unique across linked crates.
    fn name(&self) -> &'static str;

    /// One-line human-readable summary; shown by `gamereel-cli list-protocols`.
    fn description(&self) -> &'static str { "" }

    /// Parse a raw binary message blob into something the rendering
    /// pipeline can drive. Wired up to `Scene` in a follow-up commit;
    /// for the bring-up of M5 the trait stays small and we extend it
    /// once the first real game protocol lands.
    fn parse(&self, msg: &[u8]) -> Result<ParsedReplay, GamereelError>;
}

/// Bag of "what the renderer needs" produced by a parser. Intentionally
/// minimal at this stage — we'll grow this struct as proto-puzzle and
/// proto-bubble teach us what data shapes are common.
#[derive(Debug, Default)]
pub struct ParsedReplay {
    /// Where the rendered video should live (relative to a caller-chosen
    /// output root). Filename only — extension is added by the renderer.
    pub suggested_filename: String,
    /// Raw decoded fields the parser wants to expose verbatim. Stable
    /// JSON-serializable type makes it cheap to log + diff in tests.
    pub metadata: serde_json::Value,
    /// Number of frames the eventual video should have. Lets the CLI
    /// warn if a parser claims a 90-minute video by mistake.
    pub frames: u32,
}

/// Compile-time descriptor used by `inventory::submit!`. Each proto-*
/// crate constructs one of these and registers it; the core never
/// instantiates parsers directly.
pub struct ProtocolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    /// Factory: returns a fresh boxed parser. Closures register parsers
    /// without forcing every parser type to be `Default`.
    pub factory: fn() -> Box<dyn ProtocolParser>,
}

inventory::collect!(ProtocolDescriptor);

/// Public iterator the CLI uses at startup. Returns descriptors sorted
/// by `name` for deterministic listing.
pub fn registered_protocols() -> Vec<&'static ProtocolDescriptor> {
    let mut v: Vec<&'static ProtocolDescriptor> =
        inventory::iter::<ProtocolDescriptor>().collect();
    v.sort_by_key(|d| d.name);
    v
}

/// Finds a registered protocol by name, returns its freshly-constructed
/// parser. The Box is owned by the caller; multiple instances are fine
/// (each is cheap — parsers should hold no per-message state).
pub fn build_parser(name: &str) -> Result<Box<dyn ProtocolParser>, GamereelError> {
    for desc in inventory::iter::<ProtocolDescriptor>() {
        if desc.name == name {
            return Ok((desc.factory)());
        }
    }
    Err(GamereelError::CustomError(format!(
        "no parser registered for protocol '{name}' — known: {:?}",
        registered_protocols().iter().map(|d| d.name).collect::<Vec<_>>()
    )))
}
