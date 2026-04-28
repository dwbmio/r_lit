//! Per-palette-slot texture generation pipeline.
//!
//! The Block Composer (`block_composer.rs`) covers the "create one
//! block from scratch" workflow. This module covers the
//! complementary "I've drawn the model, now texture every palette
//! slot" workflow that lives inside the main editor window.
//!
//! ## Surfaces
//!
//! * **Palette swatch right-click menu** in `ui::palette_bar`:
//!   `Generate texture ▶` submenu with three lanes (Mock / Rustyme
//!   CPU / Rustyme Fal). Each lane writes a [`GenerateSlotTexture`]
//!   message tagged with the slot index and chosen provider.
//! * **D-1.C** — palette-wide "Generate all" button that fans these
//!   same messages out across every live slot. Not in this file
//!   yet; the per-slot system here is what that bulk path will
//!   call into.
//!
//! ## Pipeline
//!
//! 1. `handle_generate_request` reads a [`GenerateSlotTexture`]
//!    event, takes a snapshot of the inputs that
//!    [`maquette::texture_meta::derive_texture_prompt`] needs
//!    (palette slot color, slot meta, the bound block if any,
//!    project meta), composes a [`TextureRequest`], spawns an
//!    `AsyncComputeTaskPool` task, and tags the slot as in-flight.
//! 2. `poll_generate_tasks` watches each spawned task; on success
//!    the PNG is written through `texgen::cache_put` and the slot's
//!    `PaletteSlotMeta::texture` is updated to point at the new
//!    cache key. On failure the toast bubble surfaces the error
//!    and the slot is taken out of `in_flight`.
//!
//! Busy-guard: a slot already in-flight refuses further generates
//! until its current task finishes. Prevents a frustrated user
//! from queueing five concurrent runs onto the same swatch (the
//! Rustyme worker would happily process them all and drown out
//! their other slots).

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy::window::RequestRedraw;

use maquette::block_meta::BlockMeta;
use maquette::grid::Palette;
use maquette::project::ProjectMeta;
use maquette::texgen::{
    cache_put, default_cache_dir,
    rustyme::{RustymeConfig, RustymeProfile, RustymeProvider},
    MockProvider, TextureBytes, TextureProvider, TextureRequest,
};
use maquette::texture_meta::{derive_texture_prompt, TextureHandle};

use crate::block_library::BlockLibraryState;
use crate::notify::Toasts;
use crate::session::CurrentProject;

/// Which lane the user picked from the swatch context menu.
///
/// Mirrors `block_composer::ComposerProvider` — same three lanes,
/// same dispatch semantics. Kept as a separate enum so the two
/// surfaces can evolve independently (e.g. main-window may grow a
/// "fan-out across canvas group" lane that the per-block composer
/// doesn't need).
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotTexgenProvider {
    /// Offline deterministic noise — instant, free, useful for
    /// proving the pipeline works without burning Fal credits or
    /// even running a Rustyme worker.
    #[default]
    Mock,
    /// `texgen-cpu` queue — Rustyme worker generates the PNG with
    /// programmatic Lua + `texgen.gen` task. Cheap, fast, but
    /// limited stylistically.
    RustymeCpu,
    /// `texgen-fal` queue — Rustyme worker proxies to Fal.ai's
    /// FLUX schnell. Best quality; consumes Fal credits.
    RustymeFal,
}

impl SlotTexgenProvider {
    pub fn label(self) -> &'static str {
        match self {
            SlotTexgenProvider::Mock => "Mock (offline)",
            SlotTexgenProvider::RustymeCpu => "Rustyme CPU",
            SlotTexgenProvider::RustymeFal => "Rustyme Fal",
        }
    }
}

/// Outbound: ask the pipeline to generate a texture for one
/// palette slot. The UI writes one of these per right-click
/// menu pick.
#[derive(Message, Clone, Debug)]
pub struct GenerateSlotTexture {
    pub slot: u8,
    pub provider: SlotTexgenProvider,
}

/// Per-frame state for the slot-texgen pipeline.
///
/// Currently just a busy-guard set + last-error string for tests
/// / future "show progress in the swatch" UI. Kept as its own
/// resource (rather than living on `Palette`) because the busy
/// status is GUI-only — the headless lib has no concept of
/// "currently generating".
#[derive(Resource, Default)]
pub struct SlotTexgenState {
    /// Slots whose [`PendingSlotGen`] entity is still alive.
    /// Membership is the busy-guard: any slot in here refuses
    /// further `GenerateSlotTexture` until the entity drops.
    pub in_flight: HashSet<u8>,
    /// Last failure surfaced to the user, kept around for
    /// inspection in tests + future status-bar tooltip.
    pub last_error: Option<String>,
}

/// One in-flight slot-texgen task. Spawned as an entity so
/// `poll_generate_tasks` can `Query` them and despawn on
/// completion — same pattern as `block_composer::PendingGenerate`
/// but per-slot keyed.
#[derive(Component)]
struct PendingSlotGen {
    slot: u8,
    provider: SlotTexgenProvider,
    request: TextureRequest,
    /// Brief one-liner reproduced into the success toast and
    /// stored on the resulting `TextureHandle`.
    prompt_preview: String,
    task: Task<Result<TextureBytes, String>>,
}

pub struct SlotTexgenPlugin;

impl Plugin for SlotTexgenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SlotTexgenState>()
            .add_message::<GenerateSlotTexture>()
            .add_systems(
                Update,
                (handle_generate_request, poll_generate_tasks).chain(),
            );
    }
}

fn handle_generate_request(
    mut events: MessageReader<GenerateSlotTexture>,
    palette: Res<Palette>,
    meta: Res<ProjectMeta>,
    library: Res<BlockLibraryState>,
    mut state: ResMut<SlotTexgenState>,
    mut toasts: ResMut<Toasts>,
    mut commands: Commands,
) {
    for ev in events.read() {
        // Busy guard — same `slot` can't have two simultaneous
        // generates. The user gets a friendly toast instead of a
        // mysteriously second task that overwrites the first.
        if state.in_flight.contains(&ev.slot) {
            toasts.info(format!(
                "slot #{} already generating — waiting for it to finish",
                ev.slot
            ));
            continue;
        }

        // Read the inputs `derive_texture_prompt` needs. Each
        // `Option` here represents one of "the user might not have
        // selected a palette slot" / "the slot might be empty" /
        // "the slot has no bound block". Surface a concrete error
        // toast so the user knows why nothing happened — silent
        // no-ops are the worst kind of pipeline bug.
        let Some(color) = palette.get(ev.slot) else {
            state.last_error = Some(format!("slot {} has no color", ev.slot));
            toasts.error(format!("slot #{} has no color", ev.slot));
            continue;
        };
        let slot_meta = palette.meta(ev.slot).cloned().unwrap_or_default();
        let bound_block: Option<BlockMeta> = slot_meta
            .block_id
            .as_deref()
            .and_then(|id| library.blocks.iter().find(|b| b.id == id).cloned());

        let prompt = derive_texture_prompt(
            &meta.model_description,
            &slot_meta,
            color,
            bound_block.as_ref(),
            &meta.texture_prefs,
        );
        // Same defaults as block_composer — 256² is the sweet
        // spot for tile-able block textures (large enough to read
        // detail, small enough to render fast through Fal). The
        // seed is per-slot so re-rolling the same slot stays
        // deterministic; the user can tweak via the override
        // hint or by editing the slot color.
        let model = match ev.provider {
            SlotTexgenProvider::Mock => MockProvider::MODEL_ID.to_string(),
            SlotTexgenProvider::RustymeCpu | SlotTexgenProvider::RustymeFal => {
                std::env::var("MAQUETTE_RUSTYME_MODEL")
                    .unwrap_or_else(|_| "rustyme:texture.gen".to_string())
            }
        };
        let seed = stable_slot_seed(&prompt, ev.slot);
        let request = TextureRequest::new(prompt.clone(), seed, 256, 256, model);

        state.in_flight.insert(ev.slot);
        state.last_error = None;

        let provider = ev.provider;
        let task_request = request.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            run_generate_blocking(provider, &task_request)
        });

        let prompt_preview = preview_prompt(&prompt);
        commands.spawn(PendingSlotGen {
            slot: ev.slot,
            provider,
            request,
            prompt_preview: prompt_preview.clone(),
            task,
        });

        toasts.info(format!(
            "Generating slot #{}: {}",
            ev.slot, prompt_preview
        ));
    }
}

fn poll_generate_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PendingSlotGen)>,
    mut palette: ResMut<Palette>,
    mut current: ResMut<CurrentProject>,
    mut state: ResMut<SlotTexgenState>,
    mut toasts: ResMut<Toasts>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut woke_redraw = false;
    for (entity, mut pending) in &mut tasks {
        let Some(result) = block_on(future::poll_once(&mut pending.task)) else {
            continue;
        };
        let slot = pending.slot;
        let provider = pending.provider;
        let request = pending.request.clone();
        let prompt_preview = pending.prompt_preview.clone();
        commands.entity(entity).despawn();
        state.in_flight.remove(&slot);
        woke_redraw = true;

        match result {
            Ok(bytes) => {
                // Cache the PNG so the GUI has a stable
                // `<cache_key>.png` to upload to wgpu in D-1.D.
                // If `default_cache_dir()` is `None` (HOME unset
                // in CI / sandboxed environments) we still
                // record the handle — the cache_key alone is
                // enough for tests, and the next launch will
                // re-fetch from Rustyme on a miss.
                if let Some(dir) = default_cache_dir() {
                    if let Err(e) = cache_put(&dir, &request, &bytes) {
                        log::warn!("cache_put failed for slot {slot}: {e}");
                    }
                }
                let handle = TextureHandle {
                    cache_key: request.cache_key(),
                    generated_at: unix_seconds(),
                };
                if let Some(meta) = palette.meta_mut(slot) {
                    meta.texture = Some(handle);
                }
                current.mark_dirty();
                toasts.success(format!(
                    "{} done · slot #{slot} · {prompt_preview}",
                    provider.label()
                ));
            }
            Err(err) => {
                state.last_error = Some(err.clone());
                toasts.error(format!("slot #{slot} failed — {err}"));
            }
        }
    }
    if woke_redraw {
        redraw.write(RequestRedraw);
    }
}

/// Per-slot deterministic seed: hash the prompt + slot index so
/// "regenerate the same slot, same prompt" is reproducible across
/// launches, while two different slots with coincidentally
/// identical prompts still get different seeds (sub-pixel-different
/// noise patterns).
fn stable_slot_seed(prompt: &str, slot: u8) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prompt.hash(&mut h);
    slot.hash(&mut h);
    h.finish()
}

/// Trim the derived prompt to a single short line for toast /
/// hover-text display. Keeps the success toast readable when the
/// prompt is multi-clause.
fn preview_prompt(prompt: &str) -> String {
    const MAX: usize = 64;
    let one_line = prompt.replace('\n', " ");
    if one_line.chars().count() <= MAX {
        one_line
    } else {
        let truncated: String = one_line.chars().take(MAX - 1).collect();
        format!("{truncated}…")
    }
}

/// Local seconds-since-epoch helper. The lib has the same one as
/// `pub(crate) fn maquette::block_meta::unix_seconds`, but it's
/// not visible from the bin target so we keep an identical
/// fallback here. Used to stamp `TextureHandle::generated_at`.
fn unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sync wrapper around the chosen provider's `generate`. Runs on
/// the `AsyncComputeTaskPool` so the (potentially many seconds
/// long) blocking call doesn't stall the egui frame.
fn run_generate_blocking(
    provider: SlotTexgenProvider,
    request: &TextureRequest,
) -> Result<TextureBytes, String> {
    match provider {
        SlotTexgenProvider::Mock => MockProvider
            .generate(request)
            .map_err(|e| e.to_string()),
        SlotTexgenProvider::RustymeCpu | SlotTexgenProvider::RustymeFal => {
            let mut cfg = RustymeConfig::from_env().ok_or_else(|| {
                "rustyme provider needs MAQUETTE_RUSTYME_REDIS_URL — \
                 set it (and MAQUETTE_RUSTYME_ADMIN_URL for revoke) \
                 and try again, or pick provider=mock for an offline run."
                    .to_string()
            })?;
            let profile = match provider {
                SlotTexgenProvider::RustymeCpu => RustymeProfile::Cpu,
                SlotTexgenProvider::RustymeFal => RustymeProfile::Fal,
                SlotTexgenProvider::Mock => unreachable!(),
            };
            cfg.queue_key = profile.queue_key().to_string();
            cfg.result_key = profile.result_key().to_string();
            let p = RustymeProvider::new(cfg);
            p.generate(request).map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_prompt_keeps_short_lines_intact() {
        assert_eq!(preview_prompt("a grass block"), "a grass block");
    }

    #[test]
    fn preview_prompt_collapses_newlines() {
        assert_eq!(preview_prompt("line1\nline2"), "line1 line2");
    }

    #[test]
    fn preview_prompt_truncates_long_input() {
        let long = "x".repeat(200);
        let preview = preview_prompt(&long);
        assert!(preview.chars().count() <= 64);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn stable_slot_seed_is_deterministic() {
        let a = stable_slot_seed("a grass block", 3);
        let b = stable_slot_seed("a grass block", 3);
        assert_eq!(a, b);
    }

    #[test]
    fn stable_slot_seed_differs_per_slot() {
        let a = stable_slot_seed("a grass block", 3);
        let b = stable_slot_seed("a grass block", 4);
        assert_ne!(a, b, "slot index must influence the seed");
    }

    #[test]
    fn stable_slot_seed_differs_per_prompt() {
        let a = stable_slot_seed("grass", 3);
        let b = stable_slot_seed("dirt", 3);
        assert_ne!(a, b, "prompt must influence the seed");
    }

    #[test]
    fn provider_label_is_human_readable() {
        for p in [
            SlotTexgenProvider::Mock,
            SlotTexgenProvider::RustymeCpu,
            SlotTexgenProvider::RustymeFal,
        ] {
            let label = p.label();
            assert!(!label.is_empty());
            // No internal-id leaks like "rustyme:" in user-facing
            // copy.
            assert!(!label.contains(':'));
        }
    }
}
