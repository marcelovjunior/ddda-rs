# character-shadows: Shadows under characters indoors

- **Owner:** codex lane
- **Branch:** `task/character-shadows`
- **Board area:** `render/character-shadows`
- **Status:** first cut by head (shadows.rs); follow-ups open

## Goal

The maintainer reports no shadow under the Arisen in Cassardis's
class-selection room. The sun's shadow works: one map over 60 m, from
the sky scheduler's `uSkyStableShadow` (main dad9693). Indoors, though,
the roof shades everything. The original most likely draws character
contact shadows with other units: the reflected classes include
`uAmbientShadow` (mStart, mEnd, mIntensity, mSize, mProjectionType),
`uProjectionShadow` (mShadowGroup, mChannel, mIntensity), `uPointShadow`
and `uSpotShadow`, and `uCharacterBase` has shadow groups.

Trace which of these give a character its shadow on the floor, indoors
and out: which unit, created by whom, attached to which joint, with what
size and intensity. Then implement it in the viewer, for the Arisen,
pawns and NPCs.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may use the oracle, with the safety rules in `docs/CODEX.md`.

## Scope

- `crates/ddview/src/character.rs` (claim), plus a new
  `crates/ddview/src/shadows.rs`
- `docs/SHADING.md` (character shadows section)
- this brief's Results

## Done when

A character in the Cassardis room shows the original's floor shadow,
compared against an oracle capture of the same spot. Fill in Results and
report to `head`.

## Results

**Head, first cut.** Traced: player vf05 -> 0x00B565A0 creates two
`uAmbientShadow` (player +0x58B0, +0x58B4), texture
`effect\tex\cm\t_cm100_07_GM` (DXT5 32x32; the shape is in the colour,
white middle, black edges), `mParentNo` 0x10 and 0x14, `mSize` x body
scale (player +0x2E54 -> +0xF78), registered with FUN_00DB58B0(0xB, ...).
Constructor defaults 0x00E79BC0: mSize 50, mFov 30, mStart 1, mEnd 100,
mStartDecay 10, mEndDecay 50, mIntensity 1, mDir (0,0,1), types 0.

`crates/ddview/src/shadows.rs` draws each as an unlit black quad on the
collision ground under its joint, alpha = texture brightness x DARKEN
(0.6, viewer choice) x a height fade (inferred). Arisen and pawns only.

**Head, draw traced (follow-up 1, partly).** `uAmbientShadow::vf11`
(0x00E7B6F0) draws a projected box. `mProjectionType` 0 (0x00E7AB30):
look-at from the joint's world position straight down (target pos -
(0,1,0), up (0,0,1); `mDir` unused), orthographic `mSize` x `mSize` full
width (0x00CFC8B0), depth `mStart`..`mEnd`. Type 1 (0x00E7B110) is the
perspective one (`mFov`). Constants 1/(`mEnd`-`mStart`) and the decays:
fade in over `mStartDecay`, out over `mEndDecay` (shader not read).
`mSpread` > 0 sets 2/`mSpread`. `mBlendType` picks draw-context blend state
+0x1520/+0x1528/+0x1530/+0x1538 (+0x1540.. with an ambient mask group).
shadows.rs now uses the traced width and both fades; joints 0x10/0x14
sit 8-10 cm above the floor (fade 0.65-0.93). Still open: the blend
state's factors and the pixel shader (so DARKEN), NPC/enemy users,
body scale, oracle compare.

**Open follow-ups (Codex):**
1. Trace the draw: the uAmbientShadow vtable's render/update, the
   shader it uses, its blend state, what mSize/mFov/mProjectionType/mDir
   mean, and how mStart/mEnd/decays fade it. Replace DARKEN/height_fade
   with the traced rule.
2. Project onto drawn geometry if that's what the original does (Bevy
   ForwardDecal needs DepthPrepass on the camera: measure the FPS cost
   with --perf first; the maintainer is FPS-sensitive).
3. Read the body scale at player +0x2E54 -> +0xF78 from the look.
4. NPCs and enemies: which of them create uAmbientShadow (search other
   callers of FUN_00E79BC0 / the DTI).
5. Oracle capture of the class-selection room vs the viewer, same spot.
