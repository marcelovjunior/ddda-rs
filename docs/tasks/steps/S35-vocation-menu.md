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
