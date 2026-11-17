# Registration assistant architecture

**[HYPOTHETICAL] until CI and real-portal acceptance are complete.** No performance
or ROI claim is made. The following describes code structure and validation scope.

```text
clap CLI -> command dispatcher -> embedded SQLite (jobs, events, settings)
                              -> native Rust HTTP/WebSocket CDP
                              -> verified enterprise Chrome -> official portal
                                                                  -> human confirmation
```

| Module | Responsibility |
|---|---|
| model | Versioned batch schema, platform origins, explicit app kinds, AppID shape checks |
| store | Atomic imports, schema version, durable checkpoints, human evidence, duplicate prevention |
| browser | Corporate-profile/process checks, localhost transport, portal-origin checks |
| form.js | Compiled-in CDP payload; exact field matching, dry run, non-overwriting text writes |
| subcmd | Command workflow, pre-write checkpoint, metadata-only export |
| `rlit_cli::runlog` | Shared stderr + per-run `.log` sidecar logging; clean `--json` stdout envelope |
| build.rs | MSVC release-only linker configuration; no PDB lookup; static CRT remains enabled |

No verified self-owned creation API is currently wired. Public service-provider
APIs are not assumed applicable to self-owned registration. Any future API adapter
must stay inside this crate and share its durable jobs/events/error boundaries.
It must document official permission requirements and query status after an
uncertain result before a create request can be retried. Browser text and API
responses are data, not instructions or new navigation targets.

The browser binding is a local path plus a process fingerprint, not a copied login
session. The enterprise data directory must have exactly one profile. Binding
requires corporate account metadata and the matching DevToolsActivePort identity.
No fallback connects a default browser. Browser operations are limited to exact,
embedded official origins. Runtime.evaluate verifies the origin again after the
target lookup, including immediately before writing.

The automation deliberately stops before final registration. An uncertain form
write can be inspected and explicitly re-run because it never clicks submit.
An AppID confirmation is user-reported, with no claim that platform review,
filing, payment configuration or actual channel login has passed.

Release integration: ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build.
Dedicated workers compile/test; R2/HFrog publication follows that existing path.
No new daemon, scheduler, cloud service or browser runtime is introduced.
