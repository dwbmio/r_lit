default:
    just --list

# Build and tests are worker-only. This is a development helper, not a runtime dependency.
[private]
worker:
    test -n "${JENKINS_URL:-}" && test -n "${NODE_NAME:-}" && test "${NODE_NAME}" != "master" && test "${NODE_NAME}" != "built-in" && test "${NODE_NAME}" != "controller"

__cargo_build method="debug": worker
    {{if method == "release" { "cargo build --locked --release" } else { "cargo build --locked" }}}

build method="debug":
    just __cargo_build {{method}}

test: worker
    cargo test --locked

install_loc target:
    install -m 755 out/{{target}}/mini_reg "${INSTALL_DIR:-$HOME/.local/bin}/mini_reg"

gen_doc:
    mini_reg --help
