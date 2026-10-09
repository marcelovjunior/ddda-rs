# event-polish: cameras follow actors, real NPC actors, the script gate

- **Owner:** claude-7
- **Branch:** `task/event-polish`
- **Board area:** `story/events`
- **Status:** review

## Goal

Make `ddview --event 22` look like the original's cutscene:
- **Camera:** apply the camera's actor-joint adjustments and `mOffsetPos`.
  Cut 1 currently frames a torso.
- **Actors:** real NPC models and looks for the event's actors, not
  stand-ins; place them correctly (the player lies in bed).
- **Motion slots:** fix the `npc001_00` / `npc002_00` "slot 1 is empty or
  missing" errors.
- **Script gate:** add the gate `0x0044D430` to `ddstory` (scripts stop
  during stop flags, talks and stages 0x321-0x324).

## Scope

- `crates/ddstory/**`, `crates/ddquest/**`
- `crates/ddview/src/event.rs`
- `crates/mtcamera/**`
- `docs/EVENTS.md`

## Done when

- Screenshots at the cut frames frame the right subjects, with NPC models.
- The gate has a test.
- Fill in Results and report.

## Results

**Camera.**
- `CameraView::adjusted` applies the actor-joint adjustments (0x007ecc30,
  0x007ed220, 0x007ebba0) to host-supplied joint displacements, with a
  unit test.
- The displacement is a joint's world position minus its local-chain
  position, so it only compensates for IK and body morphs.
- `ddview` passes zero, because it has neither.
- The cut 1 torso shot is the authored framing: the player stands in
  cuts 0, 1 and 7 (data: `pl_00` hips 108 cm above the root).
- `mOffsetPos` has no reader in the per-frame camera path (five routines
  checked; listed in EVENTS.md), so it is reported, not applied.

**NPC actors.** `ddstory::look::npc_look` follows `uActorModelPl::vf05`:
- the ledger stage, then rEditPawn;
- the set variants;
- the base body by sex;
- the rArmorTableNpc -> rArmorModelNpc outfit;
- hair and beard from h_parts.

There are readers for all five formats, and test `st200_ev10_npc_looks`
covers them:
- NPC 1 is male: `e_npc004`, `hair104`, `beard104`;
- NPC 2 is female: `e_npc005`, `hair105`.

`ddview` spawns every model of a look on the actor's motion.

**Motion slots.** Event lists only hold the cuts an actor is drawn in.
The viewer starts from the first present slot and keeps the last motion
over empty slots. The `npc001_00` / `npc002_00` errors are gone (smoke
run at frame 470: no errors).

**Script gate.**
- `ddstory::story::script_gate` (0x0044d430) is wired into the stage and
  quest scripts and the QCT. The flags arrive as `HostEvent::GameFlags`.
- Test `script_gate_stops_scripts` covers:
  - every flag;
  - stages 801-804;
  - with the gate shut for 120 frames, no script state or world change;
  - scripts resuming once the gate opens.
- The opening tests still pass.

**Screenshots** of the cut frames are described in EVENTS.md (frames
200, 460, 670, 910, 1270).

**Not done:**
- part on/off masks;
- body and face morphs, colours;
- lighting;
- a shared skeleton for a look's models;
- IK;
- the meaning of the gate's flag bits.

**Commits:** `task/event-polish` c223bda (ddstory) and the viewer/docs
commit.
