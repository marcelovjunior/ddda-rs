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

## Results

**2026-10-10, codex-63: source checkpoint; S31 remains open.**

- `OpeningWorld::enable_dragon_scene` disables the q0 camera/quest-bit
  injections and requests real event playback for rows19/20/21. It binds
  an explicit installed enemy target; production scene entry is pending.
- Story-phase combat observation supplies the actual opening dragon's
  first life pool and every combat Arisen except party pawn seats. The
  original `0x0051CCC0` reads the first pointer at actor+0x1D18, rather
  than searching for region ID0. `0x00770910` appends selected RST elements
  in resource order; the opening ctor selects kind0 at `0x00A49F2D`.
  Missing bodies/pools remain absent; unload and HP0 do not set a
  persistent placement-killed bit. Aggregating Arisen is explicit co-op
  policy; the native stage rule reads seat0.
- Installed q0 regressions exercise GSF379, leaving AreaStayFlag1 and the
  persistent-killed `EnemyCheck(flag1)` exits in both states16/5. The
  timers are strictly >17 then >43, and state5 starts its own counter.
  State5's setting bit0 checks transitions after callbacks and enters the
  destination on the next update. Event requests do not complete ev01.
- Installed LOT1 admission exposes seven group3 records704..710; all
  authored graphs are Demo actors with no Attack process. Admission is
  tested separately from materialized bodies and controller playback.
- Head5475/5491 approved an independent NPC Demo adapter and a narrow
  `ddai::Core::new_with_cluster_work` API. Native ClusterWork is a list
  of once-only from-all node IDs, not copied owner variables:
  `0x00E087E0 -> 0x010C9310(8)`, `0x00E059B0/0x00E05A00`,
  `0x00E092A0`. Its key is `mOwnerNodeUniqueId` (native cluster+8;
  reflector `0x010CB1F0`), not the repeated `mId`. The opt-in preserves
  these lists across subcluster reentry. Default hierarchical rejection
  remains for callers that have not opted in.

**Tested:** prescribed shared core target: `ddhost --test dragon_route`
6passed (compile15.51s, tests13.85s); `ddai --lib` 18passed (cached0.36s,
tests0.00s). The new core test also passed separately (compile2.51s).
Actual command exit0; no skipped install tests on this machine. The
combat snapshot regression sets HP scalars explicitly to check boundary
and observation behavior; it is not a normal-input hit or route replay.
No game files, resource dumps or decompiler output are committed.

**Open:** production scene entry and event player continuation to q1;
native Demo Wait/Motion/GotoEx/SetDamage controller/body integration;
actual S30 damage/FSM integration; persistent dragon-death lifecycle;
normal-input route capture and original comparison. Existing simple/
unsupported camera adapters remain explicitly labelled stand-ins.

**Next:** source/port the beach Demo callbacks using native actor
observations and real motion clocks, then enter ev00 after editor
confirmation instead of fast-forwarding through the fight.
