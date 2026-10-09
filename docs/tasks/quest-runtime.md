# quest-runtime: a headless story interpreter that runs the opening

- **Owner:** claude lane
- **Branch:** `task/quest-runtime`
- **Board area:** `story/runtime`
- **Status:** open

## Goal

Proposals 1 and 3 in `docs/QUESTS.md`: a headless crate (`ddstory`) that
holds the world state and runs the story's scripts the way the original
does, so that the opening plays from scripted input:
new game → prologue → q0000 → q0001 → q0002 boot.

**World state:**
- SceNo;
- the global story flags;
- the 96 bits per quest;
- layout flags;
- the per-script scratch flags and timers.

Plus a save/load of that state. Design it for N players: the host owns
the story (`docs/COOP_DESIGN.md`).

**Interpreter:**
- `cFSMOrder` scripts: the 70 order types. Implement the ones the opening
  uses; every other order returns an explicit "unimplemented" error, never
  a silent no-op.
- QCT evaluation: one row per pass, disabled after firing unless
  repeatable.
- Stage scripts that start on load, and the trigger volumes (`rScenario`
  areas) fed with player positions.
- **Outputs as requests to the host,** never direct calls into rendering:
  play event X, open menu Y, give items Z, move the party.

## Evidence

- `docs/QUESTS.md` and `docs/EVENTS.md`.
- `crates/ddquest` (merged).
- Text through `mtgmd`.
- Live comparisons through claude-4's oracle: read SceNo and the flags at
  key points.

## Scope

- `crates/ddstory/**` (new)
- `crates/ddquest/**` (reader fixes if needed)
- `docs/QUESTS.md` (runtime section)

## Done when

- A test drives the opening from scripted inputs and asserts the SceNo
  ladder and the flag and quest-bit states at each step, matching QUESTS.md
  and, where available, the original's values.
- Every order the opening touches is translated with its source address.
- Fill in Results and report to `head`.

## Results

(filled in by the agent)
