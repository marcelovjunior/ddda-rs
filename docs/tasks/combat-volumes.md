# combat-volumes: hit volumes on joints, shape tests, hit history

- **Owner:** claude-8
- **Branch:** `task/combat-volumes`
- **Board area:** `research/combat`
- **Status:** review (claude-8, 2026-10-09)

## Goal

Complete hit detection (`docs/COMBAT.md` §2):
- place `rObjCollision` attack and hurt volumes on their joints each frame
  (offsets, the joint attach rule);
- the exact shape-vs-shape tests the original uses: spheres, capsules,
  cylinders, OBBs, whatever `rObjCollision` shapes need;
- the hit history / `mRelation` rule that limits repeated hits on one
  target;
- event bits 16-31 and event groups 1-3, if they bear on hits.

Feed the per-frame pairs into `merge_frame`.

**Goal test:** the goblin's first attack against a standing Arisen
produces exactly one hit, in frames 45-48, at the traced distance limits.
Then **combat-status** (proposal 3) if time allows: status types to
`rStatusParam`, durations and build-up.

## Scope

- `crates/ddcombat/**`
- `docs/COMBAT.md`

## Done when

- Volume placement and the shape tests are translated with addresses and
  tests.
- A synthetic goblin-vs-Arisen frame sequence yields the expected hit
  frames.
- The live checks stay listed with claude-4.
- Fill in Results and report.

## Results

**Traced (COMBAT.md 2.1-2.3):** node points on joints (0x00CCDE30 /
0x00CCE190: offset times the normalized joint matrix; virtual joints
-1, -3..-6), per-frame volumes (0x00773830: owner-scaled radius, shapes
0/1/4), the exact tests (sphere-sphere 0x00D13600, capsule-sphere
0x00D139A0, capsule-capsule 0x00D154E0: squared core distance <= squared
radius sum, no sweep), the pair callback 0x0076D730 (own-hit, no-hit
kinds, kind table 0x0150A5E8, friendly factors), ignore zones 0x00779B10,
same-frame dedup 0x00779860 (lower `mHitPrio` wins) and the hit history
(0x0076EAF0 / 0x0076EB60 / 0x0076ECA0 / 0x0076EF10: one hit per
continuous contact, `mMultiHit` re-hits after `mResetTime`, a shared
non-zero `mRelation` counts as one swing). Event bits 16..31 and groups
1..3 are read by action timing (`cMotionSeqCount` and others), not by
hit placement.

**Built:** `ddcombat::volumes` (placement, tests, filter, dedup,
history, `resolve_frame` feeding `enemy_attack`/`player_attack` and
`merge_frame`).

**Tests:** 5 synthetic (`tests/volumes.rs`: the goal sequence, shape
tests with touching cases, shape-4 pull-in, history timers and relation,
kind filter) and the install goal test: the goblin's first attack against
a standing Arisen hits exactly once, on frame 45, out to 133.5 between
roots (rigid poses), and not at 133.6. All earlier tests still pass;
clippy clean.

**Gaps:** friendly-hit team matrix ids and the factor's consumer, the
wall check 0x00779270, attacker flag +0x1EC1, joint -2's angle, posed
skeletons (an input here). combat-status not started in this brief.

**Live checks:** goblin hit, head hit and magick hit stay in ORACLE.md's
open checks with claude-4 (#831, #835).
