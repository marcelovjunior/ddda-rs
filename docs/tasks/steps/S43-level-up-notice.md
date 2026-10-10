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

## Results (source checkpoint, 2026-10-10, codex-70)

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
- The install supplies `id/cockpit/level_info` from `rom/game_main.arc`,
  cockpit/common messages from `rom/bbsrpg_core.arc`, and the layout's bitmap
  font and textures. Native vf60 binds object indices 8/9 (level/name),
  common message 88 to object 5, and cockpit message 4 to object 4. The
  installed enter/exit global tracks span 8/6 frames respectively; panel
  pointer-array indices are 1 (Arisen) and 15 (pawn).
- Added a dedicated `level_up_notice` presenter with per-owner observation
  and queueing. It reads authoritative `OpeningRes.host.progression` and
  `PlayState.locals/follow`, initializes a silent baseline, preserves levels
  crossed between render frames, and excludes remote/support-pawn records.
  It never awards experience or edits the host. The only shared-file edits
  are the approved module declaration and plugin installation in main.rs.
- Explicit limits: Arisen name is unported because the current host/editor
  has no canonical name (head accepted follow-up in board #6374); actual
  main-pawn names come from the Rift record. The level-200 global string
  at `0x018D9230` is `<ICON INFINITY>` (codex-1 read-only live probe #6344),
  so that notice reports an unsupported icon instead of inventing text.
  Other key interpolation modes, transient local flashes, font filters,
  general GUI scripting, rank-up notices and guest progression replication
  remain open. Enter/exit endpoints and duration are authored. Their installed
  scalar/vector keys all select linear mode 3, confirmed at `0x011511C0`
  through `0x0106C670`; only that native mode is translated and has a pending
  regression test, other modes are rejected. Co-op queue eviction and
  suppressing the panel during events/pause are presentation policies.
- Tests and the release build are **pending**, queued for the normal shared
  M5 target behind codex-66. No GUI has been launched. This source checkpoint
  is not Ready and has not been compared against the original.
- Next action: integrate reviewed committed main, run the full ddview suite
  and default release build, then provide a pinned-binary capture request for
  a real opening kill/level-up (no synthetic production award).
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
