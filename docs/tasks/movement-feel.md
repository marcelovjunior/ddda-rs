# movement-feel: Movement feel: turning, locomotion blending, air attacks, falls

- **Owner:** head
- **Branch:** `task/movement-feel`
- **Board area:** `player/movement-feel`
- **Status:** review

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

head, 2026-10-09 (commits 39e5a48, 17f9ccb, 9fd01d2):

1. **Turning.** Recorded live with the oracle (keyboard, W → D → A):
   - The velocity takes the new direction at once.
   - The model yaw (+0x257C) steps toward it by adjust 3 (22.5 degrees) ×
     the unit delta, along the short arc. That is about 776 degrees/s:
     90 degrees in about 3.5 frames.
   - No turn motion plays.

   ddsim now does the same (`Player::heading`, `Sim::turn_to_heading`).
2. **Judder holding W while turning the camera.** The viewer stepped the
   sim in fixed 1/30-s frames and drew the last state, while the camera
   turned every rendered frame. Bodies, poses and goblins are now drawn
   between the last two sim frames (`play::between`).
3. **Air attacks.** The rows flagged 0x302:
   - sword AirS and AirL (hang, plunge, landing 0x704);
   - staff WandAirStrike and the air WandShot (hangs, one volley).

   The jump rows are 0x36, 0x37 and 0x47. Tests cover the air shot and
   the plunge.
4. **High falls.** A drop of 10 m or more lands in `cPlActDmgGround`
   variant 3 (motion 0x107, bank 1 `m0000_dm`). Fall damage is
   200 + 0.8 × the excess.
   - Done: the damage is queued in `Player::fall_damage`.
   - Open: HP is applied once `Fight::fall_damage` lands (codex-1).
   - Below 10 m the original plays the normal landing. `cPlActJump`
     keeps its looping air motion all the way down; there is no switch to
     the fall motion.

Open:
- the air WandShot's aim;
- AirS's slot 4 window, which is taken as always open in the air;
- what follows an air attack's end (assumed to be the fall);
- Grace and Resilience;
- pad (analog) turning;
- the net and opening viewers (`net.rs`, `opening.rs`) don't interpolate
  between frames yet.
