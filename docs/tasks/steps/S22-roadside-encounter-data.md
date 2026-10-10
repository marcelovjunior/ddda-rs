# S22: Identify the roadside goblin encounter (q0002) in the data

- **Branch:** `step/s22-roadside-data` (docs and a test only)
- **Files:** `docs/tasks/q0002-roadside.md` (Results), one installed test
  in `crates/ddhost/tests/`
- **Kind:** research, one question

## Question

In the original, just outside Cassardis the Arisen meets goblins harassing
a woman, and the first pawn (Rook) helps (q0002 "Upon a Pawn"). Which data
makes that happen?

- the stage, cell(s) and enemy group(s) (GPL rows) of those goblins, and
  their gates (SceNo, hour, layout/quest flags) in `ddhost::spawns` terms;
- the woman: NPC id, layout record, her script (`AI\FSM\Npc\...\q0002_*`);
- the quest script steps (q0002 QCT/FSM) that start the scene, check the
  goblins' deaths and finish it (the flags and SceNo changes);
- what still blocks it in our runtime (the NPC variables/processes the logs
  report as unimplemented, e.g. "NPC 34 ... Set Invincible").

## How

Read `docs/tasks/q0002-roadside.md` and `docs/tasks/upon-a-pawn.md`
Results first. Use the installed data readers (ddspawn, ddquest,
ddnpcrt), the Ghidra server for the callbacks, and the existing tests
(`crates/ddhost/tests/goblin.rs`, `spawns.rs`). Head just made the
opening spawn every loaded field cell's gated enemy groups into its fight
(`opening::field_enemies`); say whether the roadside goblins come through
that path with the q0002 flags set, or need a quest-driven spawn.

## Done when

The q0002-roadside Results list the encounter's data with sources, and an
installed test asserts the goblin group(s) and the woman's record for the
q0002 state. No runtime changes.
