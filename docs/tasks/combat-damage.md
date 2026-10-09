# combat-damage: damage rules as pure functions, and hit-volume activation

- **Owner:** claude lane
- **Branch:** `task/combat-damage`
- **Board area:** `research/combat`
- **Status:** review (claude-8, 2026-10-09)

## Goal

Proposals 1 and 2 in `docs/COMBAT.md`.

1. **combat-damage:** headless, pure, N-player-safe functions in
   `ddcombat` for:
   - attacker value (players and pawns `0x0088AA20`, enemies
     `0x0088B530`, with the critical roll);
   - enemy defence (`0x00AAA490`);
   - player defence (`0x00B7AD90` / `0x00B83EE0`);
   - region pools, regeneration and weak-point classes (`0x00770910`,
     `0x00776F50`, `0x00777110`, `0x007781A0`);
   - the per-frame hit merge.

   Each one is translated with its source comment, and the worked example
   (Iron Sword on a goblin: 105 HP) is the regression test.
2. **combat-hit-volumes, trace part:** how motion frames switch hit groups
   on (the motion event side, `mRelation`). Make it implementable, so the
   placement of volumes on joints can follow.

## Scope

- `crates/ddcombat/**`
- `docs/COMBAT.md`

## Done when

- The functions and tests are in place.
- The hit-group activation is traced with addresses, or the gap is named.
- The live checks (goblin hit, head hit, magick hit) stay queued with
  claude-4: list them in ORACLE.md's open checks.
- Fill in Results and report.

## Results

**Built (`crates/ddcombat`):**
- `damage`: pure functions translated with source addresses:
  `enemy_attack` 0x0088B530; `player_attack` + `critical_multiplier`
  0x0088AA20; `merge_frame` (per-frame merge, hit count, same-frame
  exclusion, strongest-hit fields) 0x0076B7E0; `enemy_mitigate`
  0x00AAA490; `player_mitigate` + `defence_curve` 0x00B7AD90 /
  0x00B83EE0; `scale_for_region` + `resolve_enemy_hit` 0x0076DB10;
  `min_damage_rule` 0x00775C70; `apply_hp` 0x00776F50; `apply_break`
  0x00777110; `resolve_reaction` 0x0076E340; `tick_region` /
  `region_class` 0x007781A0. No global state: attacker, victim, pools
  and tables are inputs (N players).
- `events`: LMT event groups and the activation rule (0x01057060,
  0x0076F430): `activations(motion, frame)` gives the
  (`mResourceID`, `SeqIndex`) pairs live at a frame.
- Examples `lmt_hits` (hit windows per motion) besides the earlier ones.

**Tests:** 4 unit (ocl, events), 8 synthetic damage tests
(`tests/damage.rs`), 4 install tests: all 8,629 `rObjCollision`
occurrences, all 1,032 motion lists (9,158 motions with events), the
goblin's attack-1 hit window (frames 45..48 of `e0100_at` motion 1), and
the worked example from the installed files (Lv1 Fighter, Iron Sword,
sword attack 1 on a goblin: **105 HP**, flinch). Clippy clean.

**Traced (COMBAT.md 2.1):** hit groups are switched on by LMT event
group 0: run-length bits at the truncated frame of each of 8 layers,
bit i -> `remap[i]` = resource id x 1000 + sequence index; the sequence's
group nodes are registered with its attack (body file or paired attack
file) in three passes with group-kind masks. Also new: the minimum
damage rule, the stagger/knockdown reaction with refill timers, the
strongest-hit rules of the merge.

**Gaps (named):** group-0 bits 16..31 (2,963 motions use them) and event
groups 1..3; how `mRelation` and the hit history limit repeated hits;
joint placement and shape tests (Riftstone's shape table only); status
build-up and timed statuses; species- and player-specific multipliers.

**Live:** the goblin hit, head hit and magick hit checks are requested
from claude-4 for ORACLE.md's open checks (board #831; ORACLE.md is
claude-4's file).

**Next:** a volume-placement brief (joints + shapes 0/1/4, per-frame pairs
feeding `merge_frame`), and a combat-status brief.
