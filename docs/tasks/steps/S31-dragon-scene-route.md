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

**Demo command checkpoint:** head5475/5579 approved the independent
`ddnpcrt::demo` controller and the parser-only `Motion::from_demo_record`
hook. Installed beach graphs now execute body readiness, authored quest
flags, actor flag masks and typed Wait/Motion/GotoEx/Damage requests.
The controller keeps the native ClusterWork lifetime and stops on
Finish/Kill rather than recreating its Core. Source callbacks are
`0x00533DC0/0x00533A90`, `0x0056CFF0/0x0056F160`,
`0x0056DCC0/0x0056E230` and `0x0056EBB0/0x0056EDA0`.
Command tokens reject stale completions; Wait remains running. GotoEx
retains the installed trace record. Damage requests a native reaction,
not an HP subtraction. Actual actor movement, reactions, interface
results and the body-manager retry byte remain unintegrated.

Installed readiness writes are q0 bits10..14, including NPC708's bit14
(the earlier S24 prose omitted it). Values are read from each graph,
without a production NPC-to-bit table. Three installed Demo tests passed
(compile0.82s, tests3.00s), all eleven NPC unit tests passed
(compile1.25s, tests0.00s), and the four existing installed applied-action
tests passed (compile0.77s, tests3.25s), all actual exit0 on the prescribed
shared target. Interface completion in the command-token test is an
explicit scalar fixture; no native motion playback or full route replay
is claimed.

**Host composition checkpoint:** applied graphs with authored
`SetInterfaceDemo` now use the independent Demo owner. Class-only
selection initially broke Reynard because its graph also has a Demo
owner but uses the inherited NPC interfaces; an installed regression
now preserves that existing adapter, including Set Fix Pos and motion3226.
There is no fallback after a runtime error. Readiness comes from the
current lifetime's actual bound body and resolved motion banks, rather
than intel presence. Quest/GSF/LOT writes reach the real story World in
stable owner order; intra-step read-after-write remains a snapshot
limitation. Flags update the canonical intel/body storage. Generic
motions use the existing launch/lifetime checks and real actor clock.
Replacement destroys the previous layer. GotoEx and Damage remain
typed pending requests without a fake Wait, HP change or Done;
Damage(flag) remains false until a real reaction action is ported.

Final shared-target regression command exited0: compile8.77s; installed
NPC combat2passed (9.64s), Demo2passed (5.82s), motion6passed (6.47s),
placements9passed (2.45s). The Demo body test separately passed before
the compatibility fix (compile13.33s, tests6.72s). It withdraws NPC708's
body and proves q0 bit14 stays false, then validates real lifetime
retirement and rejection of stale body bindings. No full-route or
original comparison was performed. The core lease was released after
the command's actual exit.

**Open:** production scene entry and event player continuation to q1;
native Demo Wait/Motion/GotoEx/SetDamage controller/body integration;
actual S30 damage/FSM integration; persistent dragon-death lifecycle;
normal-input route capture and original comparison. Existing simple/
unsupported camera adapters remain explicitly labelled stand-ins.

**Next:** source/port the beach Demo callbacks using native actor
observations and real motion clocks, then enter ev00 after editor
confirmation instead of fast-forwarding through the fight.

**Guard integration regression (head5837):** commit `dae5292` selected the
new Demo adapter from the process container alone. Installed
`ai\fsm\npc\st200\quest\q0002_n001_ai` uses `SetInterfaceDemo` for
`cAIActionInterfaceDemoSetPos`, which the existing NPC adapter already
handles before inherited NPC Guard dialogue. The new adapter rejected that
placement interface and quarantined the graph; body readiness was not the
cause. Selection now reads the authored interface payload, preserving
DemoSetPos and inherited NPC callbacks on the existing adapter while beach
Demo commands still use the dedicated owner. Native dispatch
`0x00533A90 -> 0x0056C940` uses the interface object's type. Unsupported
interfaces remain explicit errors; there is no runtime fallback.

The fix is isolated on `step/s31-demo-regression` from committed main
`861c94e`. Full `cargo test -p ddhost` on the prescribed shared core target
actually exited0: 9 unit and 84 integration tests passed across all29
integration binaries (compile20.16s, no failures). Both reported failures
now pass: Guard dialogue/quest boot and applied NPC observation/restoration.
Existing beach body-readiness, Reynard, unsupported-error, NPC combat,
motion, opening and co-op regressions also passed. This is installed-data
host acceptance, not an original-game comparison or full playable S31 route.
