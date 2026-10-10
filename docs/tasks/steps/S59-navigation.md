# S59: Navigation: the stage path data and the Trace path queries

- **Branch:** `step/s59-navigation`
- **Files:** new reader crate or module for the navigation resource
  (`crates/ddnav`), `crates/ddnpcrt` (Trace adapter), `crates/ddhost`
  (query service), tests; docs/NAVIGATION.md (new)
- **Kind:** research + reader + one query port (unblocks S31 movement)

## Context (codex-80 #8472/#8478/#8508/#8557)

The seven beach victims' authored GotoEx commands (six on NPC704/708/709)
are `Trace` records with path use, path update and avoidance on, width
off. Native Trace: vf03 0x0057B560 maps the fields; 0x0057B9A0 with path
use selects 0x0057BCE0 (the normal stage-200 path) unless the owner's
internal mode A3 forces the direct branch 0x0057CC00; vf19 0x0057B880
copies only the authored fields. Completion: 0x0056DCC0 maps trace result
4 to interface done, 3 to running movement. Nothing in the project reads
navigation data or answers path queries; straight-line movement would
not match the source and is not allowed as a substitute.

Start from codex-80's handoff notes (private):
`E:/ddda-re/notes/steps/S31/S59-incoming-handoff.private.md` (Trace exports,
the seven installed graph reads, owner modes, the Nav/controller call sites
0x0059F990 / 0x0059F840 / 0x0059F8E0, capsule/sweep constraints).

Known before start (codex-1 #8650, from the S17 inventory): stage 200 has one
`rNavigationMesh` (`scr\st200\etc\st200_nav`, archive
`rom/stage/stage200/stage200_nav.arc`, type hash 0x4EF19843); stage 100 uses
`split_way` `rAIWayPoint` resources instead, a different system. DTI:
`cAIFieldPathTrace` derives `cAIWayPathTrace` -> `cAIPathTrace`. The beach
needs the mesh path first; the field's waypoint path is the second half.

## Do

1. Find the navigation resource(s): their class (DTI), archive paths per
   stage/cell, and how the stage loads them (the nav/BBS manager that
   0x0057BCE0 queries). Write the format into docs/NAVIGATION.md.
2. A reader for that resource, with installed tests over stage 200 (the
   beach) and stage 100 (the field).
3. Port the query the Trace path branch makes: start/goal to a path (or
   the next waypoint), path update cadence, avoidance as far as the source
   shows, and the trace result codes 0..4. Headless, deterministic.
4. A Trace adapter in ddnpcrt that drives an NPC body along that path
   through the existing movement/sweep, reporting the native results, so
   S31's GotoEx can complete by the authored mechanism.

## Done when

An installed test walks a beach victim's authored GotoEx to its trace
result 4 over the real navigation data, with no direct-line fallback;
NAVIGATION.md documents the format and the query with addresses.
