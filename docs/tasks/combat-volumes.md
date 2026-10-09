# combat-volumes: hit volumes on joints, shape tests, hit history

- **Owner:** claude-8
- **Branch:** `task/combat-volumes`
- **Board area:** `research/combat`
- **Status:** open

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

(filled in by the agent)
