# mini_reg

Independent, short-running r_lit Rust CLI. Read README.md/README_CN.md before edits.
The parent r_lit CLAUDE.md and `_ai/sub-tool-docs.md` apply.

- Keep persistent state in embedded SQLite, not session memory or a daemon.
- Keep all transport and orchestration in Rust; bundle any CDP form payload.
- Never add Python/Node/Playwright runtime dependencies or shell-out API clients.
- Only operate the explicitly verified gamesci-lite.com enterprise Chrome.
- Never collect browser cookies, passwords, tokens or raw form/page dumps.
- Unknown/ambiguous fields stop; no submit, identity, consent or payment automation.
- Commit needs_review before every form write; never auto-retry final registration.
- confirmed is human evidence; platform_verified remains false until a verified official API exists.
- Build/test on dedicated CI workers; never on controller/production/limited hosts.
- Update README/README_CN/llms/llms_cn with behavior changes.
- Architecture and verification boundaries: docs/ARCHITECTURE.md.
