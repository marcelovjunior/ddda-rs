# ambient-creatures: Cassardis's ambient creatures (uEm8300, uEm8602)

- **Owner:** codex lane
- **Branch:** `task/ambient-creatures`
- **Board area:** `fight/ambient`
- **Status:** in progress

## Goal

The doors runtime (codex-32) needs the enemy manager's real population:
fresh Cassardis GPL groups 1/3 admit two uEm8300 (around (404, 0, -1885)
and (301, 0, -2025)) and seventeen uEm8602 on the beach. The door's
auto-close occupancy test excludes kind 0x6B (uEm8600) but not 0x65
(uEm8300). docs/FRAME.md lists uEm8300 in the Obj move line; an oracle
session saw a static uEm8300 15 m from the party (docs/ORACLE.md).

Identify what these creatures are (models, motions, AI: uEm8300's think
interpreter is the shared one, docs/AI.md), spawn them from the GPL like
other enemies (ddspawn / ddenemyrt), run their behaviour (idle, wander,
flee, despawn) as the original does, and expose their positions to the
host so door occupancy and anything else reading the enemy manager see
them. Build it through the generic enemy runtime, not as a door special
case.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md` (the original is in
Cassardis right now).

## Scope

`crates/ddenemyrt`, `crates/ddspawn`, `crates/ddhost` enemy feed, the
viewer's enemy drawing, `docs/ENEMIES.md`, this brief's Results.

## Done when

In a fresh opening, Cassardis's ambient creatures appear where and as the
original shows them (oracle check), and the door occupancy reads their
positions. Fill in Results and report to `head`.

## Results

### First source checkpoint

Added installed resource/action readers and concrete kind identity in
`ddenemyrt::ambient`. Both models and three motion banks each load; Wait
motions exist; default think roots parse. One installed-data test and
one kind regression pass. Strict crate-only clippy passes with the
pre-existing `combat.rs` `type_complexity` lint allowed.

Important correction: uEm8602 constructor `0x00aa0000` writes kind 0x6d,
not its parent uEm8600 kind 0x6b. Layout e03 says `cSetInfoEnemy8600` but
names em8602. Root/head notified for door stand-in correction.

Source details in ENEMIES.md. No live original comparison yet. Next:
resolve family action entry/think inputs, then canonical host population
and viewer bodies; authored positions alone are not runtime behavior.


### Checkpoint66a5435

Read-only live identity snapshot confirms kind65/6d and authored first
tables6/5. Added shared bank reader and sourced Wait/WaitAir/FlyStart
callbacks. Three unit tests, one installed resource/action test and one
installed spawn regression pass. Strict no-deps clippy passes with the
pre-existing combat type_complexity allowance. ENEMIES.md has sources
and live limitations. Host/renderer hookup and movement remain open.


### Flight and lifecycle groundwork

Added sourced bird flight position/heading update (0x00aa1030/0x00aa0a80)
and independent group removal rules (0x0075f860). Immutable species model,
skeleton and bank loader is available for host instances. Four unit tests,
installed action/table test and installed spawn/removal test pass; targeted
clippy passes with the existing allowance. Authored first tables actually
select the live observed idle actions through the shared interpreter.

Still pending: actor owner pool, host frame-loop and renderer integration,
bird target/height/speed observations, and moving-original comparison.
