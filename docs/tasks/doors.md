# doors: Doors that open and close by themselves

- **Owner:** codex lane
- **Branch:** `task/doors`
- **Board area:** `world/doors`
- **Status:** open

## Goal

Maintainer, 2026-10-09: the "missing ground" in Cassardis (missing-ground
brief: st200_md06 floor mask 0x3 with nothing under it at level 4) is in
the original too. The original hides it with **a door that closes by
itself** at that house's entrance; in the original a row of unaliased
blue pixels shows the hole under the door. The LOD rule is right; the
viewer is missing the door.

Make doors work as in the original, for every stage: find which object
class the auto-closing doors are (uOm* door classes, their layout
records in `scr\stNNN\etc\*` / SDL, models and collision), spawn them at
their authored places, and run their open/close behaviour (what opens
them: player proximity, interaction, events; how fast; when they close;
their collision while open/closed, for the player and the camera). Check
the doors on the opening route first: the class-selection room, the
Cassardis houses and gates. Doors that are stage handoffs (door task /
JumpArea) keep working as they do now.

Build the system, not the step: one door runtime for all stages.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`. Test runs use
`--unattended`.

## Scope

New `crates/ddview/src/doors.rs` (or a runtime crate if the logic is
shared with the host), the layout readers it needs (mtlot / SDL),
`docs/WORLD.md` doors section, this brief's Results. Coordinate on the
board with the owners of opening.rs and stream.rs.

## Done when

From the maintainer's spot (top of the path by the Cassardis gate,
looking down the alley; probe points (2733, -82, -2239) stage cm) the
doorway shows its closed door, and walking up to it opens it as in the
original. Fill in Results and report to `head`.

## Results

(filled in by the agent)
