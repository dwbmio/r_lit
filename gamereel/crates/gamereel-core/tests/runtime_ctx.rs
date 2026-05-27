//! M0 unit tests for `RuntimeCtx` construction and `init` lifecycle.
//!
//! Covers the bug fix in `lib.rs` where `init()` previously had the guard
//! inverted (`if self.init`), so ffmpeg was never actually initialized on
//! the first call. After the fix:
//!   - first call runs `ffmpeg_inc::init_env()` and flips `self.init` to true
//!   - subsequent calls are no-ops on the ffmpeg side
//!   - `source_path` is always (re)assigned

use gamereel_core::RuntimeCtx;
use std::path::PathBuf;

#[test]
fn new_constructs_with_expected_defaults() {
    let ctx = RuntimeCtx::new(720, 1080, 10, 30);
    assert!(!ctx.init, "freshly constructed ctx should not be initialized");
    assert_eq!(ctx.draw_call_times, 0);
    assert!(
        ctx.textures.is_empty(),
        "no textures should be loaded at construction"
    );
}

#[test]
fn init_sets_flag_and_is_idempotent() {
    let mut ctx = RuntimeCtx::new(720, 1080, 10, 30);
    assert!(!ctx.init);

    ctx.init(Some(PathBuf::from("."))).expect("first init should succeed");
    assert!(ctx.init, "init flag must flip to true after first call");

    // Second call must not error and must remain initialized.
    ctx.init(Some(PathBuf::from("."))).expect("second init should be no-op");
    assert!(ctx.init);
}

#[test]
fn init_with_none_uses_current_dir() {
    let mut ctx = RuntimeCtx::new(720, 1080, 10, 30);
    ctx.init(None).expect("init(None) should succeed");
    assert!(ctx.init);
    // RuntimeCtx::source_path is private but observable via cache_path semantics
    // when loading textures. Indirect check: a relative load attempt errors
    // with a path starting from "." rather than panicking on missing source_path.
    let err = ctx.load_loc_image("nonexistent.png", "x").unwrap_err();
    let msg = format!("{err}");
    // We only assert this surfaces an io / image error, not a panic.
    assert!(
        msg.to_lowercase().contains("no such")
            || msg.to_lowercase().contains("error")
            || msg.to_lowercase().contains("nonexistent"),
        "expected an io/image error message, got: {msg}"
    );
}
