#!/usr/bin/env bash
# Build a signed + notarized .app and .dmg for one Rust GUI binary.
#
# Required env (verified at start):
#   APP_NAME              human-friendly app name (e.g. Maquette)
#   BIN_NAME              cargo binary name (e.g. maquette)
#   PROJECT_DIR           path to the Cargo crate (relative to repo root)
#   TARGET_TRIPLE         e.g. aarch64-apple-darwin
#   VERSION               marketing version, no leading v
#   OUT_DIR               where the final .dmg + .app go
#   MACOS_CERTIFICATE_NAME   "Developer ID Application: Foo Bar (TEAMID)"
#   MACOS_TEAM_ID            10-char Team ID
#   MACOS_NOTARY_APPLE_ID    Apple ID e-mail
#   MACOS_NOTARY_PWD         app-specific password
#
# Optional:
#   SKIP_NOTARIZE=1       skip notary submission (e.g. dry-run)
#
# The script assumes:
#   * codesign identity is already imported into the default keychain
#     (the workflow does this once per job).
#   * `${PROJECT_DIR}/macos/Info.plist` and
#     `${PROJECT_DIR}/macos/${BIN_NAME}.entitlements` exist.
#   * The release binary lives at
#     `${PROJECT_DIR}/target/${TARGET_TRIPLE}/release/${BIN_NAME}`.

set -euo pipefail

require_env() {
    local missing=0
    for var in "$@"; do
        if [ -z "${!var:-}" ]; then
            echo "::error::missing required env: $var" >&2
            missing=1
        fi
    done
    [ "$missing" -eq 0 ] || exit 1
}

require_env APP_NAME BIN_NAME PROJECT_DIR TARGET_TRIPLE VERSION OUT_DIR \
            MACOS_CERTIFICATE_NAME MACOS_TEAM_ID

if [ "${SKIP_NOTARIZE:-0}" != "1" ]; then
    require_env MACOS_NOTARY_APPLE_ID MACOS_NOTARY_PWD
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

BIN_PATH="${PROJECT_DIR}/target/${TARGET_TRIPLE}/release/${BIN_NAME}"
INFO_PLIST="${PROJECT_DIR}/macos/Info.plist"
ENTITLEMENTS="${PROJECT_DIR}/macos/${BIN_NAME}.entitlements"

[ -f "$BIN_PATH" ]       || { echo "::error::missing $BIN_PATH"; exit 1; }
[ -f "$INFO_PLIST" ]     || { echo "::error::missing $INFO_PLIST"; exit 1; }
[ -f "$ENTITLEMENTS" ]   || { echo "::error::missing $ENTITLEMENTS"; exit 1; }

mkdir -p "$OUT_DIR"
STAGE_DIR="$(mktemp -d)"
APP_DIR="${STAGE_DIR}/${APP_NAME}.app"
DMG_NAME="${BIN_NAME}-${TARGET_TRIPLE}.dmg"
DMG_PATH="${OUT_DIR}/${DMG_NAME}"

echo "━━━ Bundling ${APP_NAME}.app (${TARGET_TRIPLE}) ━━━"

mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources"

cp "$BIN_PATH" "${APP_DIR}/Contents/MacOS/${BIN_NAME}"
chmod +x "${APP_DIR}/Contents/MacOS/${BIN_NAME}"

# Copy Info.plist with the version substituted in.
sed "s/__MARKETING_VERSION__/${VERSION}/g" "$INFO_PLIST" \
    > "${APP_DIR}/Contents/Info.plist"

# Optional bundled assets (e.g. shaders, icons) — copy if present.
if [ -d "${PROJECT_DIR}/assets" ]; then
    cp -R "${PROJECT_DIR}/assets" "${APP_DIR}/Contents/Resources/"
fi
if [ -f "${PROJECT_DIR}/macos/AppIcon.icns" ]; then
    cp "${PROJECT_DIR}/macos/AppIcon.icns" "${APP_DIR}/Contents/Resources/"
fi

echo "━━━ Codesigning ${APP_NAME}.app ━━━"
# Sign the binary first, then the bundle, with hardened runtime.
codesign --force --options runtime --timestamp \
    --entitlements "$ENTITLEMENTS" \
    --sign "$MACOS_CERTIFICATE_NAME" \
    "${APP_DIR}/Contents/MacOS/${BIN_NAME}"

codesign --force --options runtime --timestamp \
    --entitlements "$ENTITLEMENTS" \
    --sign "$MACOS_CERTIFICATE_NAME" \
    "${APP_DIR}"

codesign --verify --deep --strict --verbose=2 "${APP_DIR}"

echo "━━━ Building DMG ${DMG_NAME} ━━━"
DMG_STAGE="${STAGE_DIR}/dmg"
mkdir -p "$DMG_STAGE"
cp -R "${APP_DIR}" "${DMG_STAGE}/"
ln -s /Applications "${DMG_STAGE}/Applications"

hdiutil create \
    -volname "${APP_NAME}" \
    -srcfolder "${DMG_STAGE}" \
    -ov -format UDZO \
    "${DMG_PATH}"

# Sign the DMG itself so notarization treats it as a single artifact.
codesign --force --timestamp --sign "$MACOS_CERTIFICATE_NAME" "${DMG_PATH}"

if [ "${SKIP_NOTARIZE:-0}" = "1" ]; then
    echo "::warning::SKIP_NOTARIZE=1 — DMG is signed but NOT notarized"
    rm -rf "$STAGE_DIR"
    echo "✓ Output: ${DMG_PATH} (un-notarized)"
    exit 0
fi

echo "━━━ Notarizing ${DMG_NAME} ━━━"
xcrun notarytool submit "${DMG_PATH}" \
    --apple-id "$MACOS_NOTARY_APPLE_ID" \
    --team-id "$MACOS_TEAM_ID" \
    --password "$MACOS_NOTARY_PWD" \
    --wait

echo "━━━ Stapling ${DMG_NAME} ━━━"
xcrun stapler staple "${DMG_PATH}"
xcrun stapler validate "${DMG_PATH}"

rm -rf "$STAGE_DIR"
echo "✓ Output: ${DMG_PATH}"
