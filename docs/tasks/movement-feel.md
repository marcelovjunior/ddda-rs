# movement-feel: Movement feel: turning, locomotion blending, air attacks, falls

- **Owner:** codex lane
- **Branch:** `task/movement-feel`
- **Board area:** `player/movement-feel`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

The maintainer's playtest (2026-10-09): (1) WASD movement snaps between 8 directions with no turn or blend, while the original turns and blends; (2) holding W while moving the camera makes the character move erratically or desync; (3) there's no attack in mid-air; (4) there's no falling or landing animation after a high drop. MOVEMENT.md already records the gaps: "Turning is instant", "No animation blending", a recorded ~70 deg/s turn of the velocity direction (cause untraced), and the rotator at uPlayer+0x32C4. Trace the original's turn (velocity and model yaw), its locomotion blend (motion blend weights and times from the LMT/motion lists), the air-attack actions and the fall and landing states with their heights. Then implement them in ddsim and the viewer animation. For (2), reproduce it first (a scripted W hold with a camera sweep, logging position, velocity and yaw per frame), then find the cause.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddsim/**`
- `crates/mtlmt/**`
- `crates/ddview/src/anim*.rs`
- `crates/ddview/src/play.rs` (claim)
- `docs/MOVEMENT.md`
- `docs/ANIMATION.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

Turning and blends match a recording of the original (an oracle trace of yaw and velocity during a 90 deg and a 180 deg turn); the W-plus-camera case is smooth, with a regression test; an air attack and a fall from above the original's fall threshold play their original motions. Fill in Results and report to `head`.

## Results

(filled in by the agent)
