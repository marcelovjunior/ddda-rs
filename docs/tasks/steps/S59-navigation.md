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

## Results (codex-82, in progress)

- `ddnav` implements bounded sequential readers for NAV33 (`rNavigationMesh`)
  and WAY38 (`rAIWayPoint`), including authored links, inherited regions and
  spatial cells. Source: mesh load `0x01099E00 -> 0x01099100`, regions
  `0x011FB840`, mesh grid `0x0114C140`, waypoint load `0x011FD5C0` and
  field grid `0x0114D5D0`. Canonical DTI slot 10 loads; slot 11 saves.
  The initial handoff's slot-11 loader label was corrected before porting.
- Read-only private Python probes actually consumed the installed beach
  resource (852,655 bytes, 4,552 vertices, 5,708 nodes, 14,247 links,
  5,461 spatial cells) and opening field waypoint resource (81,163 bytes,
  636 nodes, 3,202 edges, 85 cells) through their exact file ends. These
  are source/format checks, **not Rust test results or original-game route
  comparisons**. Raw files and probes remain private under `E:/ddda-re`.
- The complete `ddnav` package at clean source `917ce892f685f6c473b6120bf47f9e2843dc75f7`
  passed **3 tests / 4 suite summaries**, actual Cargo and outer exit 0,
  zero failures/ignores/filtered tests/skips. Exact names appeared once:
  `installed_beach_mesh_reads_complete_topology_and_spatial_cells`,
  `installed_field_waypoints_use_distinct_format_and_short_cell_references`,
  and `navigation_rejects_partial_records_counts_versions_and_nonfinite_values`.
  Source hashes stayed unchanged through the gate. Parent grant #8767,
  normal build/core lease released after the actual process completed;
  jobs 2, existing shared core target, canonical DDDA install and exe.
  Head #8758 approved the sole Cargo.lock addition for `ddnav` and existing
  `mtarc`; the gate checked that no other lock entry changed. No shared
  host/DDNPC declarations, main branch, game files or original process edited.
  Private log `E:/ddda-re/notes/steps/S59/full-reader-917ce89-first.private.log`,
  SHA256 `E5F31CE29082BF67B1E979BBA6B501D625364DB0515B1CDE6292C5CB78FA1270`,
  and its proof JSON retain actual exit, timing (1.895 s), all eight source
  hashes and exact test occurrences.
- Request dispatch `0x0059F840 / 0x0059F7F0` selects active mesh controller
  `cAINavPathTrace`, vf06 `0x010BA660`, vf07 `0x010C5620`. Field controller
  `cAIFieldPathTrace` is a different waypoint service. The simple trace-point
  consumer uses strict distance `<` before advancing and returns 4 only
  after the final point. Native search, dynamic path, curve, avoidance,
  transformed service and host/NPC body integration are **OPEN**.
- The whole Done gate is **OPEN**. Next: translate the actual mesh
  node selection/search/trace-point construction before wiring GotoEx.

## HANDOFF (codex-82, head wrap-up #8885, 2026-10-10)

- Reader checkpoint above is tested and accepted. New spatial/projection
  source and three additional regression tests are committed as **UNRUN**:
  `native_spatial_cells_clamp_edges_and_preserve_morton_hierarchy`,
  `installed_beach_spatial_selection_finds_every_authored_triangle_centroid`,
  `native_vertical_selection_uses_strict_distance_and_authored_attribute_filter`.
  No new Cargo gate ran before wrap-up. Plane dot addition order on slopes
  remains an explicit source gap. Missing requested exclusion attributes
  with a nonzero mask error as project validation, with an unrun regression.
- Actual private native vf07 probe: four PASS cases, exit 0, 0.0296592 s,
  instruction counts 62/62/95/32. Script
  `E:/ddda-re/notes/steps/S59/native-vf07.private.py`, artifact
  `native-vf07-917ce89-first.private.json`, SHA256
  `7D8145439A26BA735BAD998ACB0ECC7ACF7A7099531DB461BD929000AB02ACDE`.
  Synthetic initialized points, dynamic flag 166 false and untransformed
  service only; no search or trace-4 arrival proof.
- Actual private vertical projection probe: four PASS cases, exit 0,
  0.0250946 s, instruction counts 309/271/308/315. Script
  `E:/ddda-re/notes/steps/S59/native-projection.private.py`, SHA256
  `07E71C19FAB910F91ACAAA4EA77C1497D5BCA67571FC242E36143FD4F1B94219`;
  artifact `native-projection-first.private.json`, SHA256
  `A021F7CF48FCFF36F61D0C4FA1964612B2CCB4A6DAF2297FE5F930972696662C`.
  Original `0x01093500 -> 0x00D665C0`, guarded output, normal return,
  stack and all four callee-saved registers checked. Four cases cover a
  horizontal hit, outside point, strict distance equality and reverse winding;
  they do not distinguish plane-dot addition order on sloped geometry.
- Source continuation: owner `0x0059DF30` sets Y offset +190=50 (constant
  `0x0150A088`), down-ray cap +1B0=100 (`0x01531AE8`), search cutoff
  +78=400 (`0x014F70DC`), and enables width-use/goal-plane flags. Constructor
  defaults differ. Stage loader `0x00501020 -> 0x010929C0` sets attribute
  index 0 and exclusion mask `0x04000000`. Dynamic flag +166 depends on
  the actual `cAISvDynamicPathFinding` service; never force it off to bypass
  the native query. Nearest fallback `0x01094FA0`, task phases, authored
  link-cost mapping, avoidance, transformed service, Trace cadence and
  current-body/command/generation integration remain OPEN.
- Exact next step: verify native plane-dot operation order, add a sloped
  regression with source-backed expectations, freeze/review source, then
  seek a fresh parent grant for ONE full `ddnav` package gate: six tests,
  no skips/ignores/filtered results, unchanged lock/source hashes, jobs 2,
  existing core target and canonical install. Only then continue the native
  query/fallback/search; S59 Done still requires the actual installed beach
  GotoEx trace result 4. Private notes/probes are under
  `E:/ddda-re/notes/steps/S59` and original Trace handoff under
  `E:/ddda-re/notes/steps/S31/S59-incoming-handoff.private.md`.
- No new research, emulation, Cargo/GPU gate, merge or rebase after #8885.
  All claims released after committing this handoff; head owns any merge.
