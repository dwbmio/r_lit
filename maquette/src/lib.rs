//! Maquette headless core.
//!
//! Everything reachable from this crate root is **window-free**: no
//! `bevy_egui`, no `bevy_panorbit_camera`, no `rfd`, no `wgpu`
//! device setup. The CLI binary (`maquette-cli`) links against this
//! lib only; the GUI binary (`maquette`) links against this lib *plus*
//! the windowing stack defined in `src/main.rs`.
//!
//! See `docs/handoff/COST_AWARENESS.md` §The Headless Invariant for
//! the rules that govern what belongs here vs. in the GUI binary.
//!
//! ## Module map
//!
//! - [`grid`]    — 2D canvas data model + fixed palette.
//! - [`mesher`]  — implicit 3D voxel grid → triangle-list builder.
//! - [`project`] — `.maq` project file format, serde, load/save.
//! - [`export`]  — glTF 2.0 / GLB writer + inverted-hull outline
//!   baker. This is the single source of truth for what Maquette
//!   emits, shared by the GUI's `File → Export` flow and the CLI's
//!   `maquette-cli export` verb.
//! - [`render`]  — pure-CPU isometric rasterizer that turns a
//!   grid+palette into an RGBA buffer / PNG file. Used by
//!   `maquette-cli render` to produce preview thumbnails in CI
//!   without a GPU.
//! - [`palette_io`] — portable `colors.json` reader/writer for
//!   sharing palettes across projects.
//! - [`texture_meta`] — per-palette-slot texture metadata
//!   (`override_hint`, `texture: TextureHandle`) and project-wide
//!   `TexturePrefs`. Introduced in schema v4 (v0.10 C).

pub mod export;
pub mod grid;
pub mod mesher;
pub mod palette_io;
pub mod project;
pub mod render;
pub mod texgen;
pub mod texture_meta;
