# chara-creation-ui: Character creation (stage 801)

- **Owner:** head
- **Branch:** `task/chara-creation-ui`
- **Board area:** `ui/chara-creation`
- **Status:** partial

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

The character creation screen, built from the `uGUIEdit` GUI data and the chara-edit readers (CHARA_EDIT.md): sex, presets, body sliders (the slider tables codex-1 resolved at 0x00780fa0), face, hair and colours, voice and name. Apply the result through ddlook to the Arisen's body, and to height and weight effects (carry capacity and speed, per PARAMS/STAMINA). Do the pawn creation screen as well if it's the same GUI.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddview/src/charaedit.rs`
- `crates/ddlook/**`
- `crates/mtgui/**`
- `docs/CHARA_EDIT.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

Stage 801 shows the original screen and the edited Arisen enters stage 200 looking as edited. Edit data round-trips against a save made with the same choices in the original, where the maintainer can provide one. Fill in Results and report to `head`.

## Results

head, 2026-10-09, first version: `ddview <nativePC> --charaedit` (or
`--charaedit-female`).
- **Fields:** all 41 `cSAVE_DATA_EDIT` fields, starting from the first
  built-in pawn of that sex (`romPawnInfo`; *viewer choice*, the original's
  starting character is not traced).
- **Rebuild:** every change rebuilds the body through
  `ddlook::save::edit_look` (0x00780fa0); Enter prints the record.
- **Allowed values:**
  - presets come from the existing `edit\parts` files (the sets are not
    contiguous: 0-8, 50 and up; female adds 100);
  - colours from the pallets, hair and beard from the edit pattern;
  - sliders are the five-step tables;
  - every other field is marked "range not traced".
- **Test:** `steps_skip_missing_presets_and_wrap`.

Open:
- the `uGUIEdit` screen and its pages (the GUI data);
- each field's real range;
- face-part assembly for the face fields (CHARACTERS.md);
- clothing during creation;
- stage 801's set;
- applying the result to the opening's Arisen;
- height, weight and speed (ddedit `ground_scale`, `body_weight` and
  `body_class` from the edited skeleton).
