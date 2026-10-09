# combat-status: what a status effect does once it fires

- **Owner:** claude-8
- **Branch:** `task/combat-status`
- **Board area:** `research/combat`
- **Status:** review (claude-8, 2026-10-10)

## Goal

Finish proposal 3 in `docs/COMBAT.md`. The build-up is merged
(`ddcombat::status`). Now, for each of the 20 status types:
- its `rStatusParam` entry;
- its duration and how it ticks (per 1/30-s frame times the unit's
  `mDeltaTime`);
- its effect (damage over time, stat multipliers for the four "down"
  statuses, movement or action locks, special ones such as petrify, sleep
  and possession);
- its cure: items, time, rules.

Also how players and pawns build up statuses (the player side of
`0x007772B0`), the wall check (`0x00779270`), and the team IDs behind the
friendly-fire matrix. Item names for cures come from `ddeconomy` /
`dditem`.

## Scope

- `crates/ddcombat/**`
- `docs/COMBAT.md`
- `docs/STATUS.md` (new, optional)

## Done when

- Each status has its duration, tick and effect with addresses and tests,
  or a named gap.
- The live checks stay queued with claude-4.
- Fill in Results and report.

## Results

Write-up: **docs/STATUS.md** (COMBAT.md 7 links to it).

**Traced:** `cStatus::add` 0x00837090 (entry by kind, timer = `mTimer` x
30 frames, refresh vs stacks via `mIsOverwrite`/`mOverwriteNum`, the
conflict table 0x01509F20 through 0x00837730 / 0x00837810, kind 0x22
cleanse), `cStatus::update` 0x00836C70 (countdown on the global scaled
step, >= 10000 permanent), the per-kind handlers (vtable 137 + kind:
poison 0x00AAFE10 / 0x00B75070, slow 0x00852670, drenched 0x00852B10,
burning 0x00852F40 / 0x00B75890, frozen 0x00853060, petrify 0x00853260),
the damage-side effects (sleep, frozen, oil, drenched in 0x0084D8B0 and
0x00AAA490; curse and the downs / ups in the attack and defence code;
possession in 0x00B7AD90), cures (item messages 0x12..0x15 with masks
0xC0B / 0xF9F / 0x3C000 / 0x3D000 in 0x00B79FC0 / 0x00B5C3B0, healing
water 0x008542C0, time), the player build-up scaling by equipment
resistances (0x00B7AD90), the wall check 0x00779270 and the team matrix
0x0150A630 with the team ids set in the species' vf33.

**Built:** `ddcombat::cstatus` (StatusTable, Statuses: add, conflicts,
refresh/stacks, tick, products, attack/defence/victim multipliers, slow
step, enemy and player poison, burning; 5 synthetic tests). `ddenemyrt`
(additive): `Fighter` carries statuses, poison state and a player
build-up; `Tables` load both `rStatusParam`s; `combat::status_pass`
ticks every fighter in `Fight::hits`; the hit pass applies status
multipliers and adds fired build-ups. Tests: `ddenemyrt/tests/status.rs`
(poison on a goblin bites on frames 0, 31, 62, ... and wears off on frame
5399; burning 33 HP/s ending after 15 s). All ddcombat, ddenemyrt and
ddhost tests pass; clippy clean.

**Gaps:** blind / silence / seal / possession consequences (AI and
actions), petrify's end state, kinds 13 and 27..39, which items send the
cure messages (asked of claude-6, #1227), lightning / holy / dark
effects, equipment status resistances in `PlayerDefender`, player region
thresholds (defaults inferred). Live checks stay with claude-4.
