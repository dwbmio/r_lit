# Nexus → Cloudflare R2 — Migration Status

> Updated 2026-05-06. Both r_lit and ci-all-in-one Rust pipelines now publish
> to Cloudflare R2 via the **shared** `hfrog_publisher.py` (sourced from
> `r_lit/scripts/hfrog_publisher.py`, mirrored into ci-all-in-one through
> `scripts/services/hfrog/sync-publisher.sh`). Two separate CIs (GitHub
> Actions and Jenkins), one publishing tool, identical output schema.

## What "complete fields" means now

Every release record sent to HFrog by **either** CI carries:

| Field | Source |
|---|---|
| `software.name` `description` `install_command` `install_script_url` | publisher CLI args / per-tool metadata |
| `software.category_id` `readme_url` `llms_txt` | publisher SQL UPDATE on `rb_softwares` |
| `version.is_latest` `release_notes` `created_by` | publisher CLI args |
| `release.file_size` | computed locally by publisher |
| `release.checksum_sha256` | computed locally by publisher |
| `release.source_type` (`open_source` / `internal`) | publisher CLI args |
| `release.download_url` | publisher's `--download-url-template` |
| `release.platform_code` (one row per actually-built target) | publisher iterates `--asset` flags |

…and uploads each binary + a per-tool `install.sh` to
`s3://prod-hfrog/<prefix>/v<version>/...`, public domain
`r2.gamesci-lite.com` (custom domain binding to be done in the R2 dashboard).

## In r_lit (this repo) — done

| Path | Status |
|---|---|
| `.github/workflows/release.yml` | ✅ `mirror-r2` + `sync-hfrog` collapsed into one matrix `publish` job that calls `scripts/hfrog_publisher.py` |
| `scripts/hfrog_publisher.py` | ✅ created — single source of truth for both CIs |
| `scripts/install.sh.template` | ✅ R2-first installer template |
| `scripts/repatch_hfrog.sh` | ✅ rewritten to drive `hfrog_publisher.py` |
| `release-metadata.json` | ✅ per-tool description / category / source_type / targets |
| `bulk_upload/README.md` `README_CN.md` `llms.txt` `llms_cn.txt` | ✅ install URLs use `https://r2.gamesci-lite.com/r_lit/...` |
| Top-level README/README_CN | ✅ "Install" + "Release" sections |

## In ci-all-in-one — done (by this overhaul)

### Shared publisher infra

| Path | Status |
|---|---|
| `scripts/services/hfrog/publisher.py` | ✅ auto-synced from r_lit (placeholder + sync script) |
| `scripts/services/hfrog/sync-publisher.sh` | ✅ pulls a SHA-pinned copy from `dwbmio/r_lit` |
| `scripts/services/hfrog/jenkins-publish.sh` | ✅ thin wrapper Jenkinsfiles invoke |
| `scripts/services/hfrog/README.md` | ✅ usage + Jenkinsfile recipe |

### Jenkins pipelines (10 files, all upgraded)

For each pipeline below:
- the "上传到 Nexus" + "上传安装脚本" + "同步到 HFrog" stages are **gone**
- replaced by **one** `'发布到 R2 + HFrog'` stage that calls `bash scripts/services/hfrog/jenkins-publish.sh ...`
- the `withCredentials([...])` block expects four Jenkins string credentials:
  `r2-hfrog-endpoint`, `r2-hfrog-access-key-id`,
  `r2-hfrog-secret-access-key`, `hfrog-postgres-url` (PG optional).

| Project | Files | Status |
|---|---|---|
| r_lit (Jenkins copy) | `Jenkinsfile.binary-build` | ✅ |
| hfrog | `Jenkinsfile.binary-build` `Jenkinsfile.binary-build-parallel` `Jenkinsfile.build-deploy` | ✅ (build-deploy only swaps the binary-download step from Nexus to R2 install.sh) |
| hfrog-cli | `Jenkinsfile.binary-build` `Jenkinsfile.binary-build-parallel` | ✅ |
| hfrog-gql | `Jenkinsfile.binary-build` | ✅ |
| wirerope-any | `Jenkinsfile.multi-arch` `Jenkinsfile.dynamic-build` | ✅ |
| pgpour | `Jenkinsfile.binary-build` `Jenkinsfile.docker-build` | ✅ |

### install.sh templates / static fallbacks (created by the upgrade)

Each project gets two new files: a `*.sh.template` rendered by publisher
(uploaded to R2 as `<prefix>/install.sh`), and a static `*.sh` fallback for
local / offline use. Old nexus-pointing `install.sh` files were deleted.

| Project | Template | Static fallback | Old file deleted |
|---|---|---|---|
| hfrog | `scripts/install-hfrog.sh.template` (new) | `scripts/install-hfrog.sh` (rewritten) | — |
| hfrog-cli | `scripts/install-hfrog-cli.sh.template` (new) | `scripts/install-hfrog-cli.sh` (new) | `scripts/install.sh` (deleted) |
| hfrog-gql | `scripts/install-hfrog-gql.sh.template` (new) | `scripts/install-hfrog-gql.sh` (new) | — |
| wirerope-any | `scripts/install-wirerope.sh.template` (new, `{{TOOL}}` placeholder) | — | — |
| pgpour | `scripts/install-pgpour.sh.template` (new) | `scripts/install.sh` (rewritten) | — |
| r_lit (Jenkins copy) | (uses `r_lit/scripts/install.sh.template` fetched at build time) | — | `scripts/install.sh` (deleted) |

### Other

| Path | Status |
|---|---|
| `task/ci/pipeline/hfrog/scripts/build-upload-local.sh` | annotated `⚠ DEPRECATED` with the equivalent publisher invocation |
| `task/ci/pipeline/wirerope-any/README-multi-arch.md` | install / Nexus sections rewritten for R2 |
| `secrets/.credentials.env` | `R2_HFROG_*` block appended (the bucket-scoped token used by all the upgraded pipelines) |

## Still on Nexus (deliberately, low priority)

These projects are not Rust binary distribution and were not in scope:

- `task/ci/pipeline/godot/*/Jenkinsfile` — Godot game/engine artifacts. Large
  payloads, may want a dedicated R2 bucket; treat as a separate migration.
- `task/ci/pipeline/hybrix/*` — Android APK pipeline; APKs already use a
  different distribution (life-jt OSS), only intermediate caches touch nexus.
- `task/ci/pipeline/titan-forge/*` — Titan admin Rust binary deployment;
  migration trivial after this commit (copy the hfrog Jenkinsfile pattern).
- `task/ci/pipeline/batch_jp_url_to_s3/Jenkinsfile` — single internal
  cron job; not user-visible.

## Required Jenkins credentials (one-time setup)

Add via Jenkins UI → Manage Credentials → System → Global → Add Credentials.
Values copied verbatim from `secrets/.credentials.env`:

| ID | Kind | Value source |
|---|---|---|
| `r2-hfrog-endpoint` | Secret text | `R2_HFROG_ENDPOINT` |
| `r2-hfrog-access-key-id` | Secret text | `R2_HFROG_ACCESS_KEY_ID` |
| `r2-hfrog-secret-access-key` | Secret text | `R2_HFROG_SECRET_ACCESS_KEY` |
| `hfrog-postgres-url` | Secret text | `postgresql://${POSTGRES_HFROG_USER}:${POSTGRES_HFROG_PASSWORD}@${POSTGRES_HOST}:${POSTGRES_PORT}/${POSTGRES_HFROG_DB}` |

The legacy `nexus-gamesci-lite` credential can be deleted once any
out-of-scope pipelines that still use it (above) are migrated.

## Sanity check after pipeline runs

For any tool, the four "completeness" smoke tests:

```bash
# 1. R2 install.sh is present
curl -fsSL https://r2.gamesci-lite.com/<prefix>/install.sh | head -5

# 2. HFrog has the version
curl -sS https://hfrog.gamesci-lite.com/api/release/softwares/<tool> | jq '.data.versions[0]'

# 3. Each platform has download_url + file_size + checksum_sha256
curl -sS https://hfrog.gamesci-lite.com/api/release/releases/<tool>/v<ver> | jq '.data.platforms[]'

# 4. Software metadata uses R2 URLs (not nexus)
curl -sS https://hfrog.gamesci-lite.com/api/release/softwares/<tool> | jq '.data.{install_command, install_script_url}'
```

All four should succeed. If `download_url` is 404, the R2 custom domain
binding to `prod-hfrog` is the missing piece — go to R2 dashboard and bind
`r2.gamesci-lite.com` to bucket `prod-hfrog`.
