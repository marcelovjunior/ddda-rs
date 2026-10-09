# enemy-families: Wolves and harpies: think tables, actions, flight

- **Owner:** codex lane
- **Branch:** `task/enemy-families`
- **Board area:** `fight/enemies`
- **Status:** open

## Goal

Extend the goblin slice (`ddenemyrt`, `ddai::thinktbl`, docs/AI.md "The goblin unit") to the two families near Cassardis that `ddspawn` already places: wolves (group 252 at night) and harpies. For each: think-table rows, action classes and motions, turn and movement (harpy flight and dives), hit volumes, and death. Spawn them from `ddspawn` in `--play --save`.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs must not grab the mouse or pause on focus loss.

## Scope

- `crates/ddenemyrt/**`
- `crates/ddai/**`
- `crates/ddspawn/**`
- `docs/AI.md`
- `docs/COMBAT.md`
- this brief's Results

Shared viewer files (`play.rs`, `main.rs`) need a board claim and
coordination with the agent that holds them.

## Done when

Headless fights: an Arisen vs a wolf pack and vs a harpy, with deterministic tests. Wolves and harpies appear and fight in `--play --save` at their spawn times. Fill in Results and report to `head`.

## Results

Work in progress: installed wolf/harpy data, sourced think helpers and selected
action execution are implemented. Shared Fight processing now includes both
families in damage/status, push-out, player spell targeting and pawn inputs.
All 35 ddenemyrt tests pass, including deterministic installed-data action and
two-seat mixed-family hit tests (one debug probe ignored). Details and source
addresses are in docs/AI.md.

Not complete: autonomous family think/sensing, forced damage/death dispatch,
spawn/viewer/network integration and original-game fight comparisons. Normal
wolf death uses common action 0x03000042 (0x008f6230), distinct from the local
unsupported cEm0200Die motion. Next: trace that registration and wire death.
