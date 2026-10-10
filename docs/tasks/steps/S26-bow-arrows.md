# S26: The bow, part 2: arrows that hit

- **Branch:** `step/s26-bow-arrows` (after S25)
- **Files:** `crates/ddsim` (the shot), `crates/ddenemyrt` (arrow shells
  like the staff's, `crate::shell`), tests
- **Kind:** one feature

## Context

vf06 (0x00B17940) fires on release paths through 0x00B1A730 (a shell
spawn when the parameter record's +0x120 is 0) or a model-driven variant;
0x00B19EE0 / 0x00B1AB70 / 0x00B80650 follow (recoil motion, state reset,
stamina?). The staff's WandShot shells (`ddenemyrt::shell`, `Fight::shells`)
are the existing pattern for projectiles.

## Do

Trace the arrow: its shell parameters (speed, gravity, range, damage
source), how many per shot and the draw-time effect (a charged shot?), the
stamina cost, and the aim direction (camera vs facing). Implement the shot
in ddsim (timing) and the arrow shells in the fight so they travel and hit.

## Done when

A fight test: a Strider aims and releases at a goblin 10 m away and the
arrow hits it (damage from the traced formula), plus a miss test.
