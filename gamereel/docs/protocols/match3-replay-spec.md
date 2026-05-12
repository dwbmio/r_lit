# match-3 / 方块消除 replay protocol — v0.1 draft

> Contract between game server and `proto-puzzle`. Game side emits this
> stream per match; `proto-puzzle` deserializes and translates to
> `gamereel-core::Scene + Timeline` for rendering.
>
> **Status: draft**. Schema choices marked `// open question` need a
> game-side review pass before locking in v1.

## Goals

1. **Compact**: a 3-minute match at 30 fps = 5400 frames, but the
   replay shouldn't carry 5400 full board states. Emit *deltas*
   (events) — typically 50–500 events per match.
2. **Deterministic**: same byte stream → same video, byte-identical.
   No clock-derived randomness, no "current time" reads in parsing.
3. **Forward-compatible**: future game features (special pieces,
   power-ups) add new event types without breaking old replays.
4. **Decode-only**: `proto-puzzle` MUST NOT re-simulate the game.
   Every state needed for rendering is in the stream. If you'd need
   to re-run the match logic to render correctly, the stream is
   missing data — fix the stream, not the parser.

## Wire format

**Choice: protobuf** (proto3) for v1. Reasons:
- Schema evolution out-of-the-box (add fields, never remove).
- Multi-language game-server side (likely C# / Go / Java / TS).
- Compact binary; ~5–10× smaller than JSON for event streams.
- Tooling: `prost` on Rust side, official codegens on every server lang.

For v0 (this draft), use serde-json so we can iterate the schema fast
without committing to .proto files yet. Switch to protobuf once schema
stabilizes — `proto-puzzle::parse` swaps decoder, no Scene-translation
changes.

## Top-level shape

```jsonc
{
  "version": 1,
  "match_id": "battle-2026-05-12-001",
  "player": {
    "id": "u-12345",
    "name": "玩家昵称",          // shown in result overlay
    "avatar_tp": "avatar-12"     // texture id, see Resource section
  },
  "board": {
    "rows": 8,
    "cols": 8,
    "cell_size_px": 72            // sprite size, drives render scale
  },
  "duration_ms": 38500,           // sum of event durations + final pause
  "init": { /* BoardInit, see below */ },
  "events": [ /* ordered, monotonic timestamps */ ]
}
```

### Resource references

Replays carry **logical IDs** (`"avatar-12"`, `"block-red"`,
`"explosion-1"`), not pixels. The `gamereel`-side caller is responsible
for mapping IDs → texture file paths (typically via a sprite manifest
file). This keeps the replay payload tiny and lets art swap without
re-encoding old replays.

## Events

All events have:

```jsonc
{
  "t":   12340,        // ms since match start, monotonic
  "kind": "swap",       // discriminator
  /* kind-specific fields */
}
```

`kind` values planned for v0:

| kind | when emitted | rendering implication |
|---|---|---|
| `board_init` | once at match start (also embedded as `init` field above for convenience) | place the 8×8 grid of starting blocks |
| `swap` | player swaps two adjacent cells | animate two cells exchanging positions over `duration_ms` |
| `match` | engine detected matched group | flash + remove cells listed; emit at the *start* of the clear animation |
| `cascade_drop` | matched cells removed; remaining cells fall into the gaps | per-column drop animation; cells move along Y axis |
| `cascade_spawn` | new blocks dropped from above to fill gaps | spawn animation: cells fade in from above the grid |
| `power_clear` | a power-piece (line clear, rainbow, bomb, etc.) fires | per power-type effect; carries an `effect_id` |
| `score_change` | player's score updated | animate the score counter |
| `combo` | a sequence of matches without re-input forms a combo | show combo counter + multiplier |
| `time_pause` | server-side decision to pause replay (e.g., between moves) | renderer holds the last frame for `duration_ms` |
| `match_end` | match concluded — win/lose/timeout | switch to result overlay |

### Event field detail

```jsonc
// board_init
{
  "t": 0, "kind": "board_init",
  "cells": [
    [{"piece": "blue"},  {"piece": "red"},   /* ... */],   // row 0
    /* ... rows 1..7 */
  ]
}

// swap (always 4 cells: 2 originals + 2 destinations after swap)
{
  "t": 1200, "kind": "swap",
  "duration_ms": 240,
  "from": [3, 5],   // [row, col]
  "to":   [4, 5]
}

// match
{
  "t": 1440, "kind": "match",
  "cells": [[3,5], [4,5], [5,5]],   // cells being cleared
  "match_type": "vertical_3",        // for VFX selection
  "score_gain": 50
}

// cascade_drop  (one event per column that has falls)
{
  "t": 1640, "kind": "cascade_drop",
  "duration_ms": 180,
  "col": 5,
  "moves": [
    {"from_row": 2, "to_row": 4},
    {"from_row": 1, "to_row": 3},
    /* ... bottom-to-top so renderer can animate sequentially */
  ]
}

// cascade_spawn
{
  "t": 1820, "kind": "cascade_spawn",
  "duration_ms": 200,
  "col": 5,
  "spawns": [
    {"to_row": 0, "piece": "green"},
    {"to_row": 1, "piece": "yellow"},
    /* ... */
  ]
}

// power_clear
{
  "t": 6240, "kind": "power_clear",
  "effect_id": "line_clear_horizontal",
  "origin": [4, 5],
  "cells_cleared": [[4, 0], [4, 1], /* ... */, [4, 7]],
  "duration_ms": 600
}

// score_change
{ "t": 1500, "kind": "score_change", "from": 250, "to": 300, "duration_ms": 400 }

// combo
{ "t": 1820, "kind": "combo", "count": 3, "multiplier": 1.5, "duration_ms": 500 }

// time_pause
{ "t": 38000, "kind": "time_pause", "duration_ms": 500 }

// match_end
{
  "t": 38500, "kind": "match_end",
  "result": "win",                  // "win" | "lose" | "timeout" | "abort"
  "final_score": 12450,
  "stats": { /* arbitrary k/v rendered in result overlay */ }
}
```

## Translation to `Scene + Timeline`

`proto-puzzle::parse` walks events in order and emits a `Scene`:

1. **Static layer**: background plate (caller-supplied texture id).
2. **Board grid**: 8×8 = 64 nodes, one per cell. Each carries the
   current `piece` and starting position. Built from `board_init`.
3. **Score / combo / overlay** nodes: lazily added when first
   `score_change` / `combo` event hits.
4. **Timeline**: each event becomes one or more `MetaAction`s on
   the corresponding nodes:
   - `swap` → two `move_to` actions on the swapped cells
   - `match` → `active = false` actions (with optional fade) on
     cleared cells
   - `cascade_drop` → `move_to` per cell-fall
   - `cascade_spawn` → `active = true` + `move_to` from above-grid
     to target row
   - `power_clear` → spawns a one-shot effect node, emits
     `active = true` then `active = false` after `duration_ms`
   - `score_change` → number tween on the score node
   - `combo` → spawn + auto-remove combo banner node
   - `time_pause` → no node-level work; renderer's frame loop
     respects the pause window
   - `match_end` → switch active overlay to result

The translation is **mechanical** — for each event variant there's
exactly one (parser-side) function emitting nodes/actions. No game
logic. No re-simulation.

## v0 → v1 evolution path

- v0 (this doc): JSON wire format, hand-coded encode/decode in
  `proto-puzzle`. Goal: validate the schema is sufficient.
- v1 (after v0 ships and we discover what's missing): port to
  protobuf (`gamereel/protos/match3.proto`); generate Rust types
  via `prost-build`; game-side gets official codegen. Same Scene
  translation logic, only the byte-decoder changes.

## Open questions for game-side review

1. **Cell coordinate origin**: spec assumes `[0,0]` is top-left. Confirm
   this matches your engine's convention; if not, parser handles the
   flip — but pinning it now avoids a quiet visual bug later.
2. **Time base**: `t` in milliseconds since match start. Confirm
   monotonicity is server-guaranteed (not derived from client clock
   that could jump).
3. **Power-piece taxonomy**: what `effect_id` values exist?
   Renderer needs an asset-side mapping; if you have a fixed list of
   ~10, hardcode in the manifest. If it's open-ended, use a fallback
   "generic explosion" effect.
4. **Avatar / skin variation**: do players have customizable boards
   or block skins? If yes, the texture ids in `board_init.cells[].piece`
   need to be per-player — the manifest keying becomes
   `(player_id, piece_kind) → texture_path` instead of just
   `piece_kind → path`.
5. **Replay length**: what's the median match length? Spec assumes
   ≤ 5 minutes. Longer replays may want chunking (split into N
   segments encoded separately, then concat).
6. **Mid-replay save**: do you ever want to render a partial replay
   (e.g., "highlight reel" = subset of events)? Affects whether
   `proto-puzzle` exposes a "filter events by time range" API.

Responses to these questions feed v0.2 of this spec, then locking v1
protobuf.
