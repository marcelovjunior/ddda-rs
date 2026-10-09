# chara-creation-ui: Character creation (stage 801)

- **Owner:** codex lane
- **Branch:** `task/chara-creation-ui`
- **Board area:** `ui/chara-creation`
- **Status:** open

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

(filled in by the agent)
