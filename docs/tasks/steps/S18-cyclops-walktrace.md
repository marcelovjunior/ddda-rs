# S18: The cyclops approach (WalkTrace, two motion layers) and a test fight

- **Branch:** `step/s18-cyclops-walk` (after S17)
- **Files:** `crates/ddenemyrt/src/cyclops.rs`, the enemy body/motion
  code in ddenemyrt, tests
- **Kind:** one feature

## Context (S14 Result)

Approach action 85 is `cEm5000ActWalkTrace` (**0x00980440 / 0x00980620**)
with separate layer-1 start/loop requests and installed selector arrays
read through **0x00980760**. The turn helper **0x007975D0** schedules a
coordinate interpolation rather than turning instantly. Attack action 50
(`cEm5000ActAttack`, descriptor 0x100 -> bank 1 slot 0, 238 frames) is
already selectable.

## Do

1. Translate WalkTrace with its two motion layers (base locomotion plus the
   layer-1 requests), moving the cyclops along the navigation provider's
   path from S17.
2. Wire think driver + actions + body into a runnable cyclops actor in
   `ddenemyrt::fight` the way goblins are (spawn from the q0003 placement,
   HP/legs from S06, action enter from S05).
3. Test: the q0003 cyclops and one Arisen over several hundred frames,
   logging approach, attack and wait transitions (S14's original Done gate).

## Done when

That headless test passes and its Result lists the action transitions with
frame numbers.
