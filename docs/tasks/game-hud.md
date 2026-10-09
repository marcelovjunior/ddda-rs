# game-hud: The original HUD from the GUI data

- **Owner:** codex lane
- **Branch:** `task/game-hud`
- **Board area:** `ui/hud`
- **Status:** open

## Goal

Replace the viewer overlay with the game's own HUD, built from `mtgui` (`docs/GUI.md`): the health and stamina bars, party pawn bars, the minimap frame if feasible, and the action prompts. Bind it to the sim and host state. Use the GUI textures and the layout node trees.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs must not grab the mouse or pause on focus loss.

## Scope

- `crates/ddview/src/hud.rs`
- `crates/mtgui/**`
- `docs/GUI.md`
- this brief's Results

Shared viewer files (`play.rs`, `main.rs`) need a board claim and
coordination with the agent that holds them.

## Done when

The `--play --save` HUD shows the game's bars and prompts at the original positions, compared with a reference screenshot (ref001 has the HUD). Fill in Results and report to `head`.

## Results

(filled in by the agent)
