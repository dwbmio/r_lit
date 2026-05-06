#!/usr/bin/env bash
# scripts/repatch_hfrog.sh
#
# One-shot remediation that calls hfrog_publisher.py for every existing
# GitHub Release of every tool in release-metadata.json. Cleans probe
# leftovers, force-refreshes software metadata via SQL, then re-syncs
# each release tag with full fields + uploads R2 mirror + install.sh.
#
# Idempotent. Run with bash >= 4 (brew install bash on macOS).
#
# Required env (source ci-all-in-one/secrets/.credentials.env first):
#   R2_HFROG_*          (or R2_*)        — for R2 upload
#   POSTGRES_HFROG_*    (with HFROG_PG_URL derived below)
# Optional:
#   GITHUB_REPO         (default dwbmio/r_lit)
#   HFROG_API           (default https://hfrog.gamesci-lite.com)

set -euo pipefail

GITHUB_REPO="${GITHUB_REPO:-dwbmio/r_lit}"
HFROG_API="${HFROG_API:-https://hfrog.gamesci-lite.com}"

# Both naming conventions work — pick whichever is already in your env:
#   HFROG_R2_*  (canonical, from ci-all-in-one/secrets/.credentials.env)
#   R2_HFROG_*  (alias from this script's first iteration, kept for back-compat)
# Last fallback: the legacy R2_* (prod-gamesci-lite, currently 401).
export R2_ENDPOINT="${HFROG_R2_ENDPOINT:-${R2_HFROG_ENDPOINT:-${R2_ENDPOINT:-}}}"
export R2_BUCKET="${HFROG_R2_BUCKET:-${R2_HFROG_BUCKET:-${R2_BUCKET:-prod-hfrog}}}"
export R2_ACCESS_KEY_ID="${HFROG_R2_ACCESS_KEY_ID:-${R2_HFROG_ACCESS_KEY_ID:-${R2_ACCESS_KEY_ID:-}}}"
export R2_SECRET_ACCESS_KEY="${HFROG_R2_SECRET_ACCESS_KEY:-${R2_HFROG_SECRET_ACCESS_KEY:-${R2_SECRET_ACCESS_KEY:-}}}"
R2_PUBLIC_DOMAIN="${HFROG_R2_PUBLIC_DOMAIN:-${R2_HFROG_PUBLIC_DOMAIN:-${R2_PUBLIC_DOMAIN:-r2.gamesci-lite.com}}}"
R2_KEY_PREFIX="${R2_KEY_PREFIX:-r_lit}"

# Postgres URL is consumed by publisher.py via --postgres-url.
if [ -n "${POSTGRES_HFROG_HOST:-}" ] && [ -n "${POSTGRES_HFROG_USER:-}" ]; then
    HFROG_PG_URL="postgresql://${POSTGRES_HFROG_USER}:${POSTGRES_HFROG_PASSWORD}@${POSTGRES_HFROG_HOST}:${POSTGRES_HFROG_PORT:-5432}/${POSTGRES_HFROG_DB}"
    export HFROG_PG_URL
fi

c_red='\033[0;31m'; c_grn='\033[0;32m'; c_ylw='\033[1;33m'; c_off='\033[0m'
info() { printf "${c_grn}[info]${c_off} %s\n" "$*"; }
warn() { printf "${c_ylw}[warn]${c_off} %s\n" "$*" >&2; }
die()  { printf "${c_red}[err ]${c_off} %s\n" "$*" >&2; exit 1; }

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PUBLISHER="${REPO_ROOT}/scripts/hfrog_publisher.py"
TEMPLATE="${REPO_ROOT}/scripts/install.sh.template"
META="${REPO_ROOT}/release-metadata.json"

[ -f "$PUBLISHER" ] || die "missing $PUBLISHER"
[ -f "$TEMPLATE"  ] || die "missing $TEMPLATE"
[ -f "$META"      ] || die "missing $META"

for cmd in jq curl gh psql; do
    command -v "$cmd" >/dev/null 2>&1 || die "missing dependency: $cmd"
done

PY="${HFROG_PY:-/usr/bin/python3}"
"$PY" -c 'import boto3, psycopg2' 2>/dev/null \
    || die "python3 missing boto3 / psycopg2-binary  →  ${PY} -m pip install --user boto3 psycopg2-binary"

# ─────────────────────────────────────────────────────────────────────────────
# Step 1 — clean _probe_* rows left over from API reverse-engineering
# ─────────────────────────────────────────────────────────────────────────────
clean_probes() {
    info "═══ Step 1 — clean _probe_* rows from hfrog DB ═══"
    [ -n "${HFROG_PG_URL:-}" ] || { warn "no HFROG_PG_URL, skip"; return; }
    psql -v ON_ERROR_STOP=1 "$HFROG_PG_URL" <<'SQL'
BEGIN;
DELETE FROM rb_software_releases
 WHERE version_id IN (
    SELECT v.id FROM rb_versions v
     JOIN rb_softwares s ON v.software_id = s.id
    WHERE s.name LIKE '\_probe%' ESCAPE '\'
 )
    OR platform_id IN (SELECT id FROM rb_platforms WHERE code = '_probe_code');
DELETE FROM rb_versions
 WHERE software_id IN (SELECT id FROM rb_softwares WHERE name LIKE '\_probe%' ESCAPE '\');
DELETE FROM rb_softwares WHERE name LIKE '\_probe%' ESCAPE '\';
DELETE FROM rb_platforms WHERE code = '_probe_code';
COMMIT;
SQL
    info "✓ probe rows removed"
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 2 — for every tool x version that has a GitHub Release, run publisher.
# ─────────────────────────────────────────────────────────────────────────────
resync_one() {
    local NAME=$1 VERSION=$2 TAG="${1}-v${2}"
    info "─── ${NAME} v${VERSION} ───"

    local meta desc category source_type
    meta=$(jq -c --arg n "$NAME" '.tools[$n] // {}' "$META")
    if [ "$meta" = "{}" ]; then
        warn "no metadata entry for $NAME, using fallbacks"
    fi
    desc=$(echo "$meta"        | jq -r --arg fb "r_lit/${NAME}" '.description // $fb')
    category=$(echo "$meta"    | jq -r '.category    // "cli"')
    source_type=$(echo "$meta" | jq -r '.source_type // "open_source"')

    local install_url notes tmpdir asset_args=""
    install_url="https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/${NAME}/install.sh"
    notes=$(gh release view "$TAG" -R "$GITHUB_REPO" --json body --jq '.body' \
            2>/dev/null || echo "${NAME} v${VERSION}")

    tmpdir=$(mktemp -d)
    trap "rm -rf '$tmpdir'" RETURN

    while IFS= read -r asset; do
        case "$asset" in
            ${NAME}-*.tar.gz|${NAME}-*.zip|${NAME}-*.dmg) ;;
            *) continue ;;
        esac
        local target
        target=$(echo "$asset" | sed -E "s/^${NAME}-//; s/\\.(tar\\.gz|zip|dmg)\$//")
        gh release download "$TAG" -R "$GITHUB_REPO" -p "$asset" -D "$tmpdir" 2>/dev/null \
            || { warn "could not download $asset"; continue; }
        asset_args="${asset_args} --asset ${target}=${tmpdir}/${asset}"
    done < <(gh release view "$TAG" -R "$GITHUB_REPO" --json assets \
             --jq '.assets[].name' 2>/dev/null)

    [ -n "$asset_args" ] || { warn "$TAG has no usable assets, skip"; return 1; }

    local upload_r2_flag=""
    if [ -n "${R2_ACCESS_KEY_ID:-}" ] && [ -n "${R2_ENDPOINT:-}" ]; then
        upload_r2_flag="--upload-r2 --install-template ${TEMPLATE} --github-repo ${GITHUB_REPO}"
    fi

    local pg_flag=""
    [ -n "${HFROG_PG_URL:-}" ] && pg_flag="--postgres-url ${HFROG_PG_URL}"

    "$PY" "$PUBLISHER" publish \
        --tool "$NAME" --version "$VERSION" \
        $asset_args \
        --description "$desc" \
        --category    "$category" \
        --source-type "$source_type" \
        --install-command    "curl -fsSL ${install_url} | bash" \
        --install-script-url "${install_url}" \
        --readme-url   "https://github.com/${GITHUB_REPO}/blob/main/${NAME}/README.md" \
        --llms-txt-url "https://raw.githubusercontent.com/${GITHUB_REPO}/main/${NAME}/llms.txt" \
        --r2-prefix          "${R2_KEY_PREFIX}/${NAME}" \
        --r2-public-domain   "${R2_PUBLIC_DOMAIN}" \
        --download-url-template "https://github.com/${GITHUB_REPO}/releases/download/${TAG}/{filename}" \
        --release-notes "$notes" \
        --created-by "scripts/repatch_hfrog.sh" \
        --hfrog-api "$HFROG_API" \
        $upload_r2_flag $pg_flag
}

resync_releases() {
    info "═══ Step 2 — re-publish every GitHub Release through hfrog_publisher.py ═══"
    while IFS= read -r tag; do
        [ -n "$tag" ] || continue
        if [[ "$tag" =~ ^([a-z_][a-z0-9_]*)-v([0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?)$ ]]; then
            local name="${BASH_REMATCH[1]}" version="${BASH_REMATCH[2]}"
            if jq -e --arg n "$name" '.tools[$n]' "$META" >/dev/null; then
                resync_one "$name" "$version" || warn "$tag failed, continuing"
            fi
        fi
    done < <(gh release list -R "$GITHUB_REPO" --limit 50 --json tagName --jq '.[].tagName' || true)
}

main() {
    info "REPO_ROOT       = ${REPO_ROOT}"
    info "PUBLISHER       = ${PUBLISHER}"
    info "HFROG_API       = ${HFROG_API}"
    info "R2 bucket       = ${R2_BUCKET}"
    info "R2 public URL   = https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/<tool>/install.sh"
    info "PG patching     = $([ -n "${HFROG_PG_URL:-}" ] && echo enabled || echo disabled)"
    echo

    clean_probes
    echo
    resync_releases
    echo
    info "✓ done"
}

main "$@"
