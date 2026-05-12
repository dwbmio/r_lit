# Rename & Restructure Plan: video-generator → gamereel

**Status**: queued. Execute **after M3 lands** (CUDA hwframes pipeline). Decided 2026-05-12.

## Why rename

`movie-maker` (and the umbrella directory `video-generator`) describes the *output* (videos / movies) but obscures the *input shape* and *purpose*. The actual job is:

> Given a game's binary protocol message stream (battle reports, match results, replay frames), produce a finished short-form video for TikTok / Instagram Reels.

`gamereel` captures both the input domain (game) and the output form (vertical reel-style video).

## Target workspace layout

```
gamereel/
├── Cargo.toml                       # workspace root
├── crates/
│   ├── gamereel-core/               # ← was movie-maker. Scene + tween + ffmpeg encoding.
│   │                                #   Defines `ProtocolParser` trait + inventory plumbing.
│   ├── gamereel-cuda/               # M3 — CUDA RGBA→NV12 kernel + hwframes glue
│   ├── gamereel-compositor/         # M4 — wgpu compositor replacing image_effect.rs
│   ├── gamereel-farm/               # M5 — actix actor pool, batch CLI
│   ├── proto-puzzle/                # 方块游戏（match-3 / tetris-style）协议解析
│   ├── proto-bubble/                # 泡泡龙协议解析
│   └── proto-<future>/              # one crate per game protocol
├── apps/
│   ├── gamereel-cli/                # main binary; enumerates protocols via inventory
│   └── hs-mvp/                      # current demo, kept as integration example
├── benches/
│   ├── baseline.sh
│   ├── m1.sh / m2.sh / m3.sh ...
│   └── results/                     # mN.json trend artifacts
├── tools/
│   └── quality-eval/                # VMAF / grid_search / scale_path_bench
└── docs/
    ├── optimization-log.md
    ├── encoder-selection.md
    └── rename-and-restructure-plan.md   ← this file (delete after restructure)
```

## Plug-in mechanism: trait + `inventory`

Each `proto-*` crate registers itself at link time via the `inventory` crate; the CLI enumerates registered parsers at startup.

```rust
// gamereel-core/src/protocol.rs
pub trait ProtocolParser: Send + Sync {
    fn name(&self) -> &'static str;
    fn parse(&self, msg: &[u8]) -> Result<Scene, ProtocolError>;
}

pub struct ProtocolDescriptor {
    pub name: &'static str,
    pub factory: fn() -> Box<dyn ProtocolParser>,
}

inventory::collect!(ProtocolDescriptor);
```

```rust
// proto-puzzle/src/lib.rs
inventory::submit! {
    gamereel_core::ProtocolDescriptor {
        name: "puzzle",
        factory: || Box::new(PuzzleParser),
    }
}
```

```rust
// apps/gamereel-cli/src/main.rs
for desc in inventory::iter::<gamereel_core::ProtocolDescriptor> {
    registry.insert(desc.name, (desc.factory)());
}
```

**Adding a new game** = create `crates/proto-<name>/` + add to `[dependencies]` of the CLI. **Zero modification to core or CLI logic.**

### Notes on inventory caveats
- `inventory::submit!` uses link-time constructors (`.init_array` / equivalent). Works on Linux + macOS + Windows MSVC. Test that `cargo build --release` with `lto = "fat"` doesn't strip the constructors (use `-C link-args=-Wl,--no-gc-sections` or mark the registration symbols `#[used]` if needed).
- For static-linked builds, the consuming binary must `extern crate proto_puzzle;` (or use `--extern`) at least once so the linker pulls the object in. Cargo `[dependencies]` does this implicitly.

## Workspace `Cargo.toml` skeleton (target state)

```toml
[workspace]
resolver = "2"
members  = [
  "crates/gamereel-core",
  "crates/gamereel-cuda",
  "crates/gamereel-compositor",
  "crates/gamereel-farm",
  "crates/proto-puzzle",
  "crates/proto-bubble",
  "apps/gamereel-cli",
  "apps/hs-mvp",
]

[workspace.dependencies]
# Single source of truth for shared deps; per-crate Cargo.toml uses workspace = true
ffmpeg-next  = { git = "https://github.com/zmwangx/rust-ffmpeg.git" }
rsmpeg       = "0.15"          # M3 will introduce; better hwframes story than ffmpeg-next
cudarc       = "0.19"          # M3
inventory    = "0.3"
thiserror    = "2"
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
log          = "0.4"
env_logger   = "0.11"
image        = "0.25"
imageproc    = "0.25"
tokio        = { version = "1", features = ["fs","io-util","rt"] }
tween        = "2"
actix        = "0.13"          # M5

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = false

[profile.bench]
opt-level = 3
lto = "fat"
codegen-units = 1
debug = true

[profile.release-small]
inherits = "release"
opt-level = "z"
lto = true
strip = true
```

## Mechanical migration steps (when M3 is done)

1. **Move the directory**: `git mv video-generator gamereel`
2. **Reorganize into `crates/` and `apps/`**:
   - `mv movie-maker crates/gamereel-core` (rename `Cargo.toml` package field)
   - `mv demo apps/hs-mvp`
   - The M3 crate (named `movie-maker-cuda` or similar in M3) → `crates/gamereel-cuda`
3. **Adopt workspace.dependencies**: deduplicate per-crate `[dependencies]` blocks.
4. **Rename `MoveMakerResult` → `GamereelResult`** (typo fix bundled in).
5. **Rename `MovieError` → `GamereelError`**.
6. **Update all `use movie_maker::...` to `use gamereel_core::...`** — `cargo fix` and a sed sweep.
7. **Bump README.md / README_CN.md / llms.txt / llms_cn.txt** to reflect the new name and purpose statement.
8. **Update root r_lit `CLAUDE.md`** — add `gamereel/` row to the tools table; remove the `video-generator/` row.
9. **Add `proto-puzzle/` + `proto-bubble/` skeletons** — both empty crates that just register a no-op `ProtocolParser`. Real implementations come in subsequent PRs.
10. **CLI scaffold** at `apps/gamereel-cli` with `clap` derive, `--json` global flag, bilingual help (matches CLAUDE.md repo conventions).
11. **Update GitHub Actions release detect matrix** in `.github/workflows/release.yml` if applicable (replace `video-generator` mentions).
12. **Delete this plan file** once everything above is checked off — it's intentionally one-shot.

## Acceptance for the rename PR

- `cargo build --workspace` ✓
- `cargo test --workspace` ✓ (all 24+ existing tests pass under new paths)
- `apps/gamereel-cli --help` shows registered protocols (puzzle + bubble) — proves inventory plumbing works
- `benches/m3.sh` (latest) still produces a valid `m3.json`
- `git log --oneline` retains the M0..M3 commit chain (no rebase / squash)
- README trend table renamed and pointing to `gamereel/...` paths

## Risks

- **`inventory` + `lto=fat` interaction**: hot-test before assuming. If link-time stripping kills the descriptors, fall back to a `register_all_protocols()` fn that the CLI calls explicitly.
- **rsmpeg vs ffmpeg-next coexistence**: M3 will introduce rsmpeg for CUDA hwframes. Decide if M3 fully replaces ffmpeg-next or coexists — bookkeeping for the rename either way.
- **Cargo.lock churn**: workspace dedup will rewrite the lockfile; review carefully, do not blindly accept.
