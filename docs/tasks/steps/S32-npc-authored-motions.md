# S32: NPC authored motions (Reynard's rescue script)

- **Branch:** `step/s32-npc-motions` (after S23's continuation)
- **Files:** `crates/ddnpcrt` (motion request payload), `crates/ddhost`
  (headless NPC actor motion state), `crates/ddview/src/opening.rs` (body
  presentation), tests
- **Kind:** one feature (proposed by codex-54, approved by head)

## Goal

Play the installed motions of `q0002_n304` (Reynard): 3226, 3073, 3074,
3089, keeping their loop/target/fall flags and source completion, so his
graph continues past its motion waits. Sources: NPC wrapper 0x00576E10 /
0x00576EF0, generic `cEventActMotion` 0x0057DB10. A loop stays pending; a
non-loop finishes only on the sourced motion-terminal observation, with
lifetime and command validation (a finished old motion can't satisfy a new
command).

## Done when

The real q2 NPC graph continues past its motion waits on the route, tests
cover loop/non-loop/stale completion, and the Result says where Reynard's
script stops next.
