# S31: ev00 -> dragon fight -> ev01 -> ev05 in the opening

- **Branch:** `step/s31-dragon-route` (after S30)
- **Files:** `crates/ddhost` (opening route, replacing the ScriptedHost
  shortcuts of this scene), `crates/ddview/src/opening.rs`, tests
- **Kind:** one feature

## Context (S24 map, "Fight end, event prerequisites")

After ev00, q0000 raises SceNo 1025 (start position 3), runs camera 17
with GSF520, reaches state 16. State 16's condition 20 starts ev01 on
**GSF379 OR EnemyCheck (the dragon's killed-state flag) OR NOT
AreaStayFlag[1]**; otherwise after 17 s it goes to state 5, which sets
GSF378 and exits after 43 s more. `aStage200::vf12` 0x0051CCC0: while
GSF378 is on, it records the dragon's first-life-region HP and requests
GSF379 when HP < baseline - 4 (strictly). Seven NPC victims (group 3,
LOT 1) take part; no soldiers.

## Do

Run the real sequence in the opening: ev00, then the playable fight with
the S29/S30 dragon and the seven NPC victims, the state 16/5 exits as
authored, then ev01 and ev05, then Cassardis as today. Remove the scripted
stand-ins for this part only.

## Done when

An `--opening` run plays ev00 -> fight -> ev01 -> ev05 by the authored
exits (hit the dragon for GSF379, or wait for the timeouts), a test covers
each exit, and the route continues to q0001 as before.
