# S49: Enemy perception: the sensor pick and hate (who notices whom, from how far)

- **Branch:** `step/s49-enemy-sensor`
- **Files:** `crates/ddenemyrt/src/goblin.rs` (target choice), the existing
  `npc_sensor.rs` work, sensor resources/readers; tests (headless)
- **Kind:** research + one feature; headless acceptance
- **Priority:** TOP: it decides whether the q0002 rescue can succeed

## Evidence (head integration route, main c7c0505+, 2026-10-10)

Per-attacker damage on Reynard (owner 106) during the rescue: rescue goblins
100-102 (attack rate 0.2): 34 hits, ~1270; **ordinary goblin 107** (group 250,
default rate 1.3): **6 hits, 1448**. Goblin 107 started ~40 m away targeting
the Arisen, ran in (RunTrace) and switched to Reynard; he died (q2:30).
Neither the Arisen nor Rook hit him.

Today `Goblin::step` picks "the nearest live target at any distance" (the
sensor pick 0x00AB52E0 is *not ported*); S34 added NPC candidates with a
distance-only hate (0x00AB5A50 / 0x00AB52E0, codex-61). With no perception
range, every goblin on the field converges on the fight.

## Do

1. Port the enemy sensor: the em0100 sensor resource (sensor group /
   radius / FOV / LOS per state; the placement's `センサー半径倍率を使用する`
   / `センサー半径倍率` multiplier), what makes a goblin notice a player, a
   pawn or an NPC (proxy subtype masks: em0100 target mask 0x16 incl. NPC 4),
   alert/forget timing, and the hate weighting that picks among noticed
   targets (0x00AB5A50 influence, 0x00AB52E0 pick).
2. Replace the "nearest at any distance" choice with it, for players, pawns
   and NPC bodies alike; an unnoticed target is not chosen.
3. Headless installed tests: a goblin 40 m away (group 250 placement) does
   not notice the Arisen/Reynard; one inside its sensor does; the q0002
   rescue goblins target Reynard as authored.

## Done when

Goblins only engage what their sensors notice, with sources documented, and
the installed rescue test shows the group-250 goblins staying out unless the
party comes near.
