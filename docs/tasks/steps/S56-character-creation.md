# S56: Character creation, first milestone (stage 801: name, sex, basic look)

- **Branch:** `step/s56-character-creation`
- **Files:** `crates/ddview` (an editor module; `--charaedit` exists),
  `crates/ddlook` (edit fields -> look), `crates/ddhost` / `ddprogress` /
  `ddstory` (the Arisen's name/edit record in host state and saves), tests
- **Kind:** research + one feature (milestone; the full editor comes later)

## Context

The original's new game goes prologue -> movie -> stage 801 character
creation -> Cassardis. The opening skips 801 today: the Arisen has a default
look and **no name** (S35's header, S43's notice show it empty). Readers
exist (docs/CHARA_EDIT.md, chara-edit; `--charaedit` / `--charaedit-female`
previews; ddlook edit fields from cSAVE_DATA_EDIT), the GUI is uGUIEdit.

## Do

1. Trace stage 801's flow: the uGUIEdit pages/order, the presets, what the
   first milestone needs (sex, name entry, a preset or the basic
   face/body controls), and how the result is stored (edit record + name
   in the Arisen's save data) and handed back (JumpArea to Cassardis).
2. Implement the milestone from the installed GUI/messages: choose sex,
   enter a name (keyboard; the original's character set/limits), pick a
   preset / basic sliders, confirm. Store it as host state (per profile,
   saved with checkpoints/saves, S55 keys) and apply it via ddlook.
3. Feed the name to the GUIs that show it (S35 vocation menu header, S43
   level-up notice) and the look to the Arisen body.
4. Tests: the edit record round-trips through save/load; the chosen look
   applies; the name appears in the level-up notice text.

## Done when

The opening can pass through a stage-801 character creation that sets the
Arisen's sex, name and basic look from the installed editor data, saved
with the profile, shown in the existing GUIs; the Result lists what of
uGUIEdit remains for later milestones.

## Results (source checkpoint; validation pending)

The native class is `uGUICharacterEdit`; `uGUIEdit` above is shorthand.
Its loader `0x0060C100` loads the installed character-edit messages,
`rCharacterEditPreset` (`etc/editPattern/editPresetData`) and `rBlacklist`
(`etc/blacklist/character_name/character_name`). Child loaders
`0x00617F60`, `0x0061B0A0`, `0x0062A0E0`, and `0x0062DF10` load the basic,
detailed, body, and menu GUI resources. No game text or preset rows are
copied into source.

Prepared implementation:

- The bounded stage-801 editor chooses sex, accepts committed keyboard
  text, selects one of the installed 16 full presets for that sex, and
  confirms. It reads installed prompt/font/atlas data from `chr_edit_0`:
  native prompt binding `0x006182D6` uses instance3/object ordinal2 (ID9);
  the name panel is authored instance34. Reusing this basic panel for
  preset and confirmation is a **project milestone policy**, not a port
  of the full native page sequence. Preview camera/light/idle motion and
  keyboard bindings remain viewer presentation choices.
- Preset field copying follows `0x00617420`. The native full-preset quirk
  at `0x00617524` / `0x0061752B` reads `EyeLColor` for both eyes;
  `cSAVE_DATA_EDIT::vf03` (`0x00787010`) identifies destinations
  `+0x49` / `+0x4A` as the right/left eye fields. Beard and brow copy
  their own source `+0x25` / `+0x26` normally. An asymmetric synthetic-row
  regression exercises this binding. Earlier scratch interpretation as
  beard/brow was incorrect and was corrected before execution of tests.
  Installed tests exercise all 32 full presets through dressed look creation.
- Keyboard entry follows `0x0075D71E..0x0075D73A`: 12 single-byte
  characters, otherwise eight. Normalization follows `0x007676E0`,
  blacklist loading/matching `0x00CACAD0` / `0x00CACCC0` / `0x00CACDD0`,
  and the native 25-byte save copy `0x0060F0B2`.
  The callback counts the existing UTF-8 buffer through `0x0075D71E` /
  `0x00D88F30`; `0x00D88F7C..0x00D88FA1` counts a four-byte sequence once,
  matching Rust scalar counts for valid UTF-8. Native upstream IME and
  surrogate admission (`0x0075D76C` / `0x00D89EE0`) remains **open**;
  Bevy's committed-text input is not a port of that conversion path.
  Replacement/punctuation tables are read from the installed exe at runtime; blacklist rows stay
  installed. The editor requires a nonempty normalized name as a project
  admission gate. Unattended input requires explicit
  `DDVIEW_CHARACTER_NAME_<seat>` or `DDVIEW_CHARACTER_NAME` fixtures.
- Native confirmation `0x006153C3..0x006153E6` copies the selected
  `cSAVE_DATA_EDIT` to manual character state. Project host state uses a
  versioned `EDIT` chunk owned by persistent `ProfileId`, retaining the
  25-byte name, nickname, and 41 selectors. Every EDIT record, including
  an offline profile, is validated into a candidate before Host::load
  mutates. Host hooks are a short handoff after S53's atomic-load freeze;
  the final tested branch must include that reviewed dependency.
- Co-op policy uses sequential local-profile editors and validates that
  the original profile still occupies the seat at confirmation. Stage801
  releases only after all local profiles confirm. The existing intro is
  a local new-game flow; LAN stage801 presentation is not yet admitted by
  the viewer's new-game setup. Confirmed look restoration resolves each
  current seat through its profile, rather than supplying one global look.
- S35 uses its native name binding `0x006720D0..0x006721B4`: node1,
  ordinal9 (installed object ID11). The installed name banner is its
  parent; authored hierarchy linking follows `0x01077EC0` /
  `0x01068C40` / `0x01068C60`. Additional name glyph pages load on demand.
  S43 resolves each Arisen notice's name separately; the existing pawn
  name and queue rules remain intact. A regression round-trips named
  profile EDIT records and asserts the level-up text field.
  Head7784 approved a narrow identity correction: observing a changed
  local `ProfileId` discards that seat's prior baseline and pending/active
  notices while leaving other profiles' notices intact. A recycled-seat
  regression covers the old name not reaching the new occupant.

Prepared regressions cover profile save/load and seat remapping, an offline
record surviving departure/reentry, recycled-seat isolation, malformed
offline EDIT rejection preserving the entire host state, installed preset
look application, native name normalization, and installed GUI name hierarchy.
They have **not been compiled or executed at this checkpoint**. No original
game comparison or GUI capture has run. Cargo/GPU work waits for the shared
machine window; this checkpoint is not Ready.

Later milestones still need detailed face/body/color/voice/personality and
moniker pages, native page/selector animations, scene/background and editor
motions, GUI font filters/icons/IME composition and input-repeat behavior,
and the real dragon route replacing the preexisting intro stand-ins.
