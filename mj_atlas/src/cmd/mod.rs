//! Read- and metadata-only subcommands operating on a packed atlas manifest.
//!
//! These commands never repack — they consume `<output>.manifest.json` and
//! either print derived views (`inspect`, `diff`, `verify`) or rewrite the
//! user-editable fields (`tag`). Together they form the v0.3 "manifest as
//! first-class artifact" surface, on top of which v0.4's cross-project
//! sprite library will be built.

pub mod diff;
pub mod inspect;
pub mod tag;
pub mod verify;
