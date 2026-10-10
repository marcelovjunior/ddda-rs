# S39: NPC quest talk (Reynard's thanks, reward and departure line)

- **Branch:** `step/s39-npc-quest-talk`
- **Files:** `crates/ddnpcrt` (a new NPC QCT selector/effects module),
  `crates/ddhost/src/npcs.rs`, minimal `crates/ddview/src/opening.rs`
  interaction; tests
- **Kind:** research + one feature
- **After:** S32 (Reynard's authored motions and owner retention)

## Context (codex-54, board #4509 / #4520)

After the rescue (q2:20), talking to Reynard (NPC304) is driven by the
installed `etc/questCtrl/n304QuestCtrl`: table 0 QuestTbl row 2 tests
q2:95 && !q2:94 -> Continue(table 3, quest 2) + Repeatable; table 3 row 0
judgment [1,2,20,0,0] (q2:20) -> results conversation 12, reward item 1058
(default count 1), set q2:88 (once); row 1 after q2:88 offers conversation
13 (repeatable departure line). Talk init 0x00B9DC20 checks the actor's
NPC control (+0x713C) -> 0x00792130 -> 0x00792360 (row selection / once /
priority); QCT result dispatch 0x0078FBB0.

## Do

1. Port the NPC QCT selection (0x00792130 / 0x00792360) and the result
   effects this table uses (conversation, item reward, quest bit), with the
   once/repeatable rules.
2. Manual talk (the ordinary interaction) to the retained NPC304 evaluates
   it for the interacting seat: exclusive dialogue ownership as in S23, the
   reward into that seat's bag (S28's bags), q2:88 set once.
3. Tests: row 0 runs once (conversation 12, item 1058, q2:88), a second
   talk gives conversation 13; another seat cannot steal the dialogue; no
   timer or bypass sets anything.

## Done when

An installed host test and the opening route show Reynard's thanks with
the reward after the rescue and his repeatable line afterwards, all from
the installed table; the Result lists the sources.

## Result (source checkpoint; viewer and persistence acceptance pending)

The installed `n304QuestCtrl` and quest message resources are read at runtime
by `ddnpcrt::InstallData`. The selector keeps the NPC's permanent row masks
and transient firing marks; it tries the active-quest pass before the ordinary
pass (`0x00792130`, `0x00792360`). Continue's quest comparison is field **b**
at result+0xC (`0x0079243B`), not field c. Type7's clear-else-disable judgment
and type22's repeatable result retain their distinct effects. An unavailable
NPC talk counter (+0x8E8+npc*0x110, judgment38) cannot make a candidate true;
a known false judgment still proves its conjunction false. No affinity value
is substituted for that separate counter.

The phase distinction is sourced to `0x0078FBB0`: phase1 starts the installed
quest dialogue and defers item/quest-bit effects. Stock talk vf06 state3
(`0x00B9A430`, call `0x00B9AC72`) checks real message status through
`0x00B9C720` / `0x0044D640`, then `0x00BA02E0` applies the selected row in
phase2. Reading the actual final host text line is the supported equivalent
here; elapsed frames never produce phase2. Item count zero means one, and
inventory addition uses the existing source port of `0x0045E680` with the
installed item list. `0x007925C0` / `0x00CD7990` clear transient marks on exit;
they are not the reward dispatch. Unsupported judgments/results and quest
dialogue choice routing fail explicitly.

`Host::talk_story_npc` requires a current materialized `NpcLifetime` plus an
explicit actual-body readiness observation from its adapter. It reuses the
stock dialogue slot and actor cleanup from S23, so another seat cannot steal
the same NPC or an active global conversation. SetNoTalk is respected from
`0x007AA670`. A real unload aborts selected text without rewarding a
replacement owner. The runtime retains permanent masks across body unload.

Five focused tests actually passed: three NPC selector/runtime regressions
and two installed host regressions. They verify conversation12, no reward or
q2:88 before the last read line, one item1058 into **seat1's** host bag, q2:88
once, subsequent conversation13 for either seat without another item,
cross-seat/same-seat ownership, time remaining pending, unload cancellation
and stale lifetime rejection. Quest-start q2:95 and rescued q2:20 are explicitly
labelled fixture observations in these isolated tests; they are not route
claims. The complete source-compiled core suites passed **69 host + 31 NPC**
tests, none ignored. After adding the unsupported-choice guard, the final
five focused regressions passed again (2.86 s compile, 6.52 s host tests).

Head approved the per-seat opening adapter and optional save chunk in board
#4751. The host now writes optional shared chunk `NPCQ` version1 with only
permanent NPC/table/row masks. Its original persistence is confirmed by
`uNpc::vf32` (`0x00BACE50`), which binds control+0xC to
`sNpcManager+0x878+npc*0x110`. Tables are reloaded from the install; no asset
data or transient conversations enter the chunk. Absence means empty masks,
including load into an existing runtime. Invalid versions, truncated records
and rows outside the installed table are rejected before replacing mask memory.
The saved-state hash includes those masks. Four installed talk/save regressions
actually passed (13.76 s, 7.43 s compile), including reconstruction without a
second reward and old-save absence; the existing seven replay and two profile
save/session tests also passed. No hardcoded q2:88 skip is used.

The remaining work is viewer validation and an ordinary route. The opening
adapter is written but not yet source-compiled in this checkpoint. Full route
acceptance still depends on head's normal
combat driver and NPC combat body work; S32's three actual ordinary attempts
did not reach its complete motion chain. No original-game comparison is
claimed by this source checkpoint.
