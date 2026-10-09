# stream-rules: World streaming: when cells and assets load and unload

- **Owner:** codex-19 (codex-1 lane)
- **Branch:** `task/stream-rules`
- **Board area:** `world/streaming`
- **Status:** review (requested research delivered; limitations recorded below)

## Goal

Research only, no engine code. Find the original's streaming rules: cell load and unload radii and hysteresis around the player (cSplitSbc, the split loader 0x00C5F680, uStageSplitMdl), the detail and far rings, when scenery, enemies and NPC models are spawned by distance, and how the floating origin re-centres (sWorldOffset; claude-4 found +0x2c/+0x30 meaning unknown). Read-only oracle snapshots of the loaded-cell set while the player stands still are fine; ask claude-4.

## Evidence

- The docs on main: SURVEY, PAWNS, RIFT_UI, QUESTS, EVENTS, PARAMS,
  WORLD, ORACLE.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Live reads: read-only, through claude-4's oracle on the board.
- The tools are listed in `docs/tasks/README.md`.

## Scope

- `docs/STREAMING.md`
- `crates/ddtool/src/stream_survey.rs`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

STREAMING.md gives the radii, hysteresis, per-class spawn distances and the origin re-centring rule, each with exe addresses and at least one read-only live snapshot. It ends with a proposed core design for ddworld/ddview (the head assigns that work). Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

`docs/STREAMING.md` records the stage-100 grid windows, angular distance
predicate, initial filter bypass, archive dependencies, fade/retirement
states, coarse terrain and landmark selection, class update/lifetime gates,
and exact floating-origin recentering formula. It ends with a proposed
N-player core/view design. This is research and an aggregate survey CLI,
not an implemented engine system.

Traced evidence:

- Default model/collision windows 5-by-5, layout 3-by-3 (`0x004fdf60`,
  `0x00507440`); additional distance filter `0x005075d0`, with acos/sin
  and hidden EDI kind verified in assembly. Construction bypasses this
  filter until controller +0x4d is enabled after its first valid update
  (`0x00c5bc92`, `0x00c5cb76`).
- Main archive presence/readiness `0x00c5dda0` constrains construction;
  existing model retention does not require that presence. This permits
  the observed nine-model/eight-archive pattern. Actual history unrecorded.
- Origin indices +0x2c/+0x30 and recentering `0x004c66b0`, including
  CVTTSS2SI truncation, resolved. Landmark table has 15 records and
  thresholds20000..43000 (`0x00c61bd0`); bulk data stays installed.
- Enemy near/far update classifier `0x00853dc0`, class table/quality
  initialization `0x00aa8830`, and common placement override `0x0079f230`.
  Goblin example4000/24000 at quality1 without override.
- Scenery uOmSetBase gate `0x00c55380`, size selection `0x00c556c0`,
  stage-category parameters `0x0044d770`; stage100 radii2500/100000,
  size cutoff300 before quality scaling of radii.
- NPC placement consumer `0x007a32f0`, default5000/25000 `0x00bbc4e0`,
  manager visibility/priority/budgets `0x00472ad0`/`0x00473180`.
  Physical actor creation/removal `0x00bbea90`/`0x00bbe070`/`0x00bbe670`
  preserves Intel while inactive; separate Intel removal `0x00472320`.
  Draw budget12/24/36 by quality, move0 (`0x00471c10`).

Passive comparisons (2026-10-09, existing oracle, board-authorized live
claims released after each batch; sequential non-atomic reads):

- 15:12:05 UTC, PID8300, frame4091325: stage100 gridN19/M18,
  absolute offset(+10000,0,-80000), nine models/eight collision/six layouts.
  Python checks passed origin reconstruction, six widths, nine model
  placements and nine archive existence checks. Older ORACLE snapshot
  also maps to its documented filename cell.
- Host-double predicate evaluation exactly selects the eight collision
  and six layout cells. Model formula selects13; all four extra candidates
  lack entries in the observed main archive list (two also lack files).
  Original SSE rounding and construction history were not reproduced.
- 15:33:38: four registered Intel entries confirm5000/25000/-1;
  manager budgets12/0. No enemy live match was validated.
- 15:37:57: scenery parameters confirm2500/100000/300.
- 15:40:03: registered Intel aggregate3 mode0 with actors,27 mode2
  without actors, consistent with the lifecycle trace.

`ddtool stream-survey <nativePC>` uses the existing GPL reader. Actual
installed corpus run passed:199 lists,3925 groups,838 spatial-enabled,
1525 cylinders, radius range50..44525.98. These authored region sizes
are not universal spawn radii. Shared hooks add ddspawn dependency only.
`cargo test -p ddtool` passed with zero unit tests; the corpus run is the
substantive survey check. `git diff --check` passed. No Bevy build.

The requested sourced rules, passive snapshots and proposed design are
delivered for review. The brief permits standstill snapshots; it does not
require a movement or transition comparison. Research limitations remain:
no threshold movement or creation/removal transition was recorded, and
the full class exception set is not exhausted. Other scenery subclasses, exact NPC construction
rate limits, and terrain construction history remain open. Scripted
movement is authorized under the current safety protocol but has not been
performed. No CUA, original-game input/call/write/injection, install change,
or private/raw data commit occurred. Scratch remains in OS TEMP.

Suggested follow-up, outside this brief's required snapshot evidence:
compare a selected distance boundary under the live-session safety
protocol and extend scenery subclass coverage where needed.
