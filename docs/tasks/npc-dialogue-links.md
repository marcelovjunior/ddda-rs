# npc-dialogue-links: NPC placement, ledger and dialogue links

- **Owner:** codex lane
- **Branch:** `task/npc-dialogue-links`
- **Board area:** `world/npc`
- **Status:** review

## Goal

Readers for the NPC ledger, MSS and MSL formats, joined with the existing layout (`mtlot`) and text (`mtgmd`) readers. Then trace how the NPC manager selects schedules, separately from generic dialogue playback.

## Evidence

Start from `docs/SURVEY.md`, section "6. NPC placement, schedules and dialogue": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/ddnpc/**`
- `docs/NPCS.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

All three new formats parse across the install, and one named NPC (named from the game's text) is traced end to end: placement, schedule entry, and dialogue lines. Coordinate with claude-7 (events-quests), who owns quest and event logic. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

Implemented `ddnpc` ledger/MSS/MSL readers, documented in `docs/NPCS.md`.
Checked 2026-10-09 against all 3,076 archive instances: ledger 1 path /
1 occurrence / 727 entries; MSS 930 paths / 1,855 occurrences / 830
distinct payloads / 8,676 distinct-payload conversations; MSL 783 paths /
1,220 occurrences / 752 payloads / 12,478 distinct-payload serials.
All parse with exact byte consumption. `cargo test -p ddnpc -- --nocapture`:
8 tests passed (7 synthetic regressions and one full-install integration).

Named NPC chain: ledger id 2 is Quina; layout
`scr/st100/etc/st100_55m45n_n180` record 0 enables daily schedules and
selects `ai/fsm/npc/st100/st100_n002`. Its morning condition uses
`GlobalTime >=36 AND <72` and loads `st100/action/go002_morning`.
Executable `0x007a32f0` transfers NPC id/schedule flag into uNpcIntel;
`0x00bbd930` constructs the schedule/action owners; `0x00bbdaa0` selects
stage/scenario-specific controllers; `0x0059d790` computes ten-minute
clock buckets. Selection/clock helpers have boundary regressions.

Quina's `id/npc_wind/npc_base_talk/n002` MSS conversation 7 points to
English GMD index 6 (paired MSL serial 850); conversation 76 points to
indices 13/14. Resource binding `0x00bace50` / `0x00457670`, row lookup
`0x00cc9ca0` and GUI consumer `0x006a2eb0` / `0x006a56e0` were traced.
The MSS loader `0x00cc95f0` overwrites runtime per-line quest numbers
with the serialized quest-flag array, leaving actual runtime quest flags
at -1. Reader preserves both arrays; fresh-load lookup quirk is tested.

Verified by data/executable, **not by a live interaction**. Remaining:
conversation eligibility/choice execution, MSL serial namespace and all
consumers, quest/daily FSM arbitration, manager spawn priority and route
following. A schedule transition is not asserted to trigger the example
dialogue. Next action: original-game Quina interaction and 06:00 transition
recording with controller/conversation/GMD IDs, coordinated with the oracle
owner. Quest/event logic stays in claude-7's lane; no core files edited. Cargo.lock contains only the new ddnpc package entry.
