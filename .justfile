set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

set dotenv-load

[private]
default:
    just --list

# Build a subproject (e.g. just build img_resize release)
build tar method="release":
    cd {{tar}} && cargo build {{ if method == "release" { "--release" } else { "" } }}

# Locally install a subproject binary (e.g. just install_loc img_resize)
install_loc tar method="release":
    cd {{tar}} && if [ -f .justfile ]; then just install_loc {{method}}; else cargo install --path . {{ if method == "release" { "" } else { "--debug" } }}; fi

# Validate repository documentation, Cargo workspace excludes, and shared abstractions
check-repo:
    python3 scripts/check_repo.py

# Run unit tests for core CLI crates and shared libraries
test-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    for crate in crates/rlit_cli archcast bulk_upload img_resize textexture ui-trim mj_atlas looplog mailctl mini_reg phone_haul audiogen; do
      echo "==> cargo test ($crate)"
      (cd "$crate" && cargo test)
    done
    echo "==> cargo test (maquette headless CLI)"
    (cd maquette && cargo test --no-default-features)

# Run clippy checks across core CLI crates and shared libraries
lint-cli:
    #!/usr/bin/env bash
    set -euo pipefail
    for crate in crates/rlit_cli archcast bulk_upload img_resize textexture ui-trim mj_atlas looplog mailctl mini_reg phone_haul audiogen; do
      echo "==> clippy ($crate)"
      (cd "$crate" && cargo clippy --all-targets -- -D warnings)
    done
    echo "==> clippy (maquette headless CLI)"
    (cd maquette && cargo clippy --no-default-features --tests --bin maquette-cli -- -D warnings)

# Run all repository verification gates
check: check-repo test-cli lint-cli
