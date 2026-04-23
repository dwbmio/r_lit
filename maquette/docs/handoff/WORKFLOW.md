# Handoff Protocol — How Maquette iterates across sessions

> This document is **read by the agent at the start of every session** that
> says "continue maquette". It is the contract that lets a fresh context
> pick up exactly where the last session left off, without rereading the
> full chat history.

---

## The Loop (single-session view)

```
    ┌─────────────────────────────┐
    │ Read NEXT.md                │
    │ Read latest vX.Y-complete.md│
    │ Read COST_AWARENESS.md      │
    └──────────────┬──────────────┘
                   ▼
    ┌─────────────────────────────┐
    │ Execute the sub-tasks       │
    │ in NEXT.md, in order        │
    │                             │
    │ For each sub-task:          │
    │  • small, focused change    │
    │  • cargo check + clippy     │
    │  • short progress note      │
    └──────────────┬──────────────┘
                   ▼
       ┌───────────┴───────────┐
       ▼                       ▼
   Decision point?         All sub-tasks done?
   → Pause, ask user      → Write vX.Y-complete.md
                          → Update NEXT.md → next version
                          → Report to user
```

## The Loop (multi-session view)

- Every time the user opens a new chat and says **"continue maquette"**
  (or similar), the agent's first three actions are:
  1. `Read maquette/docs/handoff/NEXT.md`
  2. `Read` the latest `vX.Y-complete.md` referenced by NEXT
  3. `Read maquette/docs/handoff/COST_AWARENESS.md`
- After those three reads, the agent has everything it needs to resume.
  No need to grep the codebase to guess what's going on.

---

## File Conventions

Inside `maquette/docs/handoff/`:

| File | Role |
|------|------|
| `WORKFLOW.md` | This file — the protocol itself. Rarely changes. |
| `COST_AWARENESS.md` | Long-lived product wisdom: what's expensive, what's cheap, what is the North Star. Append-only. |
| `NEXT.md` | Always points to the current in-flight version and its sub-tasks. Overwritten every version. |
| `vX.Y-complete.md` | Immutable record of what `vX.Y` delivered + hand-off to the next. Never edited after creation. |

---

## `vX.Y-complete.md` Schema (mandatory sections)

```markdown
# Handoff · maquette vX.Y → vX.(Y+1)

## Version Delivered
- Version: 0.X.Y
- Date: YYYY-MM-DD
- Session: brief agent-visible summary (1–2 sentences)

## What Was Built
### User-visible
- bullet list of what a user can now do that they could not do before
### Under the hood
- bullet list of new modules, resources, systems, messages, dependencies

## Decisions Made
| Decision | Choice | Rationale |
|----------|--------|-----------|
| (each closed question recorded here so we don't reopen it) | | |

## Known Issues / TODO
- Things the version left unfinished on purpose (scoping, not bugs)
- Actual bugs if any (rare; we want these fixed before ship-it)

## Files Touched
- path/to/file.rs — what changed and why
- (keep this surgical; this is NOT a diff)

## Next Version Contract
### Goal (one sentence)
### Deliverables
- concrete, user-testable outcomes
### Sub-tasks in order
- [ ] …
- [ ] …
### Technology additions
- new crates / shaders / tools
### Open decisions that MUST be asked
- "Which X do you prefer?" (with options)
### Exit criteria
- what the next version must satisfy to be called "complete"

## Resume Instructions
- The exact three or four things a fresh agent should do first.
```

## `NEXT.md` Schema (always short)

```markdown
# NEXT · maquette

Current in-flight: **vX.Y**
Reference: maquette/docs/handoff/v(X.Y-1)-complete.md

## Remaining sub-tasks (ordered)
- [ ] …
- [x] (completed items stay, for visibility)

## Pending decisions (block progress)
- none, or list them here
```

---

## Agent Behaviour Rules

1. **Never skip reading NEXT.md + COST_AWARENESS.md on session start.** They're short.
2. **Stop at decision points.** Do not guess on product-level choices that
   `Next Version Contract > Open decisions` lists. Ask the user.
3. **Write handoff at version completion, not per sub-task.** Sub-tasks update
   NEXT.md checkboxes only.
4. **Keep `vX.Y-complete.md` immutable.** If something was wrong, write it in
   the NEXT version's "Decisions Made" as a correction, never edit history.
5. **Honour COST_AWARENESS.md.** Before adding a new feature, check if that
   feature appears in the "expensive but tempting" list; if it does, flag it
   and ask the user before committing effort.
6. **`cargo check` + `cargo clippy -- -D warnings`** must pass before
   declaring a sub-task complete.
7. **Never run `git commit`** unless the user explicitly asks. Version
   boundaries are NOT commit boundaries — the user decides when to commit.

---

## Decision-Point Discipline

Roadmap-level decisions that REQUIRE the user:

| Milestone | Question |
|-----------|----------|
| v0.2 B | Toon shader path: custom WGSL Material vs. `bevy_mod_outline` vs. hybrid |
| v0.3 start | Canvas size upper bound (64 / 128 / 256?) |
| v0.4 mid | T-preview layout: 2×2 grid or classic T-shape |
| v0.6 start | FBX strategy: write ASCII FBX / require Blender CLI / glTF-only |
| v0.7 start | AI backend: ollama-local-first / cloud-API-first / both |
| v1.0 start | CI release pipeline: same as other r_lit crates or custom |

The agent proposes options; the user chooses.

---

## Minimum Viable Session

A session that only moves one sub-task forward is valid. Don't pressure
yourself into finishing a whole version — quality of the increment matters
more than breadth. Just update NEXT.md and stop cleanly.
