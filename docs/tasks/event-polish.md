# event-polish: cameras follow actors, real NPC actors, the script gate

- **Owner:** claude-7
- **Branch:** `task/event-polish`
- **Board area:** `story/events`
- **Status:** open

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

(filled in by the agent)
