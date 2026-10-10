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
