# Skeletons, skinning and animation

Topic findings for M2 (an Arisen that walks). Private evidence:
`E:\ddda-re\shots\char\`.

## Motion lists (`mtlmt`)

- `rMotionList` v66: all 4,313 entries in the install parse (78,093 motions,
  3,357,479 tracks, 92,246,943 keys). Layout and codecs from Riftstone (MIT).
- Checks on the data: no key starts past its motion's last frame; all 34.6 M
  rotation keys outside the coarse 7/9-bit codecs are unit quaternions within
  0.05.
- Loader `rMotionList::load` 0x00E9D330 = `rMotionList` vtable slot 10 in our
  DTI labels (the same slot as `rLayout`'s loader).
- Tracks address joints by joint id (`Bone::anim_map_index`). A keyed channel
  replaces the rest value.

## Skinning: the skin space (found 2026-10-08)

- Vertex joint numbers are **bone indices** (checked: the joints a hand
  vertex names are the bones nearest it once positions are right).
- Skinned vertex positions (S16N) are **not** quantised like static meshes.
  Static meshes use `bbox_min + n * largest bbox extent` (verified by the
  terrain matching the original game to 1 cm). Skinned layouts (those with a
  `Joint` element) use a per-model transform D:
  - Each bone's stored `inverse_bind` equals `D x inverse(rest world)`, so
    `D = inverse_bind x rest world` is one matrix per model. Measured: every
    bone of 400 skinned models gives the same D (spread 0.00).
  - Male body `m000`: D is a uniform scale of 270.48 plus the bounding-box
    minimum; its largest extent is 181.98. With the old decoding the hand
    vertices sat 48 cm below the hand bones; with D the mean vertex to
    heaviest-joint distance drops from 56.9 cm to 6.6 cm.
  - `mtmod::Model::skin_space` computes D from bone 0 and `decode` applies it.
- Inverse binds used for rendering are computed from the rest pose (the
  stored ones map from the quantised space, not model space).
- Element type 4 (`IASkinBridge2wt`) stores the joint index in the low byte
  of each component.

## Conventions (Riftstone, measured on the data; DDDA.exe joint routine 0x01051B60)

- Matrices row-major for row vectors; world = local x parent world;
  local = scale, rotation, translation. `Mat4::from_cols_array` gives Bevy's
  column-vector matrix directly.

## Seen working

- `ddview --character model\pl\m\m_base\m000\m000 --motion motion\pl\m\m00\m0000_co\m0000_co --slot N`
- Slot 0: standing idle. Slot 3: a walk cycle (38 frames, loops) with
  alternating legs and counter-swinging arms at frames 0, 10, 19 and 29.
  **Not yet compared with the original game.**

## Motion requests and blending (traced 2026-10-09, static analysis)

Addresses are DDDA.exe 2017-12-13 Steam. Not yet compared with the
running game.

**The request.** Player actions call uPlayer vf+0x90 =
`uPlayerBase::vf36` (0x00B800C0) with `(motion, force restart, flags,
interpolation frames, start frame, rate)`. It forwards to
`uObjModel::vf36` (0x00886E70), which restarts layer 0 only when the motion
differs or force restart is set, through `0x00828350` to `uModel::vf30`
(0x0105B480): `(layer, motion, interpolation frames, start frame, rate,
flags)`. Layers are `uModel::Motion` objects (0x140 bytes) in
`uModel::mMotion` (+0x380; count at +0x374). Field names are the MtDTI
reflected ones (`research/dti/dti_props.py show uModel::Motion`): +0x04
motion id, +0x40 `mInterFrame` (interpolation total), +0x44
`mInterCount` (remaining), +0x4C frame (getter `mFrame`), +0x54
`mFrameMax` (frame count - 1), +0x58 `mLoopFrame`, +0x5C `mSpeed`
(rate), +0x60 the frozen-pose weight (not reflected), +0x6C
`mInterCurve` (an `easecurve`). For layer 0, add 0x380 to get the uModel
offsets in older notes (+0x3CC frame, +0x3D4 period, +0x3DC rate).

**Default interpolation frames.** A negative length (every player action
`ddsim` models passes -1.0, except `cPlCliffHang::vf05` 0x00ADCC50, which
passes 0) is replaced by `rAdjustParam::vf15(motion & 0xFF)` on the table
at uPlayer +0x3398 + 8 × bank. Those tables are loaded (`0x00B7FF30`) from
the bank tables at 0x014F1D30 (common) and 0x0163DF8C (per weapon): each
entry is `(bank << 8, path)`, and the path is loaded both as `rMotionList`
and as `rAdjustParam`. So each player motion list has an `rAdjustParam`
twin at the same path: 20 of the install's 1,032 motion-list paths have
one (19 player banks and `e5600_at`), all with 256 floats.
`m0000_co`: idle 8; walk, run, sprint, sprint start, fall, ledge 5; jump
takeoff 3, airborne 0, landing 2; sprint-jump takeoff 3, airborne 0. The
dagger set `m0004_co` slot 17 (second jump) is 5.

**What the engine does with them.**
1. `uModel::vf30` stores the length as total and remaining and sets the
   frozen-pose weight to 1.0. On a layer with no motion yet it stores 0
   (no blend). The new motion starts at the start frame at once.
2. When the layer's motion changed (`0x01061DD0`), each joint's current
   local pose (rotation, translation, scale) is copied into a per-joint
   buffer (uModel +0xDD0), if the total is above `f32::EPSILON`. The old
   motion does not keep playing: the blend source is this frozen pose,
   which may itself be mid-blend.
3. Every update (`0x01060730`): `mInterCount` -= the model's frame step
   (`cUnit::mDeltaTime`, +0x14). The layer's `mSpeed` does not apply to
   this. While remaining
   > 0 the weight is `ease(curve, remaining / total)`. At 0 the weight is 0
   and the blend ends. The frame advances by `mSpeed` × `mDeltaTime`.
4. `ease` (0x00D4EBF0) is a cubic Bezier from 0 to 1 with inner control
   values (p0, p1). Each layer's `mInterCurve` is copied (0x01056EC0) from the
   global 0x018D06A0. That global is in zero-initialised data and nothing
   writes it (all four references read it), so p0 = p1 = 0 and the weight
   is t³. Other curves of this form in the exe: (0, 1) smoothstep at
   0x0162ACC0, (1/3, 2/3) linear at 0x0162ACC8.
5. Per joint (`0x01058A70`): rotation = normalise(lerp(motion, frozen,
   weight)), with the frozen quaternion negated when their dot product is
   negative. Translation and scale are lerped the same way.

**Start frames** (not presentation, so not done in the viewer; handed to
the `ddsim` owner). `cPlActWalk::vf05` / `cPlActRun::vf05` call
`0x00B80260(motion, start frame)`. If the motion shown is in the
locomotion list at 0x018763D0 (3, 4, 0x54, 0x55, 0x603, 0x604, 0x654,
0x655), the new motion starts at the same phase:
trunc((rate × step + frame) × new last / old last), wrapped once.
Otherwise it starts at 18 or 0 (field walk), 0 or 17 (town walk), or 20
or 0 (run). `0x00AC5FB0` picks one by comparing two joints (probably the
feet) along the facing.

**Playback rate** (layer `mSpeed`, uPlayer +0x3DC). Every request above passes rate 1.0.
`cPlActWalk::vf06` (0x00ACA1F0) then sets it to uPlayer +0x333C when
uPlayer +0x32C0 == 1, else 1.0. `cPlActRun::vf06` (0x00ACA5D0) sets it to
uPlayer +0x3340. The sprint actions never write it. No instruction stores
to +0x333C / +0x3340 by a direct offset, so the value's source is not
traced. `m_pl000_00` ids 0x1E4 / 0x1E5 hold 1.15 (0x1E0-0x1E3: 1.05, 1.1),
which fits the measured 1.15, but no reader of those ids was found. The
sprint's recorded 34.5 frames/s conflicts with the exe's rate of 1.0 for
sprint. That is an open question.

**In the viewer** (`ddview --play` / `--demo`): `mtlmt::blend` holds the
translated rules (`ease`, `Interpolation`, `mix_rotation`, `mix_vector`,
`default_frames`; unit-tested). `play.rs` loads each bank's twin table and,
when a player's shown motion changes, calls `Playback::change_motion` with
the motion's default frames: 0 for the ledge hang, and no blend for the
first motion. `character::animate` freezes the joints' local transforms,
advances the fade by the frame time × 30, and mixes after sampling.

## Assumptions and gaps

- The blend curve parameters (0, 0) come from static analysis (no writer
  found). Check on the running game: layer 0 `mInterCurve` (uPlayer
  +0x3EC / +0x3F0).
- Within one update, the original's order of request, freeze, step and mix
  is not traced. The viewer freezes, steps, samples, then mixes, so the
  first frame after a change is already partly blended.
- The original freezes only joints that pass per-joint flag tests (joint
  +0x90 & 0x41, the new motion's track mask). The viewer freezes every joint.
- Motion frame rate taken as 30 fps; interpolation is nlerp. Neither traced.
- Bevy skins with four influences; eight-weight layouts keep the four largest.
- Root track (joint 255) and absolute-space tracks are not applied.
- Slot meanings (which slot is walk, run, attack) come from the player's FSM
  and motion tables, not yet read. 0 and 3 were identified by eye.
- Faces, eyes and equipment pieces are not assembled yet.

### Compared with the original (2026-10-09, oracle trace)

claude-4 traced a scripted run → jump → land live, sampled after each
`sMain::vf06`, at about 65 fps (private file
`E:\ddda-re\notes\oracle\runjump_trace.json`).

**Layer 0 of uPlayer:**
- +0x3C0 is the blend length L of the current transition, in 1/30-s
  frames.
- +0x3C4 counts down from L by `mDeltaTime`. It is decremented on the same
  frame as the transition and clamped at 0.
- +0x3CC is the motion frame (1/30-s units; it resets on a new motion),
  and +0x3D0 holds the previous frame's value.
- +0x3EC / +0x3F0, the curve controls, read 0.0 on every frame, which is
  consistent with the t³ ease.

**Blend lengths measured:**

| Transition | Frames | Matches the motion-list table |
|---|---|---|
| idle → run | 5 | yes |
| run → jump start (action 7) | 3 | yes |
| jump start → airborne (action 9) | 0 | yes |
| airborne → landing (action 14) | 2 | yes |
| landing → idle | 15 | not checked against the table yet |
| standing idle | 8 | yes |

Action ids at uPlayer +0x2DD4: 0 idle, 2 move, 7 jump start, 9 airborne,
14 landing.
