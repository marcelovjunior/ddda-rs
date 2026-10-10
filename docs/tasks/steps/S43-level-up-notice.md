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
