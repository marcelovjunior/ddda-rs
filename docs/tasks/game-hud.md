# game-hud: The original HUD from the GUI data

- **Owner:** codex lane
- **Branch:** `task/game-hud`
- **Board area:** `ui/hud`
- **Status:** working (final screenshot verification)

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

Implemented installed cockpit health/stamina bars, numeric HP, the simulated
main pawn's world bar and action prompts. The renderer reads authored geometry,
atlas regions, colour keys, GMD labels and GFD bitmap glyphs at runtime. It
replaces the temporary viewer overlay. Bindings follow the selected Arisen;
local P1/P2 prompt artwork uses the corresponding input slot. Converse/event
visibility queries reflect the opening session's current availability.

Evidence and validation:

- 16 mtgui tests pass, including all 216 installed GUI occurrences, cockpit/
  palette geometry, font pages and malformed/missing pool checks.
- 26 viewer release tests pass, including the installed male/female joint-map
  check and exact pawn-anchor regression. Release viewer build passes.
- Pawn anchor: 0x007325d0 uses mapped joint4 +25cm Y. uModel::vf23 at
  0x00ffddb0 maps actor+368 to model+110; rModel::vf10 at 0x00fa8fe0 reads
  the 256-byte bone remap there. 0x00679a10 adds 70cm toward the camera and
  the screen offset. No highest-joint guess remains.
- Private TEMP/ddda-hud-final.png (1920x1080, --play --save, --unattended,
  eight-second bounded capture) compared with private ref001.jpg. HP starts
  around (364,959), full width 228px, matching the reference layout. Numeral,
  stamina strip and Heavy/Light/Jump row positions align. J/K replace the
  reference's mouse bindings. Save HP 410 differs from ref001's 455 and world/
  camera state differs; this compares HUD geometry/artwork, not whole-scene
  pixel identity. No original process control or installation writes occurred.

Commits 86f981c/f5401aa established geometry/fonts; merge 65905f8 brings in
main's item/skill hooks. Final anchor/overlay changes follow that merge.

Limitations: recruited pawn entities, pawn names/status icons, recoverable HP
and status/flash animation remain incomplete; minimap is deferred. Original
GUI font filters are not executed, so text edge treatment differs. Pawn
world anchoring has source, unit and installed-model verification; a matching
visible-pawn screenshot remains to be captured. Head owns shared milestone updates.
