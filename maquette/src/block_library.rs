//! Block-library UI state + sync plumbing (GUI binary).
//!
//! Sits on top of the headless [`maquette::block_meta`] module: hosts a
//! Bevy resource that mirrors the merged block list (LocalProvider +
//! cached HfrogProvider), exposes a `Sync` action that re-fetches from
//! hfrog on a background task, and dispatches `BlockBinding` events
//! from the UI right-click menu / library panel down to the
//! `Palette::set_block_id` setter.
//!
//! Why a separate module (not in `ui.rs`):
//!
//! * `ui.rs` is already 1800+ lines and the block-library code wants
//!   its own resource + a couple of systems + an async task type.
//!   Folding it into the canvas/palette panel would push that file
//!   past readability.
//! * The library state is project-independent (closing a project
//!   doesn't reset the library). Putting it on its own resource
//!   makes the lifetime obvious.
//!
//! Headless invariant: this whole module is GUI-only. The lib's
//! `block_meta` is the single source of truth — we hold cached
//! `BlockMeta` records here and never reimplement provider logic.

use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy::window::RequestRedraw;

use maquette::block_meta::{
    self,
    hfrog::{HfrogConfig, HfrogProvider},
    BlockMeta, BlockMetaError, BlockMetaProvider, BlockMetaSource, LocalProvider,
};
use maquette::grid::Palette;

use crate::history::EditHistory;
use crate::notify::Toasts;
use crate::session::CurrentProject;

/// In-memory block catalog. Refreshed at startup from the cache and
/// from `LocalProvider::blocks`, then again whenever the user clicks
/// `Sync hfrog`. The order is fixed-by-id so the UI doesn't reflow
/// across refreshes.
#[derive(Resource, Default, Clone)]
pub struct BlockLibraryState {
    pub blocks: Vec<BlockMeta>,
    /// `Some(error_string)` if the latest `Sync` failed; the toast
    /// bubble surfaces it but the resource keeps it for the panel
    /// header to dim.
    pub last_error: Option<String>,
    /// Track of the in-flight sync task so the panel can show a
    /// spinner instead of letting the user spam the button.
    in_flight: bool,
}

impl BlockLibraryState {
    /// Look up a block by id. Linear scan — the catalog is at most
    /// a few hundred entries; a HashMap would just shift work onto
    /// the resource ↔ UI sync path.
    ///
    /// Used by D-1's texgen prompt-derivation path (resolves a
    /// slot's `block_id` → `BlockMeta` to feed
    /// [`maquette::texture_meta::derive_texture_prompt`]). The
    /// current panel UI navigates `library.blocks` directly so it
    /// hasn't called this yet — keeping it #allow(dead_code) until
    /// the generate wiring lands.
    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&BlockMeta> {
        self.blocks.iter().find(|b| b.id == id)
    }
    pub fn is_in_flight(&self) -> bool {
        self.in_flight
    }
}

/// User-initiated changes to a slot's `block_id` binding. The UI
/// emits one of these per click on Bind / Unbind; a system applies
/// the mutation through `Palette::set_block_id` so the lib's
/// undo-friendly setter stays the single source of mutation truth.
#[derive(Message, Clone, Debug)]
pub enum BlockBindAction {
    /// Bind `block_id` to `slot`.
    Bind { slot: u8, block_id: String },
    /// Clear the binding.
    Unbind { slot: u8 },
}

/// Trigger a hfrog sync from the UI.
#[derive(Message, Clone, Debug, Default)]
pub struct SyncBlockLibrary;

/// Background sync result delivered to the main thread.
#[derive(Component)]
struct PendingSync {
    task: Task<Result<Vec<BlockMeta>, String>>,
}

pub struct BlockLibraryPlugin;

impl Plugin for BlockLibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockLibraryState>()
            .add_message::<BlockBindAction>()
            .add_message::<SyncBlockLibrary>()
            .add_systems(Startup, populate_initial_library)
            .add_systems(
                Update,
                (
                    handle_bind_action,
                    handle_sync_request,
                    poll_pending_sync,
                ),
            );
    }
}

/// On boot, populate the library from the bundled local provider plus
/// whatever's already in `~/.cache/maquette/blocks/hfrog/.../*.json`.
/// Does not hit the network — the user has to click `Sync` for that.
fn populate_initial_library(mut library: ResMut<BlockLibraryState>) {
    let mut blocks: Vec<BlockMeta> = LocalProvider::new().list().unwrap_or_default();

    // Read the hfrog disk cache only — `Provider::list()` would do
    // a network fallback if the cache is empty, and Startup is the
    // wrong moment for that.
    let hfrog_cfg = HfrogConfig::from_env();
    if let Some(dir) = block_meta::default_cache_dir() {
        match block_meta::cache_list(&dir, "hfrog", &hfrog_cfg.runtime) {
            Ok(cached) if !cached.is_empty() => {
                let cached_ids: std::collections::HashSet<String> =
                    cached.iter().map(|b| b.id.clone()).collect();
                blocks.retain(|b| !cached_ids.contains(&b.id));
                blocks.extend(cached);
            }
            Ok(_) => {} // empty cache, that's fine
            Err(e) => log::warn!("block_library: cache_list failed: {e}"),
        }
    }
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    log::info!(
        "block_library: initial library populated with {} blocks",
        blocks.len()
    );
    library.blocks = blocks;
}

fn handle_bind_action(
    mut actions: MessageReader<BlockBindAction>,
    mut palette: ResMut<Palette>,
    mut current: ResMut<CurrentProject>,
    mut history: ResMut<EditHistory>,
    mut toasts: ResMut<Toasts>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut any = false;
    for action in actions.read() {
        any = true;
        match action {
            BlockBindAction::Bind { slot, block_id } => {
                let prev = palette.set_block_id(*slot, Some(block_id.clone()));
                match prev {
                    Some(prev_id) => {
                        log::info!(
                            "block_library: bind slot={} id={} (prev={:?})",
                            slot,
                            block_id,
                            prev_id
                        );
                        // Closing the open stroke (if any) so this
                        // edit is its own undo step. The actual
                        // undo-event extension for block bindings
                        // is part of v0.10 D-1's EditHistory
                        // generalisation; for now we end_stroke()
                        // so a Ctrl+Z at least doesn't fold the
                        // bind into a paint stroke.
                        history.end_stroke();
                        current.mark_dirty();
                        toasts.info(format!("slot #{} bound to {}", slot, block_id));
                    }
                    None => toasts.error(format!(
                        "slot #{} out of range (palette has {} slots)",
                        slot,
                        palette.colors.len()
                    )),
                }
            }
            BlockBindAction::Unbind { slot } => {
                let prev = palette.set_block_id(*slot, None);
                if let Some(Some(prev_id)) = prev {
                    log::info!("block_library: unbind slot={} (was {})", slot, prev_id);
                    history.end_stroke();
                    current.mark_dirty();
                    toasts.info(format!("slot #{} unbound (was {})", slot, prev_id));
                }
            }
        }
    }
    if any {
        redraw.write(RequestRedraw);
    }
}

fn handle_sync_request(
    mut requests: MessageReader<SyncBlockLibrary>,
    mut commands: Commands,
    mut library: ResMut<BlockLibraryState>,
    mut toasts: ResMut<Toasts>,
) {
    if requests.is_empty() {
        return;
    }
    requests.clear();
    if library.in_flight {
        toasts.info("Sync already in progress");
        return;
    }
    library.in_flight = true;
    library.last_error = None;
    log::info!("block_library: sync request → spawning background task");

    let cfg = HfrogConfig::from_env();
    // ureq is sync, so wrap the call in a spawned task the way
    // `export_dialog` does — same `AsyncComputeTaskPool` plumbing.
    // We don't pull tokio in.
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let provider = HfrogProvider::new(cfg);
        provider.sync().map_err(stringify_block_err)
    });
    commands.spawn(PendingSync { task });
}

fn stringify_block_err(e: BlockMetaError) -> String {
    match e {
        BlockMetaError::NotFound(id) => format!("block not found: {id}"),
        BlockMetaError::Remote(msg) => format!("hfrog: {msg}"),
        BlockMetaError::Io(e) => format!("io: {e}"),
        BlockMetaError::Decode(msg) => format!("decode: {msg}"),
    }
}

fn poll_pending_sync(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PendingSync)>,
    mut library: ResMut<BlockLibraryState>,
    mut toasts: ResMut<Toasts>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    for (entity, mut pending) in &mut tasks {
        if let Some(result) = block_on(future::poll_once(&mut pending.task)) {
            commands.entity(entity).despawn();
            library.in_flight = false;
            match result {
                Ok(synced) => {
                    log::info!("block_library: sync delivered {} blocks", synced.len());
                    let n = synced.len();
                    // Merge: keep local blocks not overridden by
                    // hfrog ids (matches the CLI semantics).
                    let mut merged: Vec<BlockMeta> =
                        LocalProvider::new().list().unwrap_or_default();
                    let hfrog_ids: std::collections::HashSet<String> =
                        synced.iter().map(|b| b.id.clone()).collect();
                    merged.retain(|b| !hfrog_ids.contains(&b.id));
                    merged.extend(synced);
                    merged.sort_by(|a, b| a.id.cmp(&b.id));
                    library.blocks = merged;
                    library.last_error = None;
                    toasts.info(format!("Synced {n} hfrog block(s)"));
                }
                Err(msg) => {
                    log::warn!("block_library: sync failed: {msg}");
                    library.last_error = Some(msg.clone());
                    toasts.error(format!("Sync failed: {msg}"));
                }
            }
            redraw.write(RequestRedraw);
        }
    }
}

/// Helper used by the egui panel: snapshot the library into an
/// immutable `Arc<[BlockMeta]>` for the duration of one frame so the
/// closure can lend the data to nested UIs without re-borrowing the
/// resource.
///
/// Currently unused by `ui.rs` (the panel navigates
/// `library.blocks` directly through the immutable `&Res<…>`
/// borrow). Kept for the D-1 generate-button code path that wants
/// to ship the catalog into a background task without holding the
/// resource borrow.
#[allow(dead_code)]
pub fn snapshot(library: &BlockLibraryState) -> Arc<[BlockMeta]> {
    library.blocks.clone().into()
}

/// Decide what colour to paint the source-badge tag on a library
/// card. Plain UI helper, kept here so future panel revisions don't
/// need to re-derive the colour mapping.
#[allow(dead_code)]
pub fn source_label_color(source: &BlockMetaSource) -> bevy::color::Color {
    match source {
        BlockMetaSource::Local => bevy::color::Color::srgb(0.55, 0.55, 0.65),
        BlockMetaSource::LocalDraft { .. } => bevy::color::Color::srgb(0.95, 0.75, 0.40),
        BlockMetaSource::Hfrog { .. } => bevy::color::Color::srgb(0.45, 0.65, 0.95),
    }
}
