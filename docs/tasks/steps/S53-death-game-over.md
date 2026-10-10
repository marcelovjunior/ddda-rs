# S53: When the Arisen falls: dying, revival, game over in the opening

- **Branch:** `step/s53-death-game-over`
- **Files:** `crates/ddenemyrt` / `crates/ddhost` (death state, pawn revive),
  `crates/ddview` (game-over GUI and retry), tests
- **Kind:** research + one feature

## Context

The party can now die in the opening (goblins hit hard, S49). The original
has a dying state (pawns can revive a downed Arisen? check: in DD the Arisen
dies at 0 HP while pawns can be revived), a game-over screen, and "retry /
load last save" choices. Today an Arisen at 0 HP just stays down. Co-op adds
a design question (one Arisen down while another plays): follow COOP_DESIGN
or record a decision.

## Do

1. Trace the original: Arisen 0 HP -> death action/motion, pawn downed state
   and revival by allies, the game-over GUI (class/layout) and its choices
   (load last save, retry from checkpoint...) and what each restores.
2. Implement it for the opening: death motions, pawn revival, the game-over
   screen, and "retry" via the host save/checkpoint (S46/S48) the way the
   original reloads.
3. Co-op: decide (COOP_DESIGN) what happens when one of two Arisen falls.
4. Tests: an Arisen at 0 HP reaches game over; a downed pawn is revived by
   another member; retry restores the last save.

## Done when

Dying in the opening behaves like the original (death, revival, game-over
GUI, retry), with sources and the co-op decision documented.
