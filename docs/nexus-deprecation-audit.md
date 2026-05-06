# Nexus → Cloudflare R2 — Audit of remaining references

> Generated 2026-05-06 from a scan of `/Users/admin/data0/private_work/{r_lit,ci-all-in-one}`.
>
> r_lit itself is fully migrated (see [`docs/release.md`](release.md)). The
> sister repo `ci-all-in-one` still contains many references — listed below
> grouped by **what action is needed**, not just what the file says.

## In r_lit (this repo) — done

| Path | Status |
|---|---|
| `.github/workflows/release.yml` | ✅ rewritten — `mirror-nexus` job removed, `mirror-r2` added |
| `bulk_upload/README.md` `README_CN.md` `llms.txt` `llms_cn.txt` | ✅ install URLs swapped to `https://gamesci-lite.com/r_lit/...` |
| `README.md` `README_CN.md` | ✅ added Install + Release sections pointing at R2 |

No other r_lit file references Nexus.

## In ci-all-in-one — categorised

### Category A — **Active publishers** (Jenkins jobs still uploading to Nexus)

These will keep producing dead artifacts at `nexus.gamesci-lite.com/repository/raw-prod/` because Nexus is being decommissioned. **Each needs to be ported to R2 (same pattern as r_lit's `mirror-r2` job) or retired.**

| Project | Pipeline | Suggested action |
|---|---|---|
| hfrog | `task/ci/pipeline/hfrog/Jenkinsfile.binary-build` `Jenkinsfile.binary-build-parallel` `Jenkinsfile.build-deploy` | Port upload step to R2; service is still in heavy use |
| hfrog-cli | `task/ci/pipeline/hfrog-cli/Jenkinsfile.binary-build` `Jenkinsfile.binary-build-parallel` | Port |
| hfrog-gql | `task/ci/pipeline/hfrog-gql/Jenkinsfile.binary-build` | Port |
| pgpour | `task/ci/pipeline/pgpour/Jenkinsfile.binary-build` | Port |
| titan-forge | `task/ci/pipeline/titan-forge/Jenkinsfile.{binary-build,admin-build,admin-deploy,deploy}` | Port |
| wirerope-any | `task/ci/pipeline/wirerope-any/Jenkinsfile.{multi-arch,dynamic-build}` | Port |
| godot | `task/ci/pipeline/godot/{editor,engine,gd-minip-template,games/*}/Jenkinsfile` | Port (large game artifacts may justify keeping a separate R2 prefix) |
| hybrix | `task/ci/pipeline/hybrix/Jenkinsfile{,.android}` | Port |
| batch_jp_url_to_s3 | `task/ci/pipeline/batch_jp_url_to_s3/Jenkinsfile` | Port |
| **r_lit (legacy)** | `task/ci/pipeline/r_lit/Jenkinsfile.binary-build` | **Delete** — replaced by `r_lit/.github/workflows/release.yml` in this repo |

### Category B — **Install scripts pointing at dead Nexus URLs**

End users running these get 404s today. Replace each with the same template r_lit now uses (`r_lit/scripts/install.sh.template`) hosted on R2.

| File | Status |
|---|---|
| `task/ci/pipeline/r_lit/scripts/install.sh` | **Dead — delete** (replaced by `r_lit/scripts/install.sh.template` rendered per-tool to R2) |
| `task/ci/pipeline/pgpour/scripts/install.sh` | Rewrite for R2 |
| `task/ci/pipeline/hfrog-cli/scripts/install.sh` | Rewrite for R2 |
| `task/ci/pipeline/hfrog/scripts/install-hfrog.sh` | Rewrite for R2 |
| `task/ci/pipeline/hfrog/scripts/build-upload-local.sh` | Rewrite (publishing target) |
| `task/ci/pipeline/titan-forge/scripts/install-titan.sh` | Rewrite for R2 |
| `_ai/skills/devops/install-rust-binary.sh.template` | Update template to R2 baseline |

### Category C — **Infrastructure for Nexus itself**

Nexus the service is still booted; if it's truly being shut down, also tear these down.

| Path | Question |
|---|---|
| `compose/nexus/docker-compose.yml` `nginx.conf` `README.md` | Keep until last consumer migrated, then archive |
| `compose/gateway/conf.d/nexus.conf` | Stop routing once decommissioned |
| `scripts/services/nexus/deploy.sh` `deploy-remote.sh` | Move to `archived/` once stopped |
| `compose/nexus/.env` (not on disk?) | Check |
| `task/envs/nexus/.nexus.gamesci-lite` | Delete or move to `archived/` |

### Category D — **Credentials & global env**

| File | Action |
|---|---|
| `secrets/.credentials.env` | Keep `NEXUS_GAMESCI_*` until Category A jobs are all ported, **then delete those three lines (18-20)** |
| `task/envs/jenkins/init-credentials.groovy` | Drop `nexus-gamesci-lite` Jenkins credential after port |

### Category E — **Documentation only**

These are descriptive — fix opportunistically along with the corresponding code change. Not blocking.

```
_ai/rules/devops/{ci,cd,docker}.md
_ai/rules/backend/gql.md
_ai/rules/{rlit/_tech-stack,mobile/{apk-build,apk-artifact,apk-naming},rtk,rtk-reference}.md
_ai/skills/devops/{rust-binary-ci,wirerope-gql-build,hfrog-build-deploy}.md
_ai/skills/mobile/apk-build-ci.md
_ai/reference/{infrastructure,credentials-architecture,ci-hfrog-sync-and-scripts,apk-build-scripts}.md
_ai/projects/{infra/service-registry,backend/{wirerope-hfrog,wirerope-life-wenshui}}.md
_ai/machines/{offline/{README,dwb-z490},}.md
README.md  CHANGELOG.md  claude.md
docs/services/picboo/{README,ANDROID_BUILD}.md
task/claude-plugins/gamesci-devops/{rules,skills}/**  (mirrors of _ai/)
task/claude-plugins/manage.sh
```

## Recommended migration order

1. **Now** — kill the dead `task/ci/pipeline/r_lit/{Jenkinsfile.binary-build,scripts/install.sh}` (this repo's GitHub Action superseded it).
2. **Next sprint** — port the highest-traffic services (`hfrog`, `hfrog-cli`, `titan-forge`) to R2 using the same `aws --endpoint-url $R2_ENDPOINT s3 cp` pattern as `r_lit/.github/workflows/release.yml`'s `mirror-r2` job.
3. **Then** — port `wirerope-any`, `pgpour`, `hybrix`, `batch_jp_url_to_s3`.
4. **Then** — port godot pipelines (largest payload, may want dedicated bucket).
5. **Finally** — tear down Category C / D, then drop Category E docs in one sweep.

## Reference: the 3-line R2 upload pattern

Anywhere a Jenkinsfile / shell script today does:

```bash
curl -fsS -u "$NEXUS_USER:$NEXUS_PASS" \
  --upload-file ./build.gz \
  "https://nexus.gamesci-lite.com/repository/raw-prod/<proj>/v1.2.3/<target>/build.gz"
```

…replace with (R2 is S3-API compatible):

```bash
aws --endpoint-url "$R2_ENDPOINT" s3 cp ./build.gz \
  "s3://prod-gamesci-lite/<proj>/v1.2.3/<target>/build.gz" \
  --content-type "application/octet-stream" \
  --cache-control "public, max-age=31536000, immutable"
# Public URL → https://gamesci-lite.com/<proj>/v1.2.3/<target>/build.gz
```

Required env (from `secrets/.credentials.env`):

```
AWS_ACCESS_KEY_ID=$R2_ACCESS_KEY_ID
AWS_SECRET_ACCESS_KEY=$R2_SECRET_ACCESS_KEY
AWS_DEFAULT_REGION=auto
R2_ENDPOINT=https://240d77865abd8ef6f48521ba34845508.r2.cloudflarestorage.com
```

Bucket `prod-gamesci-lite` is bound to the public custom domain
`gamesci-lite.com`, so any S3 key becomes
`https://gamesci-lite.com/<key>` with no auth needed.
