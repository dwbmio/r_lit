set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

set dotenv-load
BIN := "distill"
SERVER_BIN := "distilld"
BINARY_INSTALL_PATH := if os() == "windows" { env_var('USERPROFILE') + "/.snappywatt/bin" } else { env_var('HOME') + "/.snappywatt/bin" }
S3_ALIAS := "starlink-s3"
S3_BUCKET := "starlink-artifacts"
S3_PATH := "distill"

[private]
default:
    just --list

install_loc method="release":
    cargo build {{ if method == "release" { "--release" } else { "" } }} -p distill-client
    mkdir -p "{{BINARY_INSTALL_PATH}}"
    install -m 755 "target/{{method}}/{{BIN}}" "{{BINARY_INSTALL_PATH}}/{{BIN}}"
    @echo "installed → {{BINARY_INSTALL_PATH}}/{{BIN}}"

s3_setup:
    mc alias set {{S3_ALIAS}} "$S3_ENDPOINT" "$S3_AK" "$S3_SK"

publish-all:
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{justfile_directory()}}"
    VER=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
    HOST=$(rustc -vV | sed -n 's/^host: //p')
    declare -a TARGETS=()
    case "$HOST" in
        aarch64-apple-darwin|x86_64-apple-darwin)
            TARGETS=(
                "aarch64-apple-darwin:{{BIN}}:distill-client"
                "x86_64-apple-darwin:{{BIN}}:distill-client"
                "x86_64-pc-windows-msvc:{{BIN}}.exe:distill-client"
            )
            ;;
        x86_64-unknown-linux-gnu)
            TARGETS=(
                "x86_64-unknown-linux-gnu:{{BIN}}:distill-client"
                "x86_64-unknown-linux-gnu:{{SERVER_BIN}}:distill-server"
            )
            ;;
        *)
            echo "unsupported host: $HOST"; exit 1 ;;
    esac
    for entry in "${TARGETS[@]}"; do
        IFS=':' read -r TARGET BIN_NAME PKG <<< "$entry"
        OUT_NAME="${BIN_NAME%.exe}"
        echo "=== building ${PKG} for ${TARGET} ==="
        if [[ "$TARGET" == "x86_64-pc-windows-msvc" ]] && command -v cargo-xwin >/dev/null 2>&1; then
            RUSTFLAGS="-C target-feature=+crt-static" \
                cargo xwin build --release --target "$TARGET" -p "$PKG"
        else
            cargo build --release --target "$TARGET" -p "$PKG"
        fi
        SRC="target/${TARGET}/release/${BIN_NAME}"
        GZ="/tmp/${OUT_NAME}-${TARGET//\//-}.gz"
        gzip -9 -c "$SRC" > "$GZ"
        mc cp "$GZ" "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/latest/${TARGET}/${OUT_NAME}.gz"
        mc cp "$GZ" "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/v${VER}/${TARGET}/${OUT_NAME}.gz"
        rm -f "$GZ"
    done
    for s in install.sh install.ps1; do
        CLEAN="/tmp/distill-$s"
        perl -0777 -pe 's/^\xef\xbb\xbf//' "scripts/$s" > "$CLEAN"
        mc cp "$CLEAN" "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/$s"
        rm -f "$CLEAN"
    done
    printf '%s\n' "$VER" > /tmp/distill-version.txt
    mc cp --attr "Content-Type=text/plain; charset=utf-8" /tmp/distill-version.txt "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/version.txt"
    mc cp --attr "Content-Type=text/plain; charset=utf-8" /tmp/distill-version.txt "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/latest/version.txt"
    rm -f /tmp/distill-version.txt
    mc cp --attr "Content-Type=text/markdown; charset=utf-8" SKILL.md "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/SKILL.md"
    mc cp --attr "Content-Type=text/markdown; charset=utf-8" SKILL.md "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/latest/SKILL.md"
    if [[ -f CHANGELOG.md ]]; then
        mc cp --attr "Content-Type=text/markdown; charset=utf-8" CHANGELOG.md "{{S3_ALIAS}}/{{S3_BUCKET}}/{{S3_PATH}}/CHANGELOG.md"
    fi
    echo "done! v${VER} (${HOST})"
