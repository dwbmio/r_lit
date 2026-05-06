# Release Pipeline

Reference for the `Release` workflow at
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

## Pipeline overview

```
release-metadata.json   (single source of truth: description, category,
       │                 source_type, targets, gui, macos_app_name)
       ▼
push:main / workflow_dispatch / repository_dispatch
       │
       ▼
┌──────────────┐
│  detect      │  diff Cargo.toml + enrich each project from metadata
└──────┬───────┘
       │  projects = [{name, version, description, category,
       │               source_type, gui, macos_app_name, targets}]
       ▼
┌──────────────┐
│  build       │  per-target matrix; only targets the project actually
│              │  declares; macOS GUI builds are signed + notarized
└──────┬───────┘
       │  artifacts = release/<tool>-<target>.{tar.gz,zip,dmg}
       ▼
┌──────────────┐
│  release     │  per-tool GitHub Release with SHA256SUMS + per-asset
│              │  size & checksum table; notes generated from template
└──────┬───────┘
       │  released = [{name, version, tag}]   pkg-* artifact
       ▼
┌──────────────┐  Cloudflare R2 — bucket prod-hfrog
│  mirror-r2   │  → r_lit/<tool>/install.sh                (latest)
│              │  → r_lit/<tool>/v<ver>/<assets>           (immutable)
└──────┬───────┘
       │
       ▼
┌──────────────┐  HFrog — https://hfrog.gamesci-lite.com
│  sync-hfrog  │  software   (name, description, install_command, install_script_url)
│              │  platform   (code, os, arch, display_name) — only built targets
│              │  version    (software_name, version, is_latest, release_notes,
│              │              created_by)        ← release_notes = real RELEASE_NOTES.md
│              │  release    (software_name, version, platform_code, download_url,
│              │              file_size, checksum_sha256, source_type)
│              │  category   patched via psql (UPDATE rb_softwares SET category_id=…)
└──────────────┘
```

`mirror-r2` and `sync-hfrog` both run from the same `release-pkgs` artifact
the `release` job uploaded, so file sizes / sha256 / RELEASE_NOTES are
authoritative across all sinks.

## Required GitHub Secrets

Configure in `Settings → Secrets and variables → Actions`. Anything marked
**required** will fail the workflow if missing; optional secrets degrade
gracefully.

| Secret | Required by | Notes |
|---|---|---|
| `R2_ACCESS_KEY_ID` | `mirror-r2` | Cloudflare R2 access key — value of `R2_HFROG_ACCESS_KEY_ID` in `ci-all-in-one/secrets/.credentials.env` |
| `R2_SECRET_ACCESS_KEY` | `mirror-r2` | Cloudflare R2 secret key — value of `R2_HFROG_SECRET_ACCESS_KEY` |
| `R2_ENDPOINT` | `mirror-r2` | `https://240d77865abd8ef6f48521ba34845508.r2.cloudflarestorage.com` |
| `R2_BUCKET` | `mirror-r2` | `prod-hfrog` (bound to public domain `r2.gamesci-lite.com`) |
| `MACOS_CERTIFICATE` | `build` (macOS GUI only) | base64-encoded `.p12` Developer ID Application cert. Skip → unsigned build |
| `MACOS_CERTIFICATE_PWD` | `build` (macOS GUI only) | password for the .p12 |
| `MACOS_KEYCHAIN_PASSWORD` | `build` (macOS GUI only) | transient keychain pwd, any string |
| `MACOS_CERTIFICATE_NAME` | `build` (macOS GUI only) | e.g. `Developer ID Application: Foo (TEAMID)` |
| `MACOS_TEAM_ID` | `build` (macOS GUI only) | 10-char Apple Team ID |
| `MACOS_NOTARY_APPLE_ID` | `build` (macOS GUI only) | Apple ID e-mail for notarytool |
| `MACOS_NOTARY_PWD` | `build` (macOS GUI only) | app-specific password |
| `POSTGRES_HFROG_HOST` | `sync-hfrog` (optional) | If set, also patches `category_id` via SQL |
| `POSTGRES_HFROG_PORT` | `sync-hfrog` (optional) | default `5432` |
| `POSTGRES_HFROG_DB` | `sync-hfrog` (optional) | `hfrog` |
| `POSTGRES_HFROG_USER` | `sync-hfrog` (optional) | `kong_gate_admin` |
| `POSTGRES_HFROG_PASSWORD` | `sync-hfrog` (optional) | see `ci-all-in-one/secrets/.credentials.env` |

The four `R2_*` and the `POSTGRES_HFROG_*` values all live in
`/Users/.../ci-all-in-one/secrets/.credentials.env`. Copy them verbatim
into GitHub Actions secrets.

## Adding a new tool

1. Create the crate (`<tool>/Cargo.toml`).
2. Append an entry under `tools` in
   [`release-metadata.json`](../release-metadata.json):

   ```json
   "your_tool": {
     "description": "What it does, in one paragraph.",
     "category": "cli"
   }
   ```

   Optional keys: `gui`, `macos_app_name`, `source_type`, `targets`.
3. Bump `version` in `Cargo.toml`, push to `main` — done.

The first release will create the `software` row in HFrog and the
`r_lit/<tool>/install.sh` entry on R2.

## One-shot remediation

`scripts/repatch_hfrog.sh` exists for two scenarios:

- After this CI overhaul lands, to retroactively backfill
  `install_script_url`, `file_size`, `checksum_sha256`, `source_type`,
  `release_notes`, and `category_id` for releases that pre-date the new
  workflow (textexture-v0.1.0, maquette-v0.1.0).
- To clean up `_probe_*` rows left over from API reverse-engineering
  (hfrog has no DELETE endpoint, so these need raw SQL).

Run locally with `ci-all-in-one/secrets/.credentials.env` sourced. Idempotent.

```bash
source /Users/admin/data0/private_work/ci-all-in-one/secrets/.credentials.env
bash scripts/repatch_hfrog.sh
```

## Migration note: Nexus → Cloudflare R2

We migrated all r_lit binary distribution from Nexus
(`nexus.gamesci-lite.com`) to Cloudflare R2 (`gamesci-lite.com`). All old
`nexus.gamesci-lite.com/repository/raw-prod/r_lit/...` URLs are dead.

The `mirror-nexus` job in the previous `release.yml` is gone. If you have
external scripts still pointing at Nexus, switch them to:

```
https://r2.gamesci-lite.com/r_lit/<tool>/install.sh
https://r2.gamesci-lite.com/r_lit/<tool>/v<ver>/<tool>-<target>.tar.gz
```

A scan of remaining Nexus references in the sister `ci-all-in-one`
repository (Jenkins pipelines, docs) lives at
[`docs/nexus-deprecation-audit.md`](nexus-deprecation-audit.md).
