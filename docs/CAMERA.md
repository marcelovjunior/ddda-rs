# Cutscene camera lists

`mtcamera` reads `rCameraList` (LCM version 3) and samples its base motion
camera. Evidence: the 2017-12-13 Steam executable identified in
[METHODOLOGY.md](METHODOLOGY.md), read-only Ghidra/disassembly and the
installed resources, checked 2026-10-09. The MIT Riftstone format notes
(`camera.py` section in the private reference tree) guided the initial
survey; the Rust implementation was written from the loader and consumer,
without copying reference code or game data.

**Implemented:** bounded borrowed reader, raw per-frame access, fractional
sampling and base pose reconstruction. **Tested:** five synthetic tests
and two installed-data tests. **Compared with the original:** no camera
playback or rendered-frame comparison. This does not implement an event
player or the actor-relative adjustments of `uOfsMotionCamera`.

## Binary layout

All numbers are little-endian. All offsets are from the start of the
file. Loader `rCameraList::vf10` **0x01127660** checks magic/version,
relocates non-null camera table entries, then relocates their four sample
arrays. Writer **0x01127710** confirms the layout and element strides.

| Header offset | Type | Meaning |
|---|---|---|
| 0x00 | 4 bytes | `LCM\0` |
| 0x04 | u16 | version 3 |
| 0x06 | u16 | number of camera slots |
| 0x08 | u32[slots] | camera block offsets; zero is an empty slot |

Every installed list has 256 slots, but the reader accepts the count in
the header. A cut number is a **slot index**, not the index in a compacted
list of present cameras. `CameraList.cameras` therefore preserves nulls.
The XML-to-binary builder **0x01127c30** explicitly creates 256 slots.

Each non-null offset addresses a 44-byte block:

| Block offset | Type | Meaning |
|---|---|---|
| 0x00 | i32[4] | user data, preserved; no application meaning established |
| 0x10 | u32 | frame count N |
| 0x14 | u32 | FOV type (0 vertical, 1 horizontal) |
| 0x18 | u32 | opaque metadata, preserved without interpretation |
| 0x1c | u32 offset | N eye positions, three f32 each |
| 0x20 | u32 offset | N stored targets, three f32 each |
| 0x24 | u32 offset | N XYZW rotations, four f32 each |
| 0x28 | u32 offset | N FOV values, one f32 each, degrees |

User data comes from `rCameraListXml::CameraInfo.mUserData[4]`: reflection
**0x01128120** registers four signed integers at object +0x2c, using the
name at **0x01441820**; **0x01127c30** copies them to block +0x00.
All four are zero in the corpus. The +0x18 word comes from the XML source
camera object +0x88; it is **not read by the base sampling consumer**.
Its six observed bit patterns are 0x3faa9fbe, 0x3faaaaab, 0x3fe38bac,
0x3fe38e39, 0x3fe38ef3 and 0x40066666. Its meaning remains unresolved.

`uMotionCamera::vf08` **0x00fab460** identifies the arrays by their writes
to reflected `uCamera.mCameraPos` +0x40, `mTargetPos` +0x60 and `mFov`
+0x3c. Element getters are **0x00fa97a0** (eye), **0x00fa97d0** (target),
**0x00fa9800** (quaternion). `uMotionCamera::vf21` **0x00faa920** adds
`mZoom`, clamps FOV to 0.01..179.99 degrees, converts by 0.017453292,
then constructs projection; a nonzero FOV type adjusts it for horizontal
FOV. Projection is documented, not implemented here. The FOV_V/FOV_H
names also agree with the reference notes.

The parser bounds-checks the header, offset table, every block and every
sample array using checked arithmetic before allocation/indexing. It
borrows sample arrays; a malicious frame count cannot request an enormous
sample allocation. Zero-frame blocks and null slots remain representable.
It accepts padding, overlapping in-bounds spans and trailing bytes because
the loader does not prohibit them. It does not normalize quaternions,
replace nonfinite values, or reject unknown metadata/enums. Those bytes
must remain inspectable; the install test performs the numeric checks.

## Keys and interpolation

LCM v3 has **dense integer-frame samples**, without a sparse key or
interpolation-mode table. The SDL timeline chooses `mCut` and `mFrame`.
The following pure helpers translate the relevant portion of
**0x00fab460**; tests cover boundaries, fractional time, antipodal
quaternions and reconstructed directions.

1. Skip a missing resource, out-of-range/null cut or zero-frame camera.
2. Clamp negative local frame to zero. Without `mRepeat`, clamp to N-1.
3. Truncate to the lower integer frame. The disassembly at
   **0x00fab543..0x00fab580** changes x87 rounding to *toward zero*;
   Ghidra's `ROUND` spelling is misleading here.
4. For a nonzero fraction, choose the next row, wrapping to zero at N.
   Eye and stored target use `a + (b-a)*fraction`; FOV uses
   `b*fraction + a*(1-fraction)`.
5. Quaternion interpolation follows the shorter arc. It takes the
   absolute dot product and, for a negative dot, negates the **first**
   endpoint's weight. Below **0x3f7ffffe** (0.9999998807907104, constant at
   **0x0161c9e4**) it uses spherical weights; otherwise linear weights.
   It does not normalize afterward. Rust uses standard `acos`/`sin`,
   not the executable's vector math implementation, so this is not a
   claim of bit-identical transcendental results.
6. **0x00fab8e4..0x00faba6a** reconstructs up from the quaternion's Y axis
   and target from its negative Z axis, scaled by the distance between
   the interpolated eye and stored target. Thus `Sample.target` and
   `Sample::pose().target` are intentionally distinct.

`Camera::sample(frame, repeat)` returns `None` for nonfinite time or an
empty camera. Repeating samples require frame < N; this deliberately
does not invent modulo-time semantics for externally supplied oversized
frames. The original advances time afterward by `mSpeed*mDeltaTime` and,
when reaching N, resets to zero for repeat or clamps to N-1 otherwise.
Cut advancement and `mCutRepeat` are caller state and are not implemented.

The returned pose is still in the camera's resource coordinate system.
`uOfsMotionCamera::vf08` **0x007eb9b0** calls the base sampler, adds
`sWorldOffset` +0x50/+0x54/+0x58 to eye/target and may adjust for the
actor. Its **0x007ebba0** late update adds further offsets and target
adjustment. Those paths, parent transforms, roll/track offsets and the
viewport binding are outside this reader. A future player must supply
explicit per-view/player context rather than a global player.

## st200 ev10: camera references resolve

See [EVENTS.md](EVENTS.md) for the wake-up scene. The install test reads
`event\st200\ev10\st200ev10` through `ddquest::sdl::Scheduler`, finds
its actual `rCameraList` resource key and resolves
`event\st200\ev10\list\camera\camera`. The list has 256 slots, with
slots 0..7 populated. `mCut` keys select each slot, and the same camera's
`mFrame` keys use count mode 1 starting from 0 at each cut.

| Cut | SDL start | Next cut / timeline end | LCM frames |
|---|---:|---:|---:|
| 0 | 0 | 460 | 460 |
| 1 | 460 | 670 | 210 |
| 2 | 670 | 790 | 120 |
| 3 | 790 | 910 | 120 |
| 4 | 910 | 1030 | 120 |
| 5 | 1030 | 1150 | 120 |
| 6 | 1150 | 1270 | 120 |
| 7 | 1270 | 1702 | 433 |

The last camera has one more row than the 432-frame final timeline
interval. The event ends at timeline frame 1701; the extra row is retained,
not treated as a parser error. Scheduler cut snapping (EVENTS.md,
**0x00ddeb00**) remains necessary to avoid fractional interpolation across
a cut. Resolving the data does not establish final rendered camera poses,
because this timeline also has actor adjustments.

## Validation and next action

`cargo test -p mtcamera -- --nocapture`:

- Five synthetic regression tests: all truncation lengths of a complete
  fixture; bad magic/version/count/offsets; zero frames; null slots;
  preserved metadata; clamp/repeat boundaries; linear/spherical sampling;
  antipodal quaternion sign; quaternion-derived target and up.
- Full install: **109 paths, 195 archive copies, 49,920 slots, 1,411
  non-null cameras, 195,024 frames**. Every copy parses. All numeric samples
  are finite; every quaternion's squared norm differs from 1 by <0.0001.
  Half-frame samples and reconstructed poses stay finite. FOV types are
  exactly {0,1}. These totals include duplicate archive entries.
- The actual st200 ev10 resource reference, all eight cut indices,
  corresponding frame-count keys and LCM frame lengths are asserted.

Set `DDDA_NATIVE_PC` to another install root. The install tests explicitly
skip if the default root is unavailable, fail for a missing explicit root,
and fail if an existing root contains no camera lists. No game bytes or
external research exports are included.

Open: +0x18 metadata semantics; user-data application meaning; final
`uOfsMotionCamera` actor-adjusted pose; original playback comparison.
Next action: integrate the sampler with an event-player timeline and trace
the st200 ev10 adjustment targets before a recorded pose comparison.

## Follow camera: near plane and actor fading

Read-only exe trace, 2026-10-09, camera-near:

- `uCameraGame::vf22` **0x007dc8a0** writes **96 cm** to `mNearPlane`
  (+0x34), at **0x007dce13**. Its projection consumer **0x007dc810** uses
  that field directly. No wall-dependent near-plane change was found in
  the normal update **0x007dcf30**; this is not a claim that every camera
  implementation uses the same near plane.
- **0x0088bdc0**, called by `uCharacterBase::vf97` **0x0084c800**, computes
  near-camera opacity. It selects semantic joint **1** through
  `uModel::vf22` **0x00ff9a20**; missing IDs use the model matrix.
  Normal distance uses all three axes, with a separate horizontal test.
- Threshold globals **0x01875de4..0x01875dfc** are read from the installed
  exe: ordinary inner/outer 64/74 cm, stage 100 non-pawn 80/90 cm
  (90 is the code constant at **0x01516f38**), pawn 130/140 cm,
  special camera 135/160 cm.
  The special camera also changes the distance reference and is not
  implemented by the viewer adapter.
- The four states live at actor +0x2520, timer +0x251c. Visible enters
  fading-out only when squared 3D distance is **strictly below 40000** and
  squared horizontal distance is **strictly below inner squared**. The
  entry frame remains opaque. Fading-out advances by actor delta time,
  clamps the timer at four frames and returns `1 - timer/4`. Hidden
  enters fading-in when either distance is strictly above its outer
  threshold (200 cm in 3D); that transition frame remains transparent.
  Fading-in returns `timer/4` and changes to visible at five frames.
  Thus its source value can exceed one; the rendering adapter clamps it.
  Changing distance during an in-progress fade does not reverse it.
- The resulting multiplier is combined with actor +0x2510 and stored at
  +0x2514. `uPlayerBase::vf11` **0x00b6a070** copies it to the reflected
  `uBaseModel.mTransparency` +0x158 and sets model flag 0x40000000 below
  one. The source has additional global bypasses and an aiming-mode pawn
  attenuation cone, which remain unimplemented.

`mtcamera::fade` implements the ordinary four-state rule with explicit
distance/threshold inputs and caller-supplied bypass. The viewer uses
current morphed joint positions after transform propagation, per-view and
per-actor state, and private material clones for the PlayState Arisen and
pawns. It currently supports the process's single followed camera;
simultaneous views require per-view draw material selection. NPCs outside
PlayState, special cameras and the original global bypass flags are not
covered. The renderer approximates model transparency with alpha blending,
restoring authored alpha mode/depth state afterward. Alpha-tested parts,
sorting and shadows still require an original-frame comparison.

### Collision followups found during this trace

**0x007de570** rejects an attribute with bit 0x10 *after* the nearest ray
or sweep result. It does not continue to the next surface. **0x007c6f90**
resolves the collision result's material flag word for stage/static hits.
The sphere wrapper **0x010fe5e0** calls **0x010fdea0**, which supplies the
target as the start and `near-plane-centre - target` as displacement.
Thus this is a sweep outward from target toward the candidate centre,
not a search for the first clear point moving inward. **0x010fd710**
applies a 0.1 cm contact-normal offset; initial overlap has a separate
penetration response. The followup replaces bisection with an outward
nearest-contact cast and applies that response. Contact geometry reuses
`ddsim::sweep` conservative advancement, already used for character
collision. It is a geometric replacement for the original GJK callbacks;
degenerate contacts, tied contacts and collision-category prefilters have
not been compared. Ray and sweep both reject bit0x10 only after selecting
the nearest result, preserving the source's no-search-past behavior.

Field stage 100 already includes layout-object collision through
`Terrain::add_scenery_cells`. Nonfield loading uses only the stage merged
SBC in `ddhost::install::stage_collision`. The camera-near followup now
preserves its authored material record and triangle attribute, matching
the field loader (previously these were discarded by `Terrain::add`).
An installed regression verifies unchanged triangle count and 100 surface
casts against the actual Cassardis SBC metadata, including nonzero flags.
`Ground::ray_with_flags` carries this flag word to the camera hit filter.
Whether the reported Cassardis porch pillar
is represented in that merged SBC remains to be checked; no new scenery
collision or guessed near-plane reduction is introduced here.

### Actor collision offset and its smoothing (source traced, not implemented)

The camera +0xB98 offset in **0x007e1400** is fed by actor collision,
rather than a wall-dependent near-plane adjustment. The producer
**0x00888750** selects an actor's `cObjCollision` entries through
**0x00773340** (collection selector 1, entry flag mask 0x10). It only
considers shape types 0 (capsule) and 1 (sphere), with radius strictly
greater than **32 cm** (constant **0x01517928**). Actor eligibility has
additional state/type gates; this is not a query against every visible
actor or a replacement for the stage SBC query.

The initial overlap gate tests these shapes against a capsule from the
desired eye (+0x310) to the current eye (+0x40), radius read from installed
global **0x01875dec** (80 cm). Once any eligible shape overlaps, the
producer starts a **30 cm** sphere (constant **0x014ea160**) at
`desired_target + normalize(desired_eye - desired_target) * 32`, and casts
it with displacement `desired_eye - desired_target`. Sphere and capsule
queries are **0x00d62bb0** and **0x00d62e90** respectively. For each hit,
it measures the distance between the returned contact position and the
desired eye, then submits that nonnegative distance to **0x00421d60**.
The latter retains the greatest absolute input at `sCameraExt +0xD14`;
**0x00420910** clears the accumulator each frame. These are collider
contacts, not rendered triangle intersections.

Normal camera update **0x007dcf30** smooths that accumulator through the
scalar object at +0xB94, coefficient 0.1, output +0xB98. Constructor
**0x007dc200** selects vtable **0x01599174**; the destructor's different
vtable must not be used to identify these methods. **0x007bc800** and
methods **0x007bc910..0x007bc960** give this update for each time chunk
`h = min(remaining_frames, 1.1)`:

```text
k = min(k, 1)
damping = min(2 * sqrt(k), 1)
acceleration = -k * (value - target) - damping * velocity
velocity = velocity * h + acceleration * h
value += velocity
```

Negative delta snaps the value to the target without resetting velocity;
zero delta leaves both unchanged. The unusual multiplication of the old
velocity by the chunk length is present in the source and should not be
silently replaced with a conventional spring integrator. Inactive normal
player camera mode clears +0xB98 directly. **0x007e1400** applies the
offset before its stage sweep, under its active-camera and transition
gates, moving the eye toward the target and potentially moving the target
as well when the remaining distance is small.

This path is not wired into the viewer: it needs the selected, animated
actor sphere/capsule entries and their source eligibility state. Using
render bounds or an invented generic actor radius would not reproduce it.
Next action: expose the authored actor collision entries to a per-view
camera query, then translate and regression-test the offset/accumulator
and scalar smoother together. No live/original comparison was performed
for this trace. Ghidra drops part of the producer as unreachable; the
shape loop and contact accumulator were checked against disassembly.
