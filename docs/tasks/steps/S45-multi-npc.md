# S45: MultiNpc placements (uMultiNpc) on the road (route blocker)

- **Branch:** `step/s45-multi-npc`
- **Files:** `crates/ddnpcrt` / `crates/ddhost` (npc_placements.rs, story
  NPC admission), `crates/ddview/src/opening.rs` (materialization), tests
- **Kind:** research + one feature
- **Priority:** blocks the opening route past the rescue

## Context

The full route (head, 2026-10-10, main edc7ce2) now plays through the
rescue, Reynard's reward and his repeat line, then walks the road toward
q0002_event2. Entering the next cells (around 62m52n / 64m51n) admits a
placement whose name family is MultiNpc, and the opening stops on purpose:
`opening failed: NPC materialization: NPC MultiNpc materialization not
implemented` (crates/ddview/src/opening.rs, the family check).
Known: `NameFamilies` (ddhost/src/npc_placements.rs) reads the family table
0x0150F6F0 (16-byte records, 0x007627E0 longest prefix); 0x00762000 marks
MultiNpc by the record at 0x0150F700. The layout record carries
`mMultiNpcKind` (mtlot). The unit class is `uMultiNpc`.

## Do

1. Identify which placement(s) on that road are MultiNpc (name, cell,
   group, kind) and what `uMultiNpc` is: one actor that owns several
   bodies (a squad, a crowd, a caravan?), its construction from the
   placement and `mMultiNpcKind`, its models, motions and AI/FSM, and
   whether it talks or fights.
2. Materialize it in the opening the way the source does (bodies and
   their motion), or, for any part that is not ported yet, admit it as a
   typed placeholder that is explicitly logged and does not abort the
   route (no silent skip: one log line per placement, listed in Results).
3. Tests: the installed road placements admit without an error; a
   MultiNpc unit's members spawn with their authored models.

## Done when

The route walks past the MultiNpc cells toward q0002_event2 without the
failure, and the Result documents uMultiNpc with addresses.
