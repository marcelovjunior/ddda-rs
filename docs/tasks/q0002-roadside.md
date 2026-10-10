# q0002-roadside: Upon a Pawn's roadside encounter

- **Owner:** Codex lane, agent to be assigned by head
- **Branch:** `task/q0002-roadside`
- **Board area:** `story/q0002-roadside`
- **Status:** proposed; drafting authorized by head board 2874

## Goal

Make opening slice step 7 playable through the authored roadside goblin
encounter and its actual quest consequences. Reuse the existing goblin
combat, quest interpreter and canonical NPC ownership. First establish
the exact installed encounter's stage, group, actor identities and script
gates; the opening-slice description alone is not evidence for any of them.

This continues the open requirements of `upon-a-pawn`, whose latest batch
ends at 4ab42e6. It does not repeat the already implemented readers,
NPC flag callbacks, placement identities or synchronous fixture completion.

## Starting evidence and dependencies

- Read `docs/OPENING_SLICE.md`, `docs/tasks/upon-a-pawn.md` Results,
  `docs/QUESTS.md`, `docs/NPCS.md`, and relevant `docs/AI.md` sections.
- Existing goblin fight and q0002 FSM coverage lives in
  `crates/ddhost/tests/goblin.rs` and `crates/ddenemyrt`. Distinguish those
  isolated tests from a populated quest encounter running on the route.
- NPC placement owns allocation lifetimes. Checked asynchronous observations
  must also identify the current command, so a completed old Goto cannot
  satisfy a new command on the same still-live actor.
- Check Player Distance getter **0x007abe20** uses real actor/body position.
  Distinguish an unavailable observation from an observed absent body.
  Define co-op target selection explicitly; do not silently choose seat 0.
- Goto start/update **0x00573630 / 0x005736e0**, trace
  **0x0057b9a0 / 0x0057c760**, require authored arrival, collision/height,
  actor state and optional stop-motion completion. Fixed-speed interpolation
  or a timer cannot stand in for this behavior.
- Guard needs actual body eligibility, motion readiness, candidate reservation
  versus active owner, and lifetime-safe TalkEnd. Closing UI alone is not
  completion. See source handoff in the upon-a-pawn brief.
- `road-to-encampment` owns the opening demonstration's doorway and actual
  exit checks. Coordinate with it; an indoor SceNo1300 is not a passed route.
- `ambient-creatures` owns current population additions to host/spawns.
  Preserve canonical placement identities and coordinate shared hooks.

## Scope

Proposed for head's assignment: `crates/ddhost/src/story_npcs.rs`,
`crates/ddhost/tests/npc_placement.rs`, new
`crates/ddhost/tests/q0002_roadside.rs`, `crates/ddnpcrt/**`, and this
brief. Source findings belong in `docs/NPCS.md` and `docs/QUESTS.md` only
after coordinating their current owners. Minimal hooks in host.rs,
story_pawns.rs, ddstory or ddview/opening.rs require explicit file claims
and agreement with the active owner before editing. No wholesale replacement
of the generic quest, movement, population or encounter systems.

## Done when

1. Installed data identifies the encounter's admission, participants and
   start/finish gates, with source-backed callbacks and no copied bulk data.
2. A host-owned encounter admits the correct actors once, runs their real
   movement/combat/action observations, and retires them correctly. Repeated
   cell admission and stale owner/command observations have regressions.
3. Two Arisen seats can participate without duplicate quest completion,
   global-player assumptions or renderer-owned canonical actor state.
4. A bounded unattended opening run reaches and resolves the encounter by
   ordinary movement, interaction and combat. Tests must not manufacture
   enemy death, NPC Finish, quest bits or post-encounter state.
5. Quest/SceNo/GSF changes and a private capture are compared with the
   original at matching checkpoints. Missing original comparison remains
   explicitly unaccepted, even if implementation and headless tests pass.

Live checks follow current `docs/CODEX.md`: scripts/oracle/CE only, exclusive
live/game claim, private save backup before state changes, no save writes,
no install modifications and no computer-use/native UI automation.
Use the shared core/M5 targets by board coordination; never create a new
Bevy target. Head alone merges main; never push.

## Results

Planning only. Head authorized this draft in board 2874 after the prior
NPC batch left actor/movement/Guard integration open. Implementation and
original comparison have not started under this brief.

### S22 installed encounter map (2026-10-09, codex-56)

This research step is docs/tests only, on `step/s22-roadside-data`.
The installed data identifies the rescue actor as **Reynard, NPC 304**,
not the woman described in the proposed brief. `etc/item/npclist`
(`rNpcLedgerList`) names him; its loader/reflection are
`0x00ccac90`/`0x00cca670`. The ledger model is type 0/index 214.

#### Admission and identities

| Participant | Installed data | Gate in `ddhost::spawns` terms |
| --- | --- | --- |
| Three goblins | `scr/st100/etc/st100_e` group 23; `st100_64m53n_e23`, records 0, 1, 2, all `cSetInfoEnemy0100` | `Gate::Spawn` iff layout flag **100:7** is set; no SceNo/hour/second flag/other condition |
| Unused alternate goblins | Same GPL, group 24, same cell, three records in `st100_64m53n_e24` | **`Gate::Disabled`**, even with LOT7 set; latent hour window 20:00–05:00 does not override disabled |
| Reynard | NPC GPL `scr/st100/etc/st100_n` group 4, **global cell 00m00n**, `st100_00m00n_n04`, record 0, `mNpcId=304` | Layout flag **100:3** only, no scene/hour/other condition |

The common gate is `cLayoutSetCharaBase::vf12` **0x0075faa0**;
GPL decoding is **0x00cc6140/0x00cc4690**. Reynard physically occupies
cell 64m53n despite his global layout. His target is kind 5/group 4/index
0; the goblin targets are kind 2/group 23/indices 0–2. These are layout
record ids, not `mSetID` (which is -1). Record identity follows
**0x00cc1f60 → 0x007613d0 → 0x007643d0/0x004223f0**; NPC identity is
copied by **0x007a32f0**, record id by **0x007624d0**.

Reynard uses `mUse24Schedule=0` and authored placement action
`AI/FSM/Npc/st100/quest/q0002_n304`. Group 23 records have an **empty**
`mFsmFilePath`: the separately existing `q0002_goburin` FSM is not bound
by these records. Each requests first think table 1, equipment type 0,
random-set ignore, and an enabled attack multiplier 0.2; these values
must be read from the placements rather than hard-coded into a quest.

#### Authored quest sequence

1. GSF381 boots q0002. `etc/questCtrl/q0002_b00` has seven rows; row 0
   tests GSF381, starts log 0 and sets q2:95. `quest/q0002_b00` then
   clears 200:2 and sets **100:3 and 100:7**. Its state name mentions
   flag 4, but no flag-4-on order exists there. It sets GSF67/333 and
   waits to leave stage200 before camera149 and the encounter checks.
2. `scr/st100/etc/st100_fsm` supplies two task-family-4 scenario entries,
   **q0002_event1/event2**. Both are boot type3, detection mask1, check0,
   priority4, with no GSF prerequisite. They fire once on actual entry
   into their authored Area shapes. Event1 occupies the roadside encounter;
   event2 lies farther toward the Encampment. Detection/evaluation/callback
   sources: **0x00742810 / 0x00797df0 / 0x00798ce0**. The existing co-op
   detector treats mask1 as any Arisen entering; no seat-zero assumption is
   needed here.
3. Event1 guards q2:95/94/20/30 and q2:5 before starting. First entry
   requests camera12, sound30 and q2:25, waits for EndCamera, then sets
   **q2:5**. It polls the three group23 targets with EnemyCheck flag3
   (killed or present with valid maximum HP and HP<=0), plus Reynard's
   apparent/permanent death and live-intel/HP observations. Its messages
   at authored timer thresholds 20/40/55/70 seconds are reminders, not
   manufactured goblin deaths. The success/death branches set q2:20
   or q2:30 and clear the admission flags; the apparent-death branch
   still waits for the actual permanent-death or all-enemy-death facts.
4. Main quest states8/5 also check all three group23 deaths and the
   live NPC group4/index0. Successful rescue sets **q2:20**, then waits
   for exit bit10. QCT rows3/4 add **100 friendship to NPC304** when
   bit20 is set; row3 also updates the objective if bit5 was set.
5. Event2 sets **q2:10**, boots **q0003**, and waits for the pawn message
   display to become idle. This route can clear q2 without rescuing
   Reynard. Main q2 sets **bit94**; QCT row2 clears/rewards the quest.
   Main cleanup clears **100:3/4/7**, removes NPC group4 and enemy
   groups23/24, waits for StageNo!=100, clears q2:51 and raises
   **SceNo1450**. Rescue itself does not raise SceNo. QCT rows5/6 also
   clear LOT7 after leaving stage100 for either death bank of NPC304.

Callbacks already translated in `ddstory`: SetLayout **0x005c5080**,
QuestFlag **0x005c5530/0x005c55d0**, EnemyCheck
**0x005bef10/0x005bf3d0**, NpcCheck **0x005c0420/0x005c0820**,
NpcDieCheck **0x005c09f0**, group removal **0x005bdec0**, SceNo
**0x005bc960 → 0x0043d750**. QCT evaluation/judgments/results are
**0x00792360/0x0078ece0/0x0078fbb0**. These are source/data findings,
not acceptance of the full fight or quest route.

#### Runtime have/missing and next action

- **Have:** generic GPL/LOT admission can create all three group23
  goblins through `opening::field_enemies` → `ddhost::spawns::spawn_cells`.
  No additional quest-driven spawn is needed with LOT7 already set when
  cell64m53n is admitted. The stage-wide NPC adapter includes cell00m00n
  and can admit Reynard with LOT3. S23's ordinary two-player route
  (board3861, private `notes/steps/s23/route-600s.*`) reached stage100,
  Sce1410, spawned six field goblins and admitted NPC304 group4/record0.
  The other three goblins are ordinary population, not rescue targets.
- **Missing enemy ownership/state:** `field_enemies` does not call the
  existing `OpeningWorld::bind_spawned_goblin` for the returned owners,
  so actual HP cannot reach the quest's three placement checks. Generic
  `Fight::add_goblin` also ignores the record's first-table/equipment/stat
  options. New cells are marked seen before spawning and gates are not
  reevaluated for unchanged loaded cells. Unload/group removal, kill
  records and cleanup retirement are explicitly unimplemented there.
- **Missing Reynard behavior:** `q0002_n304` starts in state1 with
  Set Invincible(true), NoDamage(true), FixPos(true); the actual S23
  route stops this graph at **Set Invincible**. Registration
  **0x007a74f0** binds **0x007aa040**: Bool toggles **0x10000000** through
  a valid body's vf180, otherwise intel+0x5c0. Existing linked-body
  getter **0x00bb4730** returns that same canonical intel flag word.
  Implement this using the existing ActorFlag path, not a quest shortcut.
- Next graph dependencies include motion3226, **SetGaugeDisp**
  (**0x005342c0 → 0x00534bc0**; body gauge create/remove
  **0x00bb8c10/0x00bb8ca0**, otherwise intel+0x547), quest-bit polls,
  a real motion3089 completion, talk/bit88, six authored Goto steps,
  Set No Talk (**0x007aa670**) and fade completion. NoDamage/FixPos
  flag writers exist, but their body consumers are separate. NPC HP,
  damage and death-bank observations remain unavailable: intel presence
  alone intentionally does not satisfy NpcCheck/CheckHp. Rook's party
  record/body recruitment is implemented; his help/dialogue behavior and
  its encounter observations remain distinct work.
- NPC34's Set Invincible and NPC37's Attack-variable errors belong to
  Cassardis graphs; they were not the S23 stall's cause. S23 resolved
  NPC1 Guard/TalkEnd → GSF381. Do not use those unrelated logs to infer
  Reynard's completion or enemy deaths.

**Verification:** `crates/ddhost/tests/q0002_roadside_data.rs` is one
installed test covering day/night gates, disabled group24, exact enemy
record ids, global versus physical NPC cell, script binding and q2
admission/QCT identity. It also checks the installed Reynard name,
initial unsupported callback and actual `spawn_cells` combat admission.
Codex-1 independently ran the final expanded test: **1 passed**, 3.11s,
compile6.54s, exit0 (board3893); the earlier admission-only version passed
in4.74s. No runtime code, raw exports, game assets or original-game state
were changed.
Original comparison and full rescue/quest-clear acceptance remain open.
**Next action:** port Reynard's flag/gauge/motion dependencies and bind
the spawned goblin owners to their existing encounter targets; then run
the ordinary rescue route with real damage/death observations.
