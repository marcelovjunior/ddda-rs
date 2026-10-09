# stream-rules: World streaming: when cells and assets load and unload

- **Owner:** codex-19 (codex-1 lane)
- **Branch:** `task/stream-rules`
- **Board area:** `world/streaming`
- **Status:** review (partial research; full distance/transition gate remains open)

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

`docs/STREAMING.md` records the traced stage-100 pipeline, default 5-by-5
model/collision and 3-by-3 layout candidate windows (`0x004fdf60`,
`0x00507440`, `0x00c5f070`, `0x00c5f680`, `0x00c60510`), additional distance
filter (`0x005075d0`), model fade and layout retirement states, regional
coarse-model variants, and 15 distance-selected landmark records
(`0x00c61bd0`, installed-exe table `0x01873550`, stride 0x40; aggregate
threshold range 20000..43000). The origin indices and exact recentering
formula are resolved (`0x004c66b0`, verified CVTTSS2SI truncation).

Passive snapshot obtained with head #1292 authorization, 2026-10-09
15:12:05 UTC, PID 8300, frame 4091325: stage100, grid indices N19/M18,
absolute offset (+10000,0,-80000), nine models/eight collision/six layouts.
Python checks passed for origin reconstruction, all six live widths, all
nine model placements and existence of their nine installed cell archives.
Older ORACLE.md indices22/23 also resolve to its recorded 63m52n cell.
This was a paused snapshot, not a boundary-crossing comparison. Probe
operations were info/read only; live/game claim released. Scratch and raw
data remain in OS TEMP. The initial batch committed no engine/reader code or game data; git diff --check passed.

Follow-up assembly identified acos/sin and EDI kind 0/1/2 at all six
model/collision/layout retention/construction call sites. A private Python
formula evaluation exactly reproduced the eight collision and six layout
cells. It selected 13 model candidates versus nine resident models: two
extra candidates lack installed archives; two have archives. A later
passive archive-list read found eight entries matching the collision set.
This discrepancy remains open rather than treating desired candidates as
resident models. Original SSE threshold rounding was not emulated.

The full done-when is **not yet met**. Individual scenery distances and the complete actor creation/removal
lifecycle remain unresolved. Group activation has authored shape gates
(`0x00cc5670`), so no universal class radius is claimed. No moving live
comparison was requested or performed. The doc ends with an N-player
interest/residency and per-view floating-origin design proposal.

Next action: resolve the terrain archive/residency discrepancy, then trace per-class distance
consumers and obtain maintainer-provided ordinary crossing observations.

Further trace resolves enemy near/far update modes (`0x00853dc0`,
`0x00aa8830`) and the NPC placement draw-distance consumer (`0x007a32f0`)
into Intel thresholds, default 5000/25000 (`0x00bbc4e0`). Manager visibility,
priority and active budgets intervene; full policy exceptions remain open.
A passive registered-list read found 30 Intel entries; four sampled entries
match 5000/25000/-1, with manager budgets 12/0. No enemy live match was
validated. No original process control or writes occurred.

Added `ddtool stream-survey <nativePC>` using the existing GPL reader.
Actual corpus run passed: 199 lists, 3925 groups; 838 spatial-enabled
across suffix e/t, 1525 cylinders, radius range 50..44525.98. Reports are
aggregates only. Shared ddtool hookup adds ddspawn as a dependency; no
engine code or private exports. Full brief remains partial, with threshold
movement comparisons and terrain residency discrepancy still open.
