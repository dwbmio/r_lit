//! 2D grid data model + palette.
//!
//! The `Grid` is the single source of truth for the shape. It holds a flat
//! `Vec<Cell>` in row-major order. Whenever any cell changes, `Grid::dirty`
//! is flipped; the [`crate::mesher`] module consumes the dirty flag and
//! produces the 3D preview mesh.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::texture_meta::{PaletteSlotMeta, TextureHandle};

pub const DEFAULT_GRID_W: usize = 16;
pub const DEFAULT_GRID_H: usize = 16;
/// Lower bound for canvas dimension. Below 4 the output stops reading as an asset.
pub const MIN_GRID: usize = 4;
/// Upper bound, bumped in v0.4 once meshing went culled-single-mesh.
/// v0.5 might bump further after true greedy meshing lands.
pub const MAX_GRID: usize = 128;
/// One grid cell = one world unit.
pub const CELL_SIZE: f32 = 1.0;

/// Maximum column height a single cell may hold.
/// This cap is a **product decision, not a technical one** — see
/// `docs/handoff/COST_AWARENESS.md` §Post-v1.0 Possible Product Line Split.
/// Do NOT raise this past 8 before v1.0; if users keep hitting it, the
/// correct response is to discuss a Maquette Figure fork, not a cap bump.
pub const MAX_HEIGHT: u8 = 8;
pub const MIN_HEIGHT: u8 = 1;

/// Per-cell block shape. v0.9 introduces `Sphere` as a placeholder
/// alternate shape for the right-click "cycle shape" gesture; more
/// shapes (cone, cylinder, slab, etc.) will land post-v1.0. The
/// default — and the only shape the exporter currently emits — is
/// `Cube`.
///
/// Serde layout: `#[serde(rename_all = "snake_case")]` so the on-disk
/// form is human-readable. Missing field in older .maq files falls
/// back to `Cube` via the custom `#[serde(default)]` on `Cell::shape`
/// — existing projects load unchanged.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    #[default]
    Cube,
    Sphere,
}

impl ShapeKind {
    /// Cycle to the next shape. Used by the right-click gesture in
    /// the paint canvas. Order is stable and round-trips.
    pub fn next(self) -> Self {
        match self {
            ShapeKind::Cube => ShapeKind::Sphere,
            ShapeKind::Sphere => ShapeKind::Cube,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Cube => "Cube",
            ShapeKind::Sphere => "Sphere",
        }
    }
}

/// A single cell of the 2D painting canvas.
///
/// `color_idx == None` means the cell is empty (transparent, no geometry
/// emitted). When painted, `height` is in `MIN_HEIGHT..=MAX_HEIGHT`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cell {
    pub color_idx: Option<u8>,
    /// Vertical extrusion in cell units. Only meaningful when
    /// `color_idx.is_some()`. Mesher treats `0` as `1` for legacy v1
    /// project files written before the height UI existed.
    pub height: u8,
    /// Per-cell block shape. Defaults to `Cube` for full backward
    /// compatibility with pre-v0.9 .maq files — the field is
    /// `#[serde(default)]` so older projects load unchanged.
    #[serde(default)]
    pub shape: ShapeKind,
}

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Cell>,
    /// Set by any mutation; cleared after the rebuild system has consumed it.
    #[serde(skip)]
    pub dirty: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            w: DEFAULT_GRID_W,
            h: DEFAULT_GRID_H,
            cells: vec![Cell::default(); DEFAULT_GRID_W * DEFAULT_GRID_H],
            dirty: true, // force initial build
        }
    }
}

impl Grid {
    /// Create a fresh empty canvas of the given dimensions. Dimensions are
    /// clamped to `[MIN_GRID, MAX_GRID]` so the caller never has to
    /// pre-validate user input.
    pub fn with_size(w: usize, h: usize) -> Self {
        let w = w.clamp(MIN_GRID, MAX_GRID);
        let h = h.clamp(MIN_GRID, MAX_GRID);
        Self {
            w,
            h,
            cells: vec![Cell::default(); w * h],
            dirty: true,
        }
    }

    pub fn in_bounds(&self, x: usize, y: usize) -> bool {
        x < self.w && y < self.h
    }

    pub fn get(&self, x: usize, y: usize) -> Option<&Cell> {
        if self.in_bounds(x, y) {
            Some(&self.cells[y * self.w + x])
        } else {
            None
        }
    }

    /// Returns `Some((before, after))` if the cell actually changed, `None` otherwise.
    /// Callers use this to record an undo entry only when a real mutation occurred.
    fn set_if_changed(&mut self, x: usize, y: usize, new_cell: Cell) -> Option<(Cell, Cell)> {
        if !self.in_bounds(x, y) {
            return None;
        }
        let idx = y * self.w + x;
        let before = self.cells[idx];
        if before != new_cell {
            self.cells[idx] = new_cell;
            self.dirty = true;
            Some((before, new_cell))
        } else {
            None
        }
    }

    /// Paint a cell with the given color and height. `height` is clamped
    /// to `MIN_HEIGHT..=MAX_HEIGHT`. Preserves the cell's existing
    /// `shape` — callers that want a specific shape should call
    /// [`Self::paint_with_shape`] instead.
    pub fn paint(
        &mut self,
        x: usize,
        y: usize,
        color_idx: u8,
        height: u8,
    ) -> Option<(Cell, Cell)> {
        // Preserve the existing shape if this cell already has one.
        // "Overwrite" paint semantics are about color + height; a
        // user who set a cell to Sphere and then recolors it expects
        // the sphere to stay a sphere.
        let shape = self.get(x, y).map(|c| c.shape).unwrap_or_default();
        self.paint_with_shape(x, y, color_idx, height, shape)
    }

    /// Paint a cell with explicit shape. Used by Additive-mode code
    /// paths in the UI where the shape carries over from the
    /// existing cell, and by future shape-aware brushes.
    pub fn paint_with_shape(
        &mut self,
        x: usize,
        y: usize,
        color_idx: u8,
        height: u8,
        shape: ShapeKind,
    ) -> Option<(Cell, Cell)> {
        self.set_if_changed(
            x,
            y,
            Cell {
                color_idx: Some(color_idx),
                height: height.clamp(MIN_HEIGHT, MAX_HEIGHT),
                shape,
            },
        )
    }

    /// Cycle the cell's shape to the next variant (see
    /// [`ShapeKind::next`]). Noop on empty cells — cycling a shape
    /// we have no color for would be silently invisible and is
    /// almost certainly a mis-click. Returns `Some((before, after))`
    /// if the cell actually changed.
    pub fn cycle_shape(&mut self, x: usize, y: usize) -> Option<(Cell, Cell)> {
        let current = self.get(x, y)?;
        current.color_idx?;
        let mut next = *current;
        next.shape = current.shape.next();
        self.set_if_changed(x, y, next)
    }

    pub fn erase(&mut self, x: usize, y: usize) -> Option<(Cell, Cell)> {
        self.set_if_changed(x, y, Cell::default())
    }

    pub fn painted_count(&self) -> usize {
        self.cells.iter().filter(|c| c.color_idx.is_some()).count()
    }
}

/// Maximum number of palette slots (ever, across the life of a
/// project). Each slot is addressed by a `u8`; 256 is the natural
/// upper bound and also way more colors than any Maquette asset will
/// ever use.
pub const MAX_PALETTE_SLOTS: usize = 256;

/// User palette. Stored as *sparse* slots so that deleting a color
/// leaves its index behind as `None` instead of shifting every later
/// color down by one. That property — "once a cell is painted with
/// color index N, it will always refer to that same slot until the
/// user explicitly edits the cell" — is what lets projects survive
/// palette edits, save/load, and future collaborators.
///
/// Slots may be reused: [`Palette::add`] fills the first vacant slot
/// before appending to the end, so long-running editing sessions
/// don't grow the palette unbounded.
#[derive(Resource, Debug, Clone)]
pub struct Palette {
    /// Slot vector. `None` means "this index has been deleted and
    /// is available for reuse"; `Some(color)` is a live color.
    pub colors: Vec<Option<Color>>,
    pub selected: u8,
    /// Per-slot texture metadata (override prompt + generated
    /// `TextureHandle`). **Invariant: `slot_meta.len() ==
    /// colors.len()`** at every observable point. `add` / `delete`
    /// / `update` maintain this; loaders ([`Palette::ensure_meta_alignment`])
    /// repair it when a hand-edited or future-versioned file
    /// arrives with a mismatched length.
    ///
    /// Empty / default `PaletteSlotMeta` for a deleted slot is fine —
    /// the slot is "deleted" by `colors[i] == None`, the meta just
    /// rides along. We deliberately do *not* nest the meta under
    /// `Option<...>` to keep `slot_meta[i]` cheaply addressable
    /// without a None-check on every read.
    pub slot_meta: Vec<PaletteSlotMeta>,
}

/// Policy for what should happen to cells that currently use a color
/// about to be deleted. The UI surfaces these as radio buttons on the
/// delete-color modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteColorMode {
    /// Cells that used the color become empty. Default, safest choice.
    Erase,
    /// Cells that used the color are re-painted with the given target
    /// palette index. If the target is invalid (deleted / same as the
    /// color being deleted), the deletion falls back to `Erase`.
    Remap { to: u8 },
}

impl Default for Palette {
    fn default() -> Self {
        let colors = vec![
            Some(Color::srgb(0.90, 0.30, 0.35)), // red
            Some(Color::srgb(0.95, 0.60, 0.25)), // orange
            Some(Color::srgb(0.95, 0.85, 0.35)), // yellow
            Some(Color::srgb(0.45, 0.80, 0.40)), // green
            Some(Color::srgb(0.35, 0.70, 0.90)), // sky
            Some(Color::srgb(0.30, 0.45, 0.85)), // blue
            Some(Color::srgb(0.65, 0.40, 0.85)), // purple
            Some(Color::srgb(0.90, 0.75, 0.65)), // sand
            Some(Color::srgb(0.50, 0.35, 0.25)), // brown
            Some(Color::srgb(0.25, 0.25, 0.30)), // slate
            Some(Color::srgb(0.85, 0.85, 0.90)), // bone
            Some(Color::srgb(0.55, 0.75, 0.55)), // moss
        ];
        let slot_meta = vec![PaletteSlotMeta::default(); colors.len()];
        Self {
            colors,
            selected: 3,
            slot_meta,
        }
    }
}

impl Palette {
    /// Construct a palette from a sparse color vector. Pads
    /// `slot_meta` to the right length so the
    /// `slot_meta.len() == colors.len()` invariant is preserved
    /// without the caller having to think about it.
    ///
    /// Useful in tests and in the project loader (`project.rs`)
    /// where we already have the `Vec<Option<Color>>` from disk
    /// but haven't decoded the optional v4 `palette_meta` yet.
    pub fn from_colors(colors: Vec<Option<Color>>, selected: u8) -> Self {
        let slot_meta = vec![PaletteSlotMeta::default(); colors.len()];
        Self {
            colors,
            selected,
            slot_meta,
        }
    }

    /// Color at the given slot, if live. `None` means either out-of-
    /// bounds or a deleted slot — either way, unsafe to paint with.
    pub fn get(&self, idx: u8) -> Option<Color> {
        self.colors.get(idx as usize).copied().flatten()
    }

    /// True if the slot is in-bounds and holds a live color.
    pub fn is_live(&self, idx: u8) -> bool {
        self.get(idx).is_some()
    }

    /// Number of live (non-deleted) colors.
    pub fn live_count(&self) -> usize {
        self.colors.iter().filter(|c| c.is_some()).count()
    }

    /// Iterate over live `(index, color)` pairs, in slot order.
    pub fn iter_live(&self) -> impl Iterator<Item = (u8, Color)> + '_ {
        self.colors
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|col| (i as u8, col)))
    }

    /// Append a new color, reusing the first vacant slot if any.
    /// Returns the slot index that now holds the new color, or `None`
    /// if the palette is full.
    ///
    /// `slot_meta` for the chosen index is reset to default — adding
    /// a fresh color into a slot that previously held a (deleted)
    /// color must not silently inherit the old slot's
    /// `override_hint` / `texture`. v0.10 C onward.
    pub fn add(&mut self, color: Color) -> Option<u8> {
        if let Some((i, slot)) = self
            .colors
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.is_none())
        {
            *slot = Some(color);
            // Vacant-slot reuse: paranoid-clear the corresponding
            // meta. The invariant is `slot_meta.len() == colors.len()`
            // so the index is in-bounds, but be defensive against a
            // freshly-constructed `Palette { ... }` literal that
            // skipped the constructor.
            if i < self.slot_meta.len() {
                self.slot_meta[i] = PaletteSlotMeta::default();
            }
            return Some(i as u8);
        }
        if self.colors.len() < MAX_PALETTE_SLOTS {
            self.colors.push(Some(color));
            self.slot_meta.push(PaletteSlotMeta::default());
            return Some((self.colors.len() - 1) as u8);
        }
        None
    }

    /// Replace the color at `idx`. Returns `false` if `idx` is
    /// out-of-bounds or currently deleted — callers should treat the
    /// palette as read-only in that case.
    pub fn update(&mut self, idx: u8, color: Color) -> bool {
        match self.colors.get_mut(idx as usize) {
            Some(slot @ Some(_)) => {
                *slot = Some(color);
                true
            }
            _ => false,
        }
    }

    /// Delete the color at `idx`, updating `grid` according to
    /// `mode`. Preserves every other slot's index.
    ///
    /// Returns `false` (and changes nothing) if `idx` is out-of-bounds
    /// or already deleted. This lets the UI bounce idempotent clicks
    /// without tracking its own validity state.
    pub fn delete(&mut self, idx: u8, grid: &mut Grid, mode: DeleteColorMode) -> bool {
        if !self.is_live(idx) {
            return false;
        }
        let remap_target = match mode {
            DeleteColorMode::Erase => None,
            DeleteColorMode::Remap { to } => {
                // Fall back to erase if the remap target is invalid —
                // either out-of-bounds, a deleted slot, or the very
                // color we're deleting.
                if to != idx && self.is_live(to) {
                    Some(to)
                } else {
                    None
                }
            }
        };

        let mut changed = false;
        for cell in &mut grid.cells {
            if cell.color_idx == Some(idx) {
                changed = true;
                match remap_target {
                    Some(to) => cell.color_idx = Some(to),
                    None => *cell = Cell::default(),
                }
            }
        }
        if changed {
            grid.dirty = true;
        }

        self.colors[idx as usize] = None;
        // Drop meta along with the color. A user who later re-adds
        // a color into this slot expects to start from a clean
        // `override_hint` and no stale `TextureHandle` (the cached
        // PNG on disk is keyed by the prompt, not the slot, so
        // even if it's still around it's no longer "this slot's"
        // texture).
        if let Some(meta) = self.slot_meta.get_mut(idx as usize) {
            *meta = PaletteSlotMeta::default();
        }

        // If the user deleted the currently-selected slot, snap
        // selection to any remaining live color. If there are none
        // (pathological: palette fully empty), leave selected at 0 —
        // the next `add` will repopulate slot 0 and select will again
        // point at something live.
        if self.selected == idx {
            let next_live = self.iter_live().next().map(|(i, _)| i).unwrap_or(0);
            self.selected = next_live;
        }
        true
    }

    /// Read-only access to the meta record at `idx`. Returns `None`
    /// if `idx` is out of bounds; **does not** check whether the
    /// slot holds a live color — meta of a deleted slot is always
    /// the default (cleared on delete), but readers shouldn't rely
    /// on that and should check `is_live` separately when it
    /// matters.
    pub fn meta(&self, idx: u8) -> Option<&PaletteSlotMeta> {
        self.slot_meta.get(idx as usize)
    }

    /// Mutable access. Mostly used by the GUI's "edit hint" /
    /// generated-texture-arrived paths; tests use the typed
    /// helpers ([`Self::set_override_hint`] /
    /// [`Self::set_texture`]) instead so the v0.10 C undo wiring
    /// (D-1) has a single chokepoint to hook into.
    pub fn meta_mut(&mut self, idx: u8) -> Option<&mut PaletteSlotMeta> {
        self.slot_meta.get_mut(idx as usize)
    }

    /// Set the per-slot override hint, returning the previous
    /// value. Empty / whitespace-only strings collapse to `None`
    /// so we don't silently store user-fingertip whitespace as
    /// "the user really wants an empty prompt".
    ///
    /// Returns `None` if `idx` is out-of-bounds. Live-vs-deleted
    /// is *not* enforced — historically users pre-write a hint on
    /// a slot they're about to add, and the UI shouldn't have to
    /// wait for `add()` to land first.
    pub fn set_override_hint(&mut self, idx: u8, hint: Option<String>) -> Option<Option<String>> {
        let normalised = hint.and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(s)
            }
        });
        let meta = self.slot_meta.get_mut(idx as usize)?;
        let prev = std::mem::take(&mut meta.override_hint);
        meta.override_hint = normalised;
        Some(prev)
    }

    /// Set the per-slot generated-texture handle, returning the
    /// previous value. The bytes themselves are owned by
    /// `~/.cache/maquette/textures/<cache_key>.png`; this method
    /// just records "which one's mine".
    pub fn set_texture(
        &mut self,
        idx: u8,
        handle: Option<TextureHandle>,
    ) -> Option<Option<TextureHandle>> {
        let meta = self.slot_meta.get_mut(idx as usize)?;
        let prev = std::mem::take(&mut meta.texture);
        meta.texture = handle;
        Some(prev)
    }

    /// Restore the `slot_meta.len() == colors.len()` invariant
    /// after an external rewrite (i.e. project file load). Pads
    /// with default meta when too short, truncates when too long.
    /// Logs a debug line if a fix-up was needed so a forensic
    /// re-load of a malformed file is auditable.
    ///
    /// Idempotent: calling on an already-aligned palette is a no-op.
    pub fn ensure_meta_alignment(&mut self) {
        let n = self.colors.len();
        if self.slot_meta.len() == n {
            return;
        }
        log::debug!(
            "palette: realigning slot_meta {} → {}",
            self.slot_meta.len(),
            n
        );
        self.slot_meta.resize(n, PaletteSlotMeta::default());
    }

    /// How many painted cells currently reference `idx`. Used by the
    /// delete-color modal to tell the user "this will affect N cells".
    pub fn usage_count(&self, grid: &Grid, idx: u8) -> usize {
        grid.cells
            .iter()
            .filter(|c| c.color_idx == Some(idx))
            .count()
    }
}

pub struct GridPlugin;

impl Plugin for GridPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Grid>().init_resource::<Palette>();
    }
}

#[cfg(test)]
mod palette_tests {
    //! Pure data-model tests for [`Palette`]. No Bevy App, no UI —
    //! everything below must stay reachable from headless CI.

    use super::*;

    fn red() -> Color {
        Color::srgb(1.0, 0.0, 0.0)
    }
    fn green() -> Color {
        Color::srgb(0.0, 1.0, 0.0)
    }
    fn blue() -> Color {
        Color::srgb(0.0, 0.0, 1.0)
    }

    #[test]
    fn default_palette_is_all_live() {
        let p = Palette::default();
        assert_eq!(p.live_count(), p.colors.len());
        for (i, slot) in p.colors.iter().enumerate() {
            assert!(slot.is_some(), "slot {i} should be live");
        }
    }

    #[test]
    fn add_appends_when_no_holes() {
        let mut p = Palette::from_colors(vec![Some(red())], 0);
        let idx = p.add(green()).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(p.colors.len(), 2);
        assert_eq!(p.get(1), Some(green()));
        // Invariant.
        assert_eq!(p.slot_meta.len(), p.colors.len());
    }

    #[test]
    fn add_reuses_first_vacant_slot_before_growing() {
        let mut p = Palette::from_colors(vec![Some(red()), None, Some(blue())], 0);
        let idx = p.add(green()).unwrap();
        assert_eq!(idx, 1, "should fill the hole, not append");
        assert_eq!(p.colors.len(), 3);
        assert_eq!(p.get(1), Some(green()));
        assert_eq!(p.slot_meta.len(), p.colors.len());
    }

    #[test]
    fn add_returns_none_when_palette_is_full() {
        let mut p = Palette::from_colors(vec![Some(red()); MAX_PALETTE_SLOTS], 0);
        assert!(p.add(green()).is_none());
    }

    #[test]
    fn update_requires_live_slot() {
        let mut p = Palette::from_colors(vec![Some(red()), None], 0);
        assert!(p.update(0, green()));
        assert_eq!(p.get(0), Some(green()));
        assert!(!p.update(1, green()), "deleted slot should reject update");
        assert!(!p.update(99, green()), "out-of-bounds should reject");
    }

    #[test]
    fn delete_erase_mode_clears_cells_and_preserves_other_indices() {
        let mut grid = Grid::with_size(2, 2);
        grid.paint(0, 0, 0, 1); // red
        grid.paint(1, 0, 1, 1); // orange (default palette)
        grid.paint(0, 1, 2, 1); // yellow

        let mut palette = Palette::default();
        assert!(palette.delete(1, &mut grid, DeleteColorMode::Erase));

        // Slot 1 should now be vacant, but every *other* slot stays
        // exactly where it was — this is the contract v0.6 is built on.
        assert!(!palette.is_live(1));
        assert!(palette.is_live(0));
        assert!(palette.is_live(2));

        // The orange cell is empty again; red + yellow untouched.
        assert_eq!(grid.get(1, 0).unwrap().color_idx, None);
        assert_eq!(grid.get(0, 0).unwrap().color_idx, Some(0));
        assert_eq!(grid.get(0, 1).unwrap().color_idx, Some(2));
    }

    #[test]
    fn delete_remap_mode_redirects_cells() {
        let mut grid = Grid::with_size(2, 1);
        grid.paint(0, 0, 0, 1);
        grid.paint(1, 0, 0, 1);
        let mut palette = Palette::default();

        assert!(palette.delete(0, &mut grid, DeleteColorMode::Remap { to: 2 }));
        assert_eq!(grid.get(0, 0).unwrap().color_idx, Some(2));
        assert_eq!(grid.get(1, 0).unwrap().color_idx, Some(2));
        assert!(!palette.is_live(0));
    }

    #[test]
    fn delete_remap_to_invalid_target_falls_back_to_erase() {
        // The UI generally guards this, but we double-check the
        // core API: remapping to `self`, to a deleted slot, or to
        // out-of-bounds all degrade to `Erase` so we never leave
        // dangling references.
        for bad_to in [0_u8, 99] {
            let mut grid = Grid::with_size(1, 1);
            grid.paint(0, 0, 0, 1);
            let mut palette = Palette::default();
            palette.delete(0, &mut grid, DeleteColorMode::Remap { to: bad_to });
            assert_eq!(grid.get(0, 0).unwrap().color_idx, None);
        }
    }

    #[test]
    fn delete_snaps_selection_to_first_live_slot() {
        let mut palette =
            Palette::from_colors(vec![Some(red()), Some(green()), Some(blue())], 1);
        let mut grid = Grid::with_size(2, 1);
        palette.delete(1, &mut grid, DeleteColorMode::Erase);
        assert_eq!(palette.selected, 0, "selection moved to first live slot");

        // Now delete slot 0; selection must move to slot 2 (skipping hole).
        palette.delete(0, &mut grid, DeleteColorMode::Erase);
        assert_eq!(palette.selected, 2);
    }

    #[test]
    fn delete_nonexistent_slot_is_noop() {
        let mut palette = Palette::from_colors(vec![Some(red()), None], 0);
        let mut grid = Grid::with_size(1, 1);
        assert!(!palette.delete(1, &mut grid, DeleteColorMode::Erase));
        assert!(!palette.delete(99, &mut grid, DeleteColorMode::Erase));
    }

    // --- v0.10 C: slot_meta invariant ---

    #[test]
    fn slot_meta_starts_aligned_with_default_palette() {
        let p = Palette::default();
        assert_eq!(p.slot_meta.len(), p.colors.len());
        for m in &p.slot_meta {
            assert!(m.is_empty(), "default meta should be empty");
        }
    }

    #[test]
    fn delete_clears_slot_meta_for_that_index() {
        let mut palette = Palette::default();
        let mut grid = Grid::with_size(1, 1);
        // Stash an override hint on slot 0, then delete the slot.
        // The hint must NOT leak forward to a future caller who
        // re-adds a color into slot 0.
        palette
            .set_override_hint(0, Some("rusty iron".into()))
            .unwrap();
        assert_eq!(
            palette.meta(0).unwrap().override_hint.as_deref(),
            Some("rusty iron")
        );
        palette.delete(0, &mut grid, DeleteColorMode::Erase);
        assert!(palette.meta(0).unwrap().is_empty());
    }

    #[test]
    fn add_to_recycled_slot_clears_inherited_meta() {
        let mut palette = Palette::default();
        palette
            .set_override_hint(2, Some("old hint".into()))
            .unwrap();
        // Manually wipe color but bypass the public API to forge
        // a "deleted slot whose meta wasn't cleaned up" scenario
        // (could happen via a bad serde path on a corrupted file).
        palette.colors[2] = None;
        // Now `add` should land on slot 2 (first vacant) and the
        // override hint must be cleared even though we left it
        // stale on the slot.
        let idx = palette.add(Color::srgb(1.0, 0.0, 1.0)).unwrap();
        assert_eq!(idx, 2);
        assert!(
            palette.meta(2).unwrap().is_empty(),
            "newly added color must not inherit a stale override hint"
        );
    }

    #[test]
    fn ensure_meta_alignment_pads_when_short() {
        let mut palette = Palette::default();
        // Forge a length mismatch (could only happen via a
        // half-loaded file). The realigner pads back to len.
        palette.slot_meta.truncate(5);
        palette.ensure_meta_alignment();
        assert_eq!(palette.slot_meta.len(), palette.colors.len());
    }

    #[test]
    fn ensure_meta_alignment_truncates_when_long() {
        let mut palette = Palette::default();
        for _ in 0..3 {
            palette.slot_meta.push(PaletteSlotMeta {
                override_hint: Some("ignore".into()),
                texture: None,
            });
        }
        palette.ensure_meta_alignment();
        assert_eq!(palette.slot_meta.len(), palette.colors.len());
    }

    #[test]
    fn set_override_hint_returns_previous_value() {
        let mut palette = Palette::default();
        // First set: prev was None.
        let prev = palette.set_override_hint(0, Some("first".into())).unwrap();
        assert!(prev.is_none());
        // Second set: prev was the first hint.
        let prev = palette.set_override_hint(0, Some("second".into())).unwrap();
        assert_eq!(prev.as_deref(), Some("first"));
        // Out-of-bounds returns None.
        assert!(palette.set_override_hint(99, Some("x".into())).is_none());
    }

    #[test]
    fn set_override_hint_normalises_whitespace_to_none() {
        let mut palette = Palette::default();
        // Whitespace-only is treated as "no hint" — guards against
        // a UI text field that user accidentally hit space in.
        palette.set_override_hint(0, Some("   ".into())).unwrap();
        assert!(palette.meta(0).unwrap().override_hint.is_none());
        palette.set_override_hint(0, Some("".into())).unwrap();
        assert!(palette.meta(0).unwrap().override_hint.is_none());
    }

    #[test]
    fn set_texture_returns_previous_handle() {
        let mut palette = Palette::default();
        let h1 = TextureHandle {
            cache_key: "abc".into(),
            generated_at: 1,
        };
        let h2 = TextureHandle {
            cache_key: "def".into(),
            generated_at: 2,
        };
        let prev = palette.set_texture(0, Some(h1.clone())).unwrap();
        assert!(prev.is_none());
        let prev = palette.set_texture(0, Some(h2.clone())).unwrap();
        assert_eq!(prev.as_ref(), Some(&h1));
        assert_eq!(palette.meta(0).unwrap().texture.as_ref(), Some(&h2));
    }

    #[test]
    fn usage_count_and_iter_live_match_reality() {
        let mut grid = Grid::with_size(4, 1);
        grid.paint(0, 0, 2, 1);
        grid.paint(1, 0, 2, 1);
        grid.paint(2, 0, 5, 1);
        let palette = Palette::default();

        assert_eq!(palette.usage_count(&grid, 2), 2);
        assert_eq!(palette.usage_count(&grid, 5), 1);
        assert_eq!(palette.usage_count(&grid, 7), 0);

        let live_indices: Vec<u8> = palette.iter_live().map(|(i, _)| i).collect();
        assert_eq!(live_indices, (0..palette.colors.len() as u8).collect::<Vec<_>>());
    }
}
