# S43: The level-up notice (uGUICharacterLevelUp)

- **Branch:** `step/s43-level-up-notice`
- **Files:** `crates/ddview/src/hud.rs` (or a new GUI module), tests
- **Kind:** one feature (viewer), source mapped
- **After:** S37 core (on main: kills give experience, `Host::progression`
  levels rise one per frame and reach the fight)

## Known (head, 2026-10-10)

- `uGUILevelUpBase::vf58` 0x00676E70 loads layout `id\cockpit\level_info`
  (rGUI, hash arg 0x4F213C54) and the messages `id\message\cockpit\cockpit`
  (+0x280) and `id\DDN\message\common\common_message` (+0x284).
- Subclasses: `uGUICharacterLevelUp` (vf59 0x006772A0, vf60 0x006772F0),
  `uGUIWeaponLevelUp` (vf62 0x00677540, ctor), `uGUIJobLevelUp` (vocation
  rank). vf59 picks the three message ids: 3/4/5 when byte +0x298 is set,
  else 12/13/14 (find what +0x298 means: pawn vs Arisen, or own vs
  another's). vf60 formats the level (+0x294; 200 = a special string) and
  the name (+0x290) into the layout's text nodes.
- Who creates it: the level-up 0x00B5BD20 (its `param_2` branch calls
  0x00452CE0 for the Arisen/pawn) - confirm it is the notice.

## Do

1. Confirm the creation path, the display time/animation (vf09/vf11 of the
   base 0x00676BE0 / 0x00676C00) and the +0x298 meaning.
2. Draw it from the installed layout and messages, with the level and
   name, when a seat's character levels up (watch
   `host.progression` levels; the local seat's own notice; pawns too if
   the source shows theirs). Also the rank-up notice (`uGUIJobLevelUp`)
   if it falls out naturally.
3. A viewer test (text content for a level-up) and a capture.

## Done when

Killing goblins in `--opening` until a level-up shows the original's
notice with the right level; the Result names the sources.

## Results (tested source checkpoint, 2026-10-10, codex-70)

- Creation confirmed: `0x00B5BD20` -> `0x00452CE0` queues a category-0
  Arisen or category-1 eligible pawn record through `0x00454B90`;
  `0x00452780` creates `uGUICharacterLevelUp` through `0x00677240`.
  Byte `+0x298` means Arisen (true) versus pawn (false), not local/remote.
- Correction to the brief: `0x00676BE0`/`0x00676C00` resolve another GUI
  class. The actual LevelUpBase overrides are vf09 `0x00676F20` and vf11
  `0x00677100`. `0x006772A0` selects **sequence** ids 3/4/5 (Arisen) or
  12/13/14 (pawn), rather than message ids. Init `0x00676D20` sets the
  hold counter to 120 native frame units; `0x00677030` decrements it after
  the intro finishes, then starts the exit sequence.
  The presenter translates that state machine: intro completion does not
  spend hold time, and the old hold counter is checked before decrementing
  it (a step crossing zero starts the exit on the following update).
- The install supplies `id/cockpit/level_info` from `rom/game_main.arc`,
  cockpit/common messages from `rom/bbsrpg_core.arc`, and the layout's bitmap
  font and textures. Native vf60 binds object indices 8/9 (level/name),
  common message 88 to object 5, and cockpit message 4 to object 4. The
  installed enter/exit global tracks span 8/6 frames respectively; panel
  pointer-array indices are 1 (Arisen) and 15 (pawn).
  Parent review #6449 caught the object-link interpretation before acceptance:
  record `+0xC` is first child (`0x01068C40`) and `+8` next sibling
  (`0x01068C60`), confirmed by both setters' disassembly. Correct installed
  number-parent positions are (-80,19) for Arisen and (-90,19) for pawn;
  title/prefix/name are siblings and inherit neither offset.
- Added a dedicated `level_up_notice` presenter with per-owner observation
  and queueing. It reads authoritative `OpeningRes.host.progression` and
  `PlayState.locals/follow`, initializes a silent baseline, preserves levels
  crossed between render frames, and excludes remote/support-pawn records.
  It never awards experience or edits the host. The only shared-file edits
  are the approved module declaration and plugin installation in main.rs.
  Parent review #6491 also caught an active notice consuming hidden time
  after changing the followed seat. Each owner now retains its own active
  panel and clock; only the followed owner advances, and the new seat can
  consume its queue independently. A two-owner consumer regression covers
  switching during the intro, the other owner's complete notice, and exact
  resumption without spending hidden time or consuming the next notice early.
- Explicit limits: Arisen name is unported because the current host/editor
  has no canonical name (head accepted follow-up in board #6374); actual
  main-pawn names come from the Rift record. The level-200 global string
  at `0x018D9230` is `<ICON INFINITY>` (codex-1 read-only live probe #6344),
  so that notice reports an unsupported icon instead of inventing text.
  Other key interpolation modes, transient local flashes, font filters,
  general GUI scripting, rank-up notices and guest progression replication
  remain open. Enter/exit endpoints and duration are authored. Their installed
  scalar/vector keys all select linear mode 3, confirmed at `0x011511C0`
  through `0x0106C670`; only that native mode is translated and its regression
  passed, other modes are rejected. Co-op queue eviction, suspension when the
  followed owner changes, and events/pause suppression are presentation policies.
- Source `bda3cfe` passed the full default viewer suite under the normal shared
  M5 lease: actual session 59890 exited 0, **89 passed / 0 failed / 2 ignored**
  (28.76 s compilation, 11.78 s test execution). All six notice regressions
  actually ran, including installed text/font/tracks, cropped atlases and
  hierarchy, native hold ordering, linear interpolation, per-owner observation,
  and the followed-seat consumer. Private log:
  `E:/ddda-re/notes/steps/S43/full-viewer-bda3cfe.log`.
  The earlier `a781b6c` suite/build passed before the co-op correction and are
  superseded. No GUI has been launched and there is no original comparison.
  Final default release build session 97157 exited 0 in 40.93 s. Pinned
  executable `E:/ddda-re/notes/steps/S43/ddview-s43-bda3cfe.exe` is 94,592,000
  bytes; its SHA-256 matches the built target:
  `48175ABAD5137EF91F0DA00DD03FF49B94908ECDE4A1652F0B67F09A822FA562`.
  Build log: `E:/ddda-re/notes/steps/S43/default-release-bda3cfe.log`.
  M5 was released after actual exit and pin verification (board #6516).
  This is a reviewable source checkpoint; the brief's capture acceptance
  remains open until a genuine new level-up is pictured.
- Next action: provide the final pinned binary to codex-1 for its coordinated
  S47/S43 capture, requiring a real opening kill/level-up (no synthetic award).
  Read-only checkpoint preflight found both local profiles at level 2,
  experience 108 / next 485 in `RescueApproach.ddhs`, and level 3,
  experience 39 / next 770 in the existing `Done.ddhs`. Restoring either
  establishes a silent baseline; only a new progression transition is a notice.
  With `--shots <private-dir>`, the first real notice automatically requests
  `S43-level-up.png` once its enter track has finished. This supports a bounded
  route capture without guessing its elapsed wall time or creating experience.
- Infinity follow-up source: message vf21 `0x010FEF20` -> analyzer
  `0x01002C30` -> tag dispatch `0x01002960` -> ICON binding `0x010024F0`.
  `sGUI::vf17` at `0x00DE1770` resolves the icon name from the active name/id
  table (`sGUI+0x5C`), and `0x01001650` temporarily switches to the GUI icon
  font (`sGUI+0x58`) before inserting a type-5 glyph. The table/font resource
  initializer still needs tracing; this is not the ordinary text font's
  Unicode infinity glyph. No inferred icon number, resource or pixel is copied.

### Parent combined review and attempted genuine capture

Codex-1 independently reviewed native hierarchy/clock and co-op consumer
changes, including the two-owner follow-switch regression. Integration
435a027 on the S47 branch passed all92 viewer tests/two existing ignored,
including all six notice tests; default release48.12s, actual94503 exit0.
The genuine RescueApproach checkpoint run exited zero after151.5195s
(owned30364). It stayed at level2/experience420 below485, so no new notice
was expected or captured. No level or experience was manufactured.
The route also suppresses step shots in enable_perf_route, reported to
head separately. This is not S43 visual acceptance. The source tests are
accepted; genuine notice capture and original comparison remain open.
Private logs/pin: E:/ddda-re/notes/steps/S47/combined-rescue-435a027.err.txt
and ddview-s47-combined-435a027.exe (provenance hash in the S47 Results).

The updated source42576fd passed all92 default viewer tests (six notice
regressions included), two existing ignored; actual53534 exit0, fresh
compile40.03s/tests12.11s and release56.98s. With the screenshot path and
demo guard fixes, the next bounded RescueApproach run genuinely succeeded,
delivered Reynard's item1058 and logged S43_LEVEL_UP owner0/level3 at
09:12:49 UTC. The Arisen name remains explicitly unresolved/empty.
However the screenshot directory had not been created, so the real notice
PNG failed with OS error3. Harness56794 later exited1 on a watchdog/process
exit race; numeric viewer exit is unknown, its executable was confirmed
absent before lease release. No notice visual/original acceptance is claimed.
Private combined-rescue-42576fd.err.txt and pin provenance in S47 preserve
the evidence. Next: repeat once with the directory prepared and inspect
the genuine notice PNG; no manufactured progression or event replay.

The approved retry at source d5567f8 includes directory creation; its full
viewer92 tests/two existing ignored and release40.03s actually passed.
Screenshot saving now works, but this route failed the rescue and stayed
at level2/experience420. The single step PNG contains pre-pipeline sky/HUD;
there is no notice PNG. Owned viewer15908 exited-1 via the retained-handle
watchdog after180.7952s; its executable was absent before lease release.
The earlier42576fd genuine level3 event remains log evidence, not visual
acceptance. Next: a stable real progression route with render-ready capture,
coordinated with head; no further speculative GUI retry is planned.
