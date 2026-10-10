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
