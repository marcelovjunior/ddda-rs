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

## Assumptions and gaps

- Motion frame rate taken as 30 fps; interpolation is nlerp. Neither traced.
- Bevy skins with four influences; eight-weight layouts keep the four largest.
- Root track (joint 255) and absolute-space tracks are not applied.
- Slot meanings (which slot is walk, run, attack) come from the player's FSM
  and motion tables, not yet read. 0 and 3 were identified by eye.
- Faces, eyes and equipment pieces are not assembled yet.
