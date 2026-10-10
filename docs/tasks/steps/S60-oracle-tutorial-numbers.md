# S60: The tutorial stretch against the running original (oracle numbers)

- **Branch:** `step/s60-oracle-numbers`
- **Files:** `research/ddda-oracle/recipes.py` (new read-only recipes),
  `docs/RECORDINGS.md` (results), regression fixtures that pin the
  original's numbers in `crates/*/tests`; no runtime change unless a
  number disagrees (then a separate fix with its own brief)
- **Kind:** comparison (the 1:1 check our source-only work cannot give)

## Context

The opening route (S50, S46 checkpoints) now plays to q0003 from source
translations, but no combat or quest number of the rescue has been read
from the running game. docs/ORACLE.md has the probe, its protocol and the
safety rules (back up the save first, never write it, short in-cell
teleports only via `recipes.safe_teleport`, Esc pause is the safe state,
one driver via the `live/game` claim, no UI automation).

## Do

Read-only, on the running original, at the matching point of a save the
maintainer already has (or a fresh new game if one exists; never write a
save):

1. The rescue goblins: HP max per placement (the 2.0 rate), attack rate,
   exp award, their equipment parts (S47's RNG), perception radii (S49).
2. Reynard: HP 2500, the gauge display and veto (S54), the camera-12
   close-up timing (S40).
3. The Arisen at level 1/2: HP, stamina, damage of the first sword hit on a
   rescue goblin (S45's 260x100/180 figure), level-up stats (S43).
4. Rook: HP/stamina and his first chatter line ids in the fight (S52).

Record each value with the memory path read, and compare with our host's
value from the matching route-v2 checkpoint (headless). Agreements become
regression fixtures (the original's value, its read path, and the date).
Disagreements are listed with the suspected source, never patched by
copying the number.

## Done when

RECORDINGS.md has a table of every value above (original vs ours, the
read path), agreements are pinned by tests, and each disagreement has a
follow-up brief or a traced explanation.
