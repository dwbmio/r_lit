//! Self-proof for the `inventory`-driven ProtocolParser registry.
//!
//! When this test binary runs alone (no proto-* crate linked), the
//! registry must be empty and `build_parser("anything")` must return
//! a structured error listing the empty set. When `cargo test
//! --workspace` runs the same code with proto-puzzle and proto-bubble
//! linked (P5 lands), `tests/protocol_registry_e2e.rs` in the CLI app
//! takes over and asserts both names are present.
//!
//! We also run under `--release` because `inventory` relies on
//! link-time constructors and our release profile uses `lto = "fat"`.
//! If LTO ever strips the constructors, this test fails loudly:
//!   cargo test --release -p gamereel-core --test protocol_inventory

use gamereel_core::protocol::{build_parser, registered_protocols};

#[test]
fn registry_empty_in_core_only_build() {
    let registered: Vec<&'static str> = registered_protocols()
        .iter()
        .map(|d| d.name)
        .collect();
    // No proto-* crate is a dependency of gamereel-core itself, so this
    // binary sees an empty registry. (The CLI binary links proto-* crates
    // and has its own e2e test.)
    assert!(
        registered.is_empty(),
        "core-only test binary should see an empty registry; saw {registered:?} \
         — a proto-* crate accidentally became a dep of gamereel-core?"
    );
}

#[test]
fn build_parser_unknown_returns_helpful_error() {
    let res = build_parser("does-not-exist");
    let msg = match res {
        Ok(_) => panic!("build_parser should fail on a missing name"),
        Err(e) => format!("{e}"),
    };
    assert!(
        msg.contains("does-not-exist") && msg.contains("known:"),
        "error message must surface the requested name and the known list: got {msg:?}"
    );
}
