# game-hud: The original HUD from the GUI data

- **Owner:** codex lane
- **Branch:** `task/game-hud`
- **Board area:** `ui/hud`
- **Status:** working (geometry/binding checkpoint; comparison gate open)

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

First checkpoint: `mtgui::layout` exposes the authored animation objects,
texture atlas rectangles, colour/texture keys and control selector ranges.
`ddview::hud` loads cockpit bars, numeric HP and keyboard action artwork;
the old overlay remains temporarily for value comparison. Release viewer
build passed; 13 GUI tests including the installed cockpit/palette and all
216 GUI archive occurrences passed. The first bounded screenshot verified
HP digits 410 against the old overlay's 410/410; it exposed animation-driven
colours, which the next build corrected. No original process was controlled.

Still open: reference image comparison after colour/prompt corrections,
font rendering, exact pawn anchor, Converse availability/event visibility,
followed-seat key artwork and final old-overlay removal. Not a completed HUD.

Second checkpoint: installed GFD bitmap fonts now render prompt text; traced
VK binding selectors fix J/K/Space artwork. Local keyboard slot selects the
corresponding P1 or P2 key art rather than assuming seat zero; remote followed
seats have no local key prompts. Opening-session read-only queries suppress
Converse when unavailable and HUD during events. Original interaction remains
host-seat-only, reflected explicitly in its availability query. All 16 mtgui
tests passed, including installed resources and missing/truncated animation
pools. Screenshot `ddda-hud-third.png` (private TEMP) visually verifies original
font, corrected key art, green health and faded full stamina with HP410.
Pawn projection, final reference comparison and overlay removal remain open.
