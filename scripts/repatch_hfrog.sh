#!/usr/bin/env bash
# Requires bash >= 4 (uses associative arrays). On macOS install with:
#   brew install bash    # then run via:  /opt/homebrew/bin/bash scripts/repatch_hfrog.sh
# scripts/repatch_hfrog.sh
#
# One-shot remediation:
#   1. Clean stray `_probe_*` records I created while reverse-engineering the
#      hfrog API (no DELETE endpoint exists, so this needs psql).
#   2. Re-sync existing GitHub Releases (textexture-v0.1.0, maquette-v0.1.0)
#      to hfrog with the COMPLETE field set: install_script_url, file_size,
#      checksum_sha256, source_type, real release_notes, category.
#   3. Render scripts/install.sh.template for every released tool and upload
#      it to R2 (bucket prod-gamesci-lite, public domain gamesci-lite.com).
#
# Run this ONCE locally after the new release.yml lands but before the next
# real release fires. Re-running is safe (idempotent).
#
# Required env (source ci-all-in-one/secrets/.credentials.env first):
#   POSTGRES_HFROG_HOST POSTGRES_HFROG_PORT POSTGRES_HFROG_DB
#   POSTGRES_HFROG_USER POSTGRES_HFROG_PASSWORD
#   R2_ENDPOINT R2_ACCESS_KEY_ID R2_SECRET_ACCESS_KEY R2_BUCKET
#
# Optional:
#   GITHUB_REPO  (default dwbmio/r_lit)
#   HFROG_API    (default https://hfrog.gamesci-lite.com)
#   R2_PUBLIC_DOMAIN (default gamesci-lite.com)

set -euo pipefail

GITHUB_REPO="${GITHUB_REPO:-dwbmio/r_lit}"
HFROG_API="${HFROG_API:-https://hfrog.gamesci-lite.com}"
R2_PUBLIC_DOMAIN="${R2_PUBLIC_DOMAIN:-gamesci-lite.com}"
R2_KEY_PREFIX="${R2_KEY_PREFIX:-r_lit}"
R2_BUCKET="${R2_BUCKET:-prod-gamesci-lite}"

c_red='\033[0;31m'; c_grn='\033[0;32m'; c_ylw='\033[1;33m'; c_off='\033[0m'
info() { printf "${c_grn}[info]${c_off} %s\n" "$*"; }
warn() { printf "${c_ylw}[warn]${c_off} %s\n" "$*" >&2; }
die()  { printf "${c_red}[err ]${c_off} %s\n" "$*" >&2; exit 1; }

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="${REPO_ROOT}/scripts/install.sh.template"
META="${REPO_ROOT}/release-metadata.json"
[ -f "$TEMPLATE" ] || die "missing $TEMPLATE"
[ -f "$META" ]     || die "missing $META"

for cmd in jq curl gh psql; do
    command -v "$cmd" >/dev/null 2>&1 || die "missing dependency: $cmd"
done

# r2.py is a tiny boto3 wrapper used in place of awscli (which is broken
# on the maintainer's macOS pyenv install). The same script works in any
# env that has Python ≥3.8 + boto3.
R2_PY="${REPO_ROOT}/scripts/r2.py"

# Choose a Python that actually has boto3.
PY="${R2_PY_PYTHON:-/usr/bin/python3}"
if ! "$PY" -c 'import boto3' 2>/dev/null; then
    if command -v python3 >/dev/null && python3 -c 'import boto3' 2>/dev/null; then
        PY=$(command -v python3)
    else
        warn "boto3 not available — Step 3 (R2 upload) will be skipped."
        warn "fix: /usr/bin/python3 -m pip install --user boto3"
        PY=""
    fi
fi

# Cross-platform helpers (macOS BSD vs Linux GNU)
file_size() {
    if stat -c%s "$1" >/dev/null 2>&1; then stat -c%s "$1"
    else stat -f%z "$1"; fi
}
file_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
    else die "need sha256sum or shasum"; fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 1 — Clean _probe_* rows created during API reverse-engineering
# ─────────────────────────────────────────────────────────────────────────────
psql_run() {
    PGPASSWORD="$POSTGRES_HFROG_PASSWORD" psql -v ON_ERROR_STOP=1 \
        "postgresql://${POSTGRES_HFROG_USER}@${POSTGRES_HFROG_HOST}:${POSTGRES_HFROG_PORT:-5432}/${POSTGRES_HFROG_DB}" \
        "$@"
}

clean_probes() {
    info "═══ Step 1/3 — clean _probe_* rows from hfrog DB ═══"
    [ -n "${POSTGRES_HFROG_HOST:-}" ] || { warn "POSTGRES_HFROG_HOST not set, skip"; return; }

    # rb_software_releases → rb_versions → rb_softwares (CASCADE handles the
    # rb_versions→rb_software_releases path, but be explicit for clarity).
    # rb_platforms is independent and only deleted if its code matches.
    psql_run <<'SQL'
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

# Force-update software metadata (description / install_command /
# install_script_url / category_id / llms_txt / readme_url) for every tool
# in release-metadata.json. The hfrog HTTP API treats POST /softwares as
# "create-only" for these fields, so we patch directly. SQL is the only way
# to flip stale "nexus.gamesci-lite.com/..." URLs that pre-date this overhaul.
force_update_software_meta() {
    info "═══ Step 1.5/3 — force-refresh software metadata (SQL) ═══"
    [ -n "${POSTGRES_HFROG_HOST:-}" ] || { warn "POSTGRES_HFROG_HOST not set, skip"; return; }

    while IFS= read -r name; do
        [ -n "$name" ] || continue
        local meta desc category install_cmd install_sh readme llms
        meta=$(jq -c --arg n "$name" '.tools[$n] // {}' "$META")
        desc=$(echo "$meta" | jq -r --arg fb "r_lit/${name}" '.description // $fb')
        category=$(echo "$meta" | jq -r '.category // "cli"')

        install_cmd="curl -fsSL https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/${name}/install.sh | bash"
        install_sh="https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/${name}/install.sh"
        readme="https://github.com/${GITHUB_REPO}/blob/main/${name}/README.md"
        llms="https://raw.githubusercontent.com/${GITHUB_REPO}/main/${name}/llms.txt"

        # Skip if the software row doesn't exist yet — first real release
        # will create it via hfrog POST.
        local present
        present=$(psql_run -At -c "SELECT 1 FROM rb_softwares WHERE name = '${name}';" 2>/dev/null)
        [ "$present" = "1" ] || { echo "  · ${name}: not in hfrog yet, will be created on first release"; continue; }

        echo "  ✎ ${name}  (cat=${category})"
        psql_run <<SQL >/dev/null
UPDATE rb_softwares
   SET description        = \$\$${desc}\$\$,
       install_command    = \$\$${install_cmd}\$\$,
       install_script_url = \$\$${install_sh}\$\$,
       readme_url         = \$\$${readme}\$\$,
       llms_txt           = \$\$${llms}\$\$,
       category_id        = (SELECT id FROM rb_categories WHERE code = '${category}'),
       updated_at         = CURRENT_TIMESTAMP
 WHERE name = '${name}';
SQL
    done < <(jq -r '.tools | keys[]' "$META")
    info "✓ metadata refreshed"
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 2 — Re-sync existing GitHub Releases to hfrog with full fields
# ─────────────────────────────────────────────────────────────────────────────
hfrog_post() {
    local endpoint=$1 data=$2 ; local resp http body code msg
    resp=$(curl -sS -w "\n%{http_code}" -X POST "${HFROG_API}${endpoint}" \
           -H 'Content-Type: application/json' -d "$data" 2>&1) || true
    http=$(printf '%s\n' "$resp" | tail -1)
    body=$(printf '%s\n' "$resp" | sed '$d')
    code=$(echo "$body" | jq -r '.code // empty' 2>/dev/null || echo '')
    msg=$(echo  "$body" | jq -r '.msg  // empty' 2>/dev/null || echo '')

    if [[ "$http" =~ ^2 ]] && [ "$code" = "0" ]; then
        echo "    ✓ ${endpoint}"
        return 0
    fi
    if echo "$msg $body" | grep -qiE 'already.*exist|AlreadyExist'; then
        echo "    = ${endpoint} (already exists)"
        return 0
    fi
    warn "${endpoint} failed (http=${http} code=${code} msg=${msg})"
    return 1
}

# Map Rust target → hfrog platform spec
declare -A PLATFORM_DICT=(
    [x86_64-unknown-linux-gnu]="linux|x86_64|Linux x86_64"
    [aarch64-unknown-linux-gnu]="linux|aarch64|Linux ARM64"
    [aarch64-apple-darwin]="macos|aarch64|macOS ARM64 (Apple Silicon)"
    [x86_64-apple-darwin]="macos|x86_64|macOS Intel"
    [x86_64-pc-windows-msvc]="windows|x86_64|Windows x86_64"
)

resync_one() {
    local NAME=$1 VERSION=$2
    local TAG="${NAME}-v${VERSION}"

    info "─── ${NAME} v${VERSION} ───"

    local meta desc category source_type gui targets
    meta=$(jq -c --arg n "$NAME" '.tools[$n] // {}' "$META")
    [ "$meta" = "{}" ] && warn "no metadata for $NAME, using fallbacks"
    desc=$(echo "$meta"        | jq -r --arg fb "r_lit/${NAME}" '.description // $fb')
    category=$(echo "$meta"    | jq -r '.category    // "cli"')
    source_type=$(echo "$meta" | jq -r '.source_type // "open_source"')
    gui=$(echo "$meta"         | jq -r '.gui         // false')

    # Use the actual targets the GitHub Release shipped (look at assets).
    local assets
    assets=$(gh release view "$TAG" -R "$GITHUB_REPO" --json assets \
            --jq '.assets[].name' 2>/dev/null || true)
    [ -n "$assets" ] || { warn "no GitHub release for $TAG, skip"; return 1; }

    local install_cmd install_sh_url notes
    install_cmd="curl -fsSL https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/${NAME}/install.sh | bash"
    install_sh_url="https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/${NAME}/install.sh"
    notes=$(gh release view "$TAG" -R "$GITHUB_REPO" --json body --jq '.body' \
            2>/dev/null || echo "${NAME} v${VERSION}")

    # 1. Software (will update install_script_url + description)
    hfrog_post "/api/release/softwares" "$(jq -nc \
        --arg n "$NAME" --arg d "$desc" \
        --arg ic "$install_cmd" --arg is "$install_sh_url" \
        '{name:$n, description:$d, install_command:$ic, install_script_url:$is}')"

    # 2. Version
    hfrog_post "/api/release/versions" "$(jq -nc \
        --arg n "$NAME" --arg v "v${VERSION}" \
        --arg notes "$notes" --arg by "scripts/repatch_hfrog.sh" \
        '{software_name:$n, version:$v, is_latest:true,
          release_notes:$notes, created_by:$by}')"

    # 3. Per-target platform + release records
    local tmpdir; tmpdir=$(mktemp -d)
    while IFS= read -r asset; do
        # Skip non-archive assets
        case "$asset" in
            ${NAME}-*.tar.gz|${NAME}-*.zip|${NAME}-*.dmg) ;;
            *) continue ;;
        esac

        # Extract target triple from filename
        local target
        target=$(echo "$asset" | sed -E "s/^${NAME}-//" | sed -E 's/\.(tar\.gz|zip|dmg)$//')
        local spec="${PLATFORM_DICT[$target]:-}"
        [ -n "$spec" ] || { warn "unknown target $target in $asset"; continue; }
        IFS='|' read -r os arch display <<< "$spec"

        # Ensure platform row exists
        hfrog_post "/api/release/platforms" "$(jq -nc \
            --arg c "$target" --arg o "$os" --arg a "$arch" --arg d "$display" \
            '{code:$c, os:$o, arch:$a, display_name:$d}')"

        # Download asset to compute size + sha256
        local local_path="${tmpdir}/${asset}"
        gh release download "$TAG" -R "$GITHUB_REPO" -p "$asset" -D "$tmpdir" 2>/dev/null \
            || { warn "could not download ${asset}, skip release record"; continue; }
        local size sha
        size=$(file_size "$local_path")
        sha=$(file_sha256 "$local_path")
        local dl="https://github.com/${GITHUB_REPO}/releases/download/${TAG}/${asset}"

        hfrog_post "/api/release/releases" "$(jq -nc \
            --arg n "$NAME" --arg v "v${VERSION}" --arg c "$target" \
            --arg url "$dl" --argjson sz "$size" \
            --arg sha "$sha" --arg src "$source_type" \
            '{software_name:$n, version:$v, platform_code:$c,
              download_url:$url, file_size:$sz, checksum_sha256:$sha,
              source_type:$src}')"
    done <<< "$assets"
    rm -rf "$tmpdir"

    # 4. Patch category via SQL (hfrog API doesn't accept it yet)
    if [ -n "${POSTGRES_HFROG_HOST:-}" ]; then
        PGPASSWORD="$POSTGRES_HFROG_PASSWORD" psql -v ON_ERROR_STOP=1 \
            "postgresql://${POSTGRES_HFROG_USER}@${POSTGRES_HFROG_HOST}:${POSTGRES_HFROG_PORT:-5432}/${POSTGRES_HFROG_DB}" \
            -c "UPDATE rb_softwares
                   SET category_id = (SELECT id FROM rb_categories WHERE code = '${category}')
                 WHERE name = '${NAME}';" >/dev/null
        echo "    ✓ category=${category}"
    fi
}

resync_releases() {
    info "═══ Step 2/3 — re-sync GitHub Releases to hfrog ═══"

    # Auto-discover all <tool>-v<ver> releases on GitHub.
    while IFS= read -r tag; do
        [ -n "$tag" ] || continue
        if [[ "$tag" =~ ^([a-z_][a-z0-9_]*)-v([0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?)$ ]]; then
            name="${BASH_REMATCH[1]}"
            version="${BASH_REMATCH[2]}"
            if jq -e --arg n "$name" '.tools[$n]' "$META" >/dev/null; then
                resync_one "$name" "$version"
            fi
        fi
    done < <(gh release list -R "$GITHUB_REPO" --limit 50 --json tagName --jq '.[].tagName' || true)
}

# ─────────────────────────────────────────────────────────────────────────────
# Step 3 — Render & upload install.sh for every released tool
# ─────────────────────────────────────────────────────────────────────────────
upload_install_sh() {
    info "═══ Step 3/3 — render & upload install.sh to R2 ═══"
    if [ -z "${R2_ENDPOINT:-}" ]; then
        warn "R2_ENDPOINT not set, skip"
        return
    fi
    if [ -z "$PY" ]; then
        warn "boto3 missing, skip"
        return
    fi

    # Smoke-test the credentials first so we fail loudly instead of half-uploading.
    if ! "$PY" "$R2_PY" ls "${R2_KEY_PREFIX}/" >/dev/null 2>&1; then
        # head_bucket would be cleaner but ls also surfaces 401 on bad creds.
        if ! "$PY" -c "
import os, boto3
from botocore.client import Config
c=boto3.client('s3', endpoint_url=os.environ['R2_ENDPOINT'],
  aws_access_key_id=os.environ['R2_ACCESS_KEY_ID'],
  aws_secret_access_key=os.environ['R2_SECRET_ACCESS_KEY'],
  region_name='auto', config=Config(signature_version='s3v4'))
c.head_bucket(Bucket=os.environ['R2_BUCKET'])
" 2>/dev/null; then
            warn "R2 credentials for bucket=${R2_BUCKET} are invalid (head_bucket → 401)."
            warn "Reset the bucket-scoped S3 API token in Cloudflare R2 dashboard,"
            warn "update R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY in"
            warn "ci-all-in-one/secrets/.credentials.env, then re-run this script."
            return
        fi
    fi

    while IFS= read -r name; do
        [ -n "$name" ] || continue
        local latest_tag latest_ver
        latest_tag=$(gh release list -R "$GITHUB_REPO" --limit 50 --json tagName \
                     --jq ".[].tagName | select(startswith(\"${name}-v\"))" \
                    | sort -V | tail -1)
        [ -n "$latest_tag" ] || { echo "  · ${name}: no GitHub release yet, skip"; continue; }
        latest_ver="${latest_tag#${name}-v}"

        info "─── ${name} v${latest_ver} ─── install.sh"
        local rendered; rendered=$(mktemp)
        sed -e "s|{{TOOL}}|${name}|g" \
            -e "s|{{VERSION}}|${latest_ver}|g" \
            -e "s|{{GITHUB_REPO}}|${GITHUB_REPO}|g" \
            "$TEMPLATE" > "$rendered"
        chmod +x "$rendered"

        local prefix="${R2_KEY_PREFIX}/${name}"
        "$PY" "$R2_PY" cp "$rendered" "${prefix}/install.sh" \
            --content-type "text/x-shellscript; charset=utf-8" \
            --cache-control "public, max-age=300"
        "$PY" "$R2_PY" cp "$rendered" "${prefix}/v${latest_ver}/install.sh" \
            --content-type "text/x-shellscript; charset=utf-8" \
            --cache-control "public, max-age=31536000, immutable"
        rm -f "$rendered"
        echo "  ✓ https://${R2_PUBLIC_DOMAIN}/${prefix}/install.sh"
    done < <(jq -r '.tools | keys[]' "$META")
}

main() {
    info "REPO_ROOT       = ${REPO_ROOT}"
    info "HFROG_API       = ${HFROG_API}"
    info "R2 bucket       = ${R2_BUCKET}"
    info "R2 public URL   = https://${R2_PUBLIC_DOMAIN}/${R2_KEY_PREFIX}/<tool>/install.sh"
    echo

    clean_probes
    echo
    force_update_software_meta
    echo
    resync_releases
    echo
    upload_install_sh
    echo
    info "✓ done"
}

main "$@"
