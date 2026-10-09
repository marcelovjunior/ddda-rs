# m4-goblin-fight: a headless goblin fight in the host

- **Owner:** claude-1
- **Branch:** `task/m4-goblin-fight`
- **Board area:** `fight/integration`
- **Status:** open

## Goal

The first M4 vertical slice, headless: one goblin against one Arisen in
`ddhost`.

**Enemy side:**
- an enemy unit in `ddsim`, or a new enemy module beside it;
- `ddai` FSM and think table → action requests → enemy actions with their
  motions (bank `e0100_at`, root motion) → movement on `ddworld`
  collision.

**Posed skeletons, headless:** sample `mtlmt` and compose joint world
matrices, so `ddcombat::volumes` places the attack and hurt volumes
(`docs/COMBAT.md`).

**Hits:** `resolve_frame` → `merge_frame` → damage → HP, stagger and
knockdown pools → death.

**Arisen side:** start with a scripted light attack (the Fighter's sword
attack 1, `collision\pl\m0001_00_lv1`), so the worked example (105 per hit)
plays out. Mage staff attacks if time allows.

**Order:** the frame order from `docs/FRAME.md`: hits from the previous
frame's volumes, before movement.

## Scope

- `crates/ddsim/**`, or a new `crates/ddenemyrt/**`
- `crates/ddhost/**`: coordinate with claude-10 (m3-lan), who also
  edits ddhost
- `crates/ddcombat/**`, `crates/ddai/**`: API additions only
- `crates/mtlmt/**`: skeleton posing
- `docs/COMBAT.md`, `docs/AI.md`

## Done when

- A ddhost test runs a scripted fight. The goblin approaches and attacks
  (its hit lands on the expected frames when the Arisen stands in range),
  the Arisen's attacks deal 105 each, and the goblin dies after the
  expected number of hits.
- It is deterministic (the replay hash).
- Fill in Results and report.

## Results

(filled in by the agent)
