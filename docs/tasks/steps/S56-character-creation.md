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

## Results (CPU validation complete; bounded capture pending)

The native class is `uGUICharacterEdit`; `uGUIEdit` above is shorthand.
Its loader `0x0060C100` loads the installed character-edit messages,
`rCharacterEditPreset` (`etc/editPattern/editPresetData`) and `rBlacklist`
(`etc/blacklist/character_name/character_name`). Child loaders
`0x00617F60`, `0x0061B0A0`, `0x0062A0E0`, and `0x0062DF10` load the basic,
detailed, body, and menu GUI resources. No game text or preset rows are
copied into source.

Implemented first milestone:

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

Regressions cover profile save/load and seat remapping, an offline
record surviving departure/reentry, recycled-seat isolation, malformed
offline EDIT rejection preserving the entire host state, installed preset
look application, native name normalization, and installed GUI name hierarchy.
Core and full release viewer validation passed.
No original-game GUI comparison or bounded editor capture has run.
This checkpoint is not Ready until the viewer gate completes.

Native subset checked 2026-10-10 in the coordinator's quiet window:
private Unicorn runs executed the original, unmodified `0x007676E0`
sanitizer against eight synthetic UTF-16 cases (8/8, actual exit0) and
`0x00617420` full-preset copying against four asymmetric sex/index cases
(4/4, actual exit0). Each asserted arrival at the normal return address
and the expected RET4/RET8 stack cleanup. Preset runs checked all38
destinations using `0x00787010` serializer field offsets and preserved
the unrelated identity prefix/tail. Explicit eye/beard/brow assertions
confirmed the corrected eye-color binding. Supplementary-character
coverage establishes two surrogate replacements in the sanitizer;
it does not establish keyboard/IME admission.

Evidence stays private in `E:/ddda-re/notes/steps/S56/`:
`preset-field-bindings.private.json`, `name-harness.private.json`,
`preset-copy-harness.private.json`, and actual-exit logs
`name-proof-20261010-095321.log` / `preset-proof-20261010-095329.log`.
No failed attempts, patched instructions, injected output callbacks,
or live-game memory were involved. This is original-code subset
comparison, not a GUI or full opening-route acceptance.

Full core gate on source `1859799` (code `6205e36`, later Results only):
`cargo test -p ddlook -p ddhost -p ddnet --target-dir
E:/ddda-rs-wt/item-use-review-target -- --test-threads=1` completed with
**193 PASS, zero failed/ignored across50 suites, actual exit0** in session6638.
All eight new core test names are explicitly present and pass once.
The installed test applies all32 full presets through dressed look creation;
the actual Host tests cover swapped/recycled seats, parked profile reentry,
and malformed offline EDIT rejection preserving save bytes/runtime hash.

The first attempt exited101 at compile: the installed test could not see
`ddlook::creation` despite its committed module declaration. A package-only
`cargo clean -p ddlook` on the verified shared target corrected stale
artifacts; no source changed before the full successful rerun. Both attempts
are retained privately: `full-core-1859799.log` and
`full-core-1859799-rerun-ddlook-clean.log`. The latter SHA-256 is
`6cef422b51f6052ea69fb21727e5a58e34b3785eed6ada2a64033b59aa652d45`;
`full-core-1859799.summary.private.json` records counts and all eight names.

Full release viewer gate on frozen `2cbcc65` (automatic integration of
reviewed main `6afbc3c`, including S52 protocol10 and the S30 helper):
`cargo test --release -p ddview --target-dir
E:/ddda-rs-wt/m5-cassardis/target --jobs 2 -- --test-threads=1` completed
**123 PASS, zero failed, two existing ignored, actual exit0** in session86377.
All three new S56 names and both expanded S35/S43 names occur exactly once
and pass. The complete125-test suite compiled in1m49s and ran in87.22s.
The first attempt succeeded; no source fix or artifact clean was needed.
The log `viewer-full-2cbcc65-first.log` SHA-256 is
`d48e0d2a09b22bc13fe950bfee16c22f23ee4b570d976adf88add63a8fdfe0fc`.

Sequential default `cargo build --release -p ddview` on that same existing
target/source completed in1m54s, session31341, actual exit0. The private
pin `ddview-s56-2cbcc65.exe` matches the built executable:95,655,424 bytes,
SHA-256 `f39d51d35bea301b33647def65bd1e0eefaba98ab4149d3fd85b4ff1df6a6f82`.
`viewer-full-2cbcc65-first.summary.private.json` and
`viewer-build-gate-2cbcc65.private.json` retain exact names/counts/log hashes.
The worktree was clean throughout both jobs. The CPU lease is released;
no viewer/GPU capture was launched under that lease. Head's later opening
chatter hooks are outside this compiled committed baseline.

Next action: obtain the explicit GPU window and capture a bounded stage801
flow with two local profile fixtures. GUI/native-page parity remains separate.

Compiled capture preparation (actual GPU capture still pending): unattended
pages now dwell for at least three successful GUI draws and 0.75 real seconds
after their fixture is prepared. With an explicit, existing private
`DDVIEW_CHARACTER_CAPTURE_DIR` inside `E:/ddda-re`, every page requests an
actual primary-window screenshot and waits for its image save acknowledgment
before advancing. Name text is entered before the dwell, rather than on the
frame that leaves the name page. Optional explicit `DDVIEW_CHARACTER_SEX_<seat>`
and `DDVIEW_CHARACTER_PRESET_<seat>` fixtures (or the unsuffixed fallback)
select installed rows; preset indices are zero-based. Capture files identify
both seat and persistent profile. After all local profiles confirm and the
story admits the opening, the capture mode writes `S56-confirmed.ddhs` through
`Host::save`, logging its saved hash. Errors block the capture flow. This is
project test automation, not native editor timing or an original-game save.
The new dwell boundary regression passes in the full release viewer gate;
the previously tested headless character-creation source is unchanged.

Bounded source-only keyboard follow-up: `0x00D89EE0` jumps to
`0x00D89800`, an internal UTF-16-to-UTF-8 converter (not an OS conversion).
Its two range checks `0x00D898C2..0x00D898DB` recognize high and low surrogate
words and branch to `0x00D89951..0x00D89959`, which emits ASCII space for
each word rather than composing a surrogate pair. The single-word callback
at `0x0075D76C` therefore cannot admit a supplementary scalar as four-byte
UTF-8 through that route. The pushed seven is output-buffer capacity,
not a conversion mode. The inverse `0x00D89EF0` jumps to `0x00D89990`;
`0x00D89A98..0x00D89AB6` rejects four/five/six-byte leading sequences through
the error return at `0x00D89BF1..0x00D89C10`. Thus the UTF-8 character counter's
four-byte support does not establish keyboard admission. Native IME/Steam
callback routing and its interaction with this conversion remain unported;
the source does not claim Bevy's supplementary-scalar behavior matches it.
No new emulator or viewer run accompanied this instruction audit.

Later milestones still need detailed face/body/color/voice/personality and
moniker pages, native page/selector animations, scene/background and editor
motions, GUI font filters/icons/IME composition and input-repeat behavior,
and the real dragon route replacing the preexisting intro stand-ins.
