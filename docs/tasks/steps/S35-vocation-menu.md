# S35: The real vocation menu at the weapon table (uGUIJobChange)

- **Branch:** `step/s35-vocation-menu`
- **Files:** `crates/ddview/src/opening.rs` (replace the stub), a new
  `crates/ddview/src/vocation_menu.rs` (or inside `hud.rs` if it fits),
  message/GUI readers if missing; tests
- **Kind:** research + one feature (viewer)

## Context

At the weapon table in the Arisen's house (stage mode 0x19), the opening
shows a labelled stub: text "1 Fighter 2 Strider 3 Mage". The choice
reaches the host as the `PlayerJob` report, and `Host::apply_job_kit` gives
the Arisen and the roster the vocation's kit (done). The original shows
its vocation GUI there. Candidates: `uGUIJobChange` (string 0x01582604,
DTI data 0x0133AB30; "rom\gui\GUIuGUIJobChange") or a first-choice
variant. The first choice also gives the starting items
(`ddprogress::change::starting_items`, 0x00673740).

## Do

1. Find which GUI the mode-0x19 weapon table opens and its layout
   resource(s) (rGUI under `id\...`), its message table (names and
   descriptions of the three base vocations), and its input (cursor
   up/down, confirm, cancel, any confirmation prompt).
2. Draw it from the installed layout the way `hud.rs` draws the cockpit
   (reuse its rGUI node drawing; extend it if a node type is missing):
   the three vocations with their names/descriptions from the install,
   the cursor, confirm.
3. Keyboard and gamepad input through the viewer's existing bindings;
   the confirmed choice goes through the same `PlayerJob` path as the stub
   (no new host API). Remove the digit-key stub once the GUI works.
4. A viewer test for the choice -> job mapping and a screenshot
   (`--opening` to the table, `--shot`).

## Done when

The weapon table opens the original's vocation GUI with installed text,
choosing Strider/Mage/Fighter gives that kit (log shows `apply_job_kit`),
and the Result names the GUI class, layout and message sources.

## Source findings (codex-67, 2026-10-10)

- `0x00504330` mode `0x19` calls `0x00512FB0`, which allocates through
  `0x00671740`, invokes `uGUIJobChange::ctor(1)` (`0x00671770`), and queues
  the GUI on lane `0x1C`. This is the first-choice flag at `+0x298`; the
  later vocation handler `0x005115F0` instead invokes `ctor(0)`.
- `vf58` (`0x00672A70`) loads `id/DDN/guild/gild_jobchn` and localized
  `id/message/shop/shop_guild/shop_guild`,
  `id/DDN/message/common/common_message`, skill and ability messages.
  The installed first-choice layout has 77 nodes on a 1280 by 720 screen.
- `vf05` (`0x00671B70`) and `0x00675780` read the base-vocation list from
  installed EXE table `0x01517A0C`. `0x00673BF0` reads name indices from
  `0x01517A90` (common messages), description indices from `0x01582724`
  (guild messages), and list node indices from `0x0158269C`. These tables
  and all message/font/atlas data remain runtime reads from the install.
- The list constructor `0x00600560` clears wrap flags `+0x86/+0x87`.
  `0x00603E40`/`0x00603F80` consequently clamp the first/last row.
  First choice omits the cancel callback, hides vocation costs, and offers
  no pawn selector. Keyboard/controller mappings and discrete stick-edge
  navigation are viewer bindings; native repeat timing is not yet ported.
- `0x00674C10` starts confirmation; `vf55` (`0x00672C10`) cases 10/11
  open a Yes/No prompt when the selected cost is positive, then retain
  that same selected job on acceptance. First choice is free even though
  the offer costs are populated from `0x01517A7C`. The prompt is guild
  message `0x1B`; Yes/No are common messages `0x5C/0x5D`.
- Confirmation uses `uGUISystemWindow` (`0x00728530`/`0x00728570`),
  installed layout `id/common/sys_wind` (`vf58`, `0x00729110`), and style
  descriptor 6 at `0x01516068 + 6*0x30` (`0x00728760`). Prompt and answer
  node indices are read from that descriptor. `vf05` (`0x00728890`) uses
  selector 1 for the selected enabled answer and selector 0 for the other;
  the constructor defaults to answer 0 (Yes).

Implementation and tests are currently in progress. The viewer projects
constant objects and the selected control's texture/color/visibility keys;
this is not a general GUI animation VM. Character preview, font filters,
the tutorial popup, and the native input legend are still open. No
original-game visual comparison or GPU acceptance is claimed yet.

## Result checkpoint (codex-67, 2026-10-10)

Source commit **`f60827e`**, branch `step/s35-vocation-menu`, worktree
`E:/ddda-rs-wt/s35-vocation-menu`; based on reviewed committed main
`c7c0505`. Head approved the viewer hooks in board #5995 and the narrow
GUI Boolean-track extension in #6120.

Implemented the installed three-row vocation list, descriptions, cursor,
bitmap glyphs, and the original `sys_wind` Yes/No confirmation. The menu
state belongs to its opening session and uses the actual request owner for
local device input. Confirmation continues through the existing
`OpeningWorld::close_menu` / `PlayerJob` / `Host::apply_job_kit` path.
The digit-key stub is removed. The unattended driver uses the same
installed list navigation and confirmation operations before that handoff.

`mtgui::layout::Object` now reads Boolean animation keys from the byte
pool and samples authored visibility. This is necessary for the cursor's
normally hidden overlay, rather than discarding that object by hand.

Verified, all with actual process exit 0:

- Full `cargo test -p mtgui`: **12 passed, 5 existing ignored**;
  2.53 seconds compilation. The new synthetic regression covers byte-pool
  offsets, key frame transitions, and truncated Boolean data.
- Explicit installed `cargo test -p mtgui --test hud -- --ignored`:
  **3 passed**, 0.48 seconds compilation, 5.60 seconds tests.
- Final full `cargo test --release -p ddview`: **82 passed, 2 existing
  ignored**, 22.82 seconds compilation, 14.16 seconds tests. Three menu
  tests cover all three confirmed jobs, rejection, list boundaries and
  owner changes, plus actual installed layout/text/font/atlas projection
  and confirmation selector frames without a GPU window.
- `cargo build --release -p ddview`: **passed**, 46.65 seconds. Existing
  `opening_world`/`Args` visibility and unused audio method warnings remain.

Private binary pin:
`E:/ddda-re/notes/steps/S35/ddview-s35-f60827e.exe`, SHA-256
`1793A335057233648AF2DA6D1F9D58A492D14093647414E8E6EDFEF2F7459709`.
The pin was copied after the actual release build exited, before releasing
the shared target to the next agent. No game files or private exports were
committed.

**Open:** one bounded menu capture and actual kit log check (board #6177),
independent lane review, and original-game visual comparison. The general
GUI animation/filter VM, character preview, tutorial popup, input legend,
and native hold-repeat timing remain explicitly unported. LAN guest menu
replication is not added here; this checkpoint preserves the existing
host menu/kit policy. This is a source checkpoint, not full S35 acceptance.

Next action: inspect the requested `2-vocation-menu.png` and confirm that
the unattended Strider selection reaches the existing kit handoff; record
the capture's owned PID, actual exit, and image hash before closing S35.


### Parent review and description correction

Codex-1 reviewed f60827e and its GUI Boolean reader. Head's bounded
f60827e capture shows installed parchment/list/cursor and confirms job2;
the parent independently inspected the private PNG. The header/grey
placeholder and description overflow remain distinct findings.

The installed description uses three CRLF-delimited lines. The painter
previously replaced CR without consuming LF, inserting a blank row for
each line ending. Normalize CRLF before isolated CR; preserve intentional
blank rows and existing CRCRLF handling. No font-size or box tuning.
After proving stale shared viewer/mtgui artifacts (the new regression was
absent and visible_at missing despite source), narrow package cleans
rebuilt actual source. Complete viewer83 passed/2 existing ignored,
24.69s compile/10.44s tests; default release39.55s, actual41657 exit0.
Private log: E:/ddda-re/notes/steps/S35/full-viewer-crlf.log.

Further native audit: first-choice vf05 at 0x006722CC..0x00672363 hides
panel instance ordinals2/3/6/4 (cost controls), so filling the empty cost
box with an invented title would be wrong. The character-name instances
are nested under animation objects; their hierarchy/name binding and
the remaining grey control still need a sourced projection. Full GUI VM,
original visual comparison and a corrected-menu capture remain open.

### First-choice visibility source checkpoint (tests pending)

The opening presenter now applies the four native vf05 cost-control
visibility overrides by instance ordinal before drawing authored keys.
The installed regression distinguishes instance ordinals2/3/4/6 from
object IDs4/5/15/7 and retains description object6. No title, font or
layout value is invented. This source checkpoint is **untested** until
the parent's queued full viewer suite/release acceptance after S43.
The remaining grey shadow/header/name/hierarchy and capture gates stay
open; hiding the native cost controls does not imply they are resolved.
