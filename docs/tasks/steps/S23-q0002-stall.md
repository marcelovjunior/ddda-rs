# S23: The opening stalls at SceNo 1400 after ev15 (TOP PRIORITY)

- **Branch:** `step/s23-q0002-stall`
- **Files:** most likely `crates/ddnpcrt` (NPC variables/processes),
  `crates/ddhost/src/story_npcs.rs`, tests
- **Kind:** diagnose and fix the tutorial's current blocker

## The blocker

The ordinary opening route (`ddview <nativePC> --perf-route`, 600 s
allowed) plays ev15 (row 23) and then stays at SceNo 1400 at about
(-825, -66, 236), the q0002 step, and never reaches the Cassardis gate or
the field (codex-1, S13 Result follow-up; log
`E:/ddda-re/notes/steps/s13/route-600s.err.txt`). Head's own run shows the
same: the demo's `Q0002Event` step targets (-879, -8, 250) and nothing
advances.

Leads from the logs (observations, not proven causes):
- NPC 37: unresolved Attack flag after ev15.
- NPC 34: process "Set Invincible" not translated
  (`AI\FSM\Npc\st200\quest\q0002_n34`).
- NPC 3 / NPC 8: NPC variable `cAIActionFS...` not translated
  (`q0002_n3`, `q0052_n008`).
- NPCs awaiting headless movement (Goto) to their targets.

## Do

1. Find what the original needs to leave SceNo 1400 (q0002's script: the
   condition that moves it on, ddquest/ddstory), and which of the NPCs
   above it waits for.
2. Translate the missing NPC variable(s)/process(es) on that path from the
   exe (no shortcut that sets the flag or SceNo directly).
3. Re-run the route and report how far it now gets.

## Done when

The route passes SceNo 1400 by the authored mechanism, tests cover the
translated callbacks, and the Result names what was missing and where the
route stops next.
