# S24: Map the beach dragon fight (research)

- **Branch:** `step/s24-dragon-map` (docs and tests only)
- **Files:** `docs/tasks/dragon-attack.md` (Results), one installed test
- **Kind:** research, one question

## Why

The dragon attack is required for the current goal (maintainer,
2026-10-10). OPENING_SLICE step 5: stage 200, st200 ev00 "It's the
dragon!", the scripted fight "Challenge the Dragon", ev01 (heart taken),
ev05. The dragon runtime has its think/requests, flight substeps, pass-C
boundaries and nine tests (dragon-attack Results), but the fight itself is
not playable, and the opening still uses ScriptedHost shortcuts here.

## Question

What exactly does the original run between ev00 and ev01?
- Which dragon actions are reachable in this encounter (its think table
  in this mode, the action ids, their classes and motions), and which of
  them the runtime already has.
- What the player can do and what ends the fight (a timer, a scripted
  hit, HP, a flag), which flags/SceNo changes it makes, and what ev01 /
  ev05 need to start.
- Which other actors take part (soldiers? NPC fighters?), and where the
  dragon spawns and moves.

## Done when

The dragon-attack Results hold an ordered list of the actions and systems
the playable fight needs (each with its address, and "have" / "missing"),
the end condition with its source, and an installed test asserting the
reachable action set. Head turns that list into S25+.
