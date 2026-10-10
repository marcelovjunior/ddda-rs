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
  implementation uses the same near plane. **Correction from camera-feel:**
  this is the child camera, not the active rendering/collision camera.
  `uCameraCtrl` inherits **16 cm** from `uCamera::ctor` **0x01128740**
  through **0x007d3a80 / 0x007d6cd0**. Its update **0x007d7210** copies
  the child eye/target/FOV, but not its near field. **0x007de570** reads
  `sCamera+0xce0` (the controller). A paused and 40-frame unpaused
  Cassardis sample both show controller near 16, child near 96, with
  identical eye and target. Follow-camera collision and projection now
  use 16 cm, giving a 20.8 cm aligned sphere rather than 124.8 cm.
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

Camera-feel correction (2026-10-09): **0x007e1400**, the caller of
**0x007de570**, translates the look target by the complete eye correction
at **0x007e1b11..0x007e1b68**. This includes off-axis initial-penetration
recovery. Moving only the eye and then looking back at the old target
incorrectly rotates the view. `follow_camera` now applies the caller's
translation to both endpoints; a wall and low-ceiling overlap regression
checks the preserved eye-target vector. Six camera-filtered release tests
and the release viewer build pass. Live wall/roof comparison is pending.

The subsequent Cassardis wall comparison records the candidate eye/target
on entry to **0x007e1400**, then its final pose after that frame. This
avoids confusing authored target height, camera lag or actor avoidance
with the collision correction. A real rock-wall contact moved eye and
target by the same approximately `(-58.379, -16.552, 0.013)` cm. Replaying
that candidate through the viewer collision helper and the installed
stage200 SBC gave **0.000788 cm** eye/target error; two neighboring clear
angles gave zero error. The private opt-in diagnostic
`camera_compare_private_original_poses` reads externally supplied samples
and prints errors; it does not claim an automatic tolerance gate.
No capture data is embedded in the repository. The low-roof contact and
full viewer pose comparison remain pending. The normal target height
still uses the earlier measured 140 cm approximation.

**0x007de570** retains a post-query bit-0x10 rejection, but this is not
its entire filter. The camera sets `sCollision::Param +0xc` to
**0x4081d01c** through **0x007e178d / 0x00de4710**. Static SBC triangle
filter **0x01165660** calls `sCollisionExt::vf10` **0x00422f40**: bit
0x40000000 selects exclusion, rejecting any material with bits in
**0x0081d01c** before nearest-contact selection. Thus an excluded nearer
surface must not hide an eligible farther one. The ray wrapper
**0x00dea870**, sweep traversal **0x01166b10**, and overlap traversal
**0x01166c00** share this filtering. **0x007c6f90** resolves the resulting
material flag word for the final post-query check.
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
not been comprehensively compared. Camera-local ray, sweep and overlap
now apply the sourced material exclusion before selecting their result.
The ray retains its existing fast path when the nearest hit is eligible;
otherwise it recollects triangles and uses Terrain's existing two-sided
Moller-Trumbore geometry policy. This duplicates a query on excluded hits;
a shared filtered Ground API is a future optimization, not part of this
checkpoint. Ground providers without triangle data cannot search beyond
an excluded hit. Backface/degenerate behavior remains unverified.

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

### Actor collision offset and its smoothing

The camera +0xB98 offset in **0x007e1400** is fed by actor collision,
rather than a wall-dependent near-plane adjustment. The producer
**0x00888750** selects an actor's `cObjCollision` entries through
**0x00773340** (collection selector 1, entry flag mask 0x10). It only
considers shape types 0 (capsule) and 1 (sphere), with radius strictly
greater than **32 cm** (constant **0x01517928**). Actor eligibility has
additional state/type gates; this is not a query against every visible
actor or a replacement for the stage SBC query.

The producer requires the followed actor and its camera (+0x2EA4), rejects
followed-actor +0x2360 bit0x02000000 and the actor referenced at its
+0x2104, and accepts only candidate +0x1FC0 value4 or the combination
of candidate +0x206D bit0x10 and +0x2574 value0x2369. These fields' complete
semantic enums remain unresolved. For candidates with +0x206C bit0x80,
subtypes 0x5C..0x5E and 0xB1..0xB4 are additionally rejected when model
centres are strictly less than 200 cm apart. Do not broaden this to every
enemy, pawn or nearby character based on a guessed enum meaning.

The entry mask 0x10 is the authored group's **mIsObjAdj** flag: reflection
**0x00ccd510** places it at group +0x0E, and **0x0076fab0** translates it
into entry +0x1C0 bit0x10. Existing `ddcombat::ObjCollision` already reads
these groups; `ddcombat::volumes::build_volume` supplies the source-backed
joint placement. Integration therefore needs the live selected entries
and actor eligibility, rather than a new collision resource parser.

**0x00770630** appends accepted entries with that flag to collection 1
at `cObjCollision +0xAB4` (count +0xAB8, data +0xAC4). Acceptance first
calls the optional +0xCC0 callback; **0x00885c10** installs thunk
**0x0088cf30**, dispatching owner virtual slot 64. This can remove flags:
`uEnemy::vf64` **0x00aaa3d0** removes bit0x10 when owner +0x2748 bit8 is
set. Goblin override **0x008d34f0** also removes it when virtual slot117
returns true or motion is 0x282, 0x283, 0x284, or 0x28C..0x28F. The
existing `ddenemyrt::Body::push_volumes` builds activated authored
object-adjustment groups, but does not represent all these callback
gates or runtime-created entries. It is therefore a partial foundation,
not yet an exact collection-1 provider. **0x0076b2c0** clears collection1's
count (direct call at **0x00baf5cc**, `uNpc::vf08` path). The offset
producer itself is called at **0x00886850**, near the end of
`uObjModel::vf09` (**0x00886530**). These call targets were checked from
instruction bytes because Ghidra's reference query mixed references to
the reset and producer. A provider must preserve the selected posed
entries at this phase rather than rebuilding an arbitrary set from a
later motion frame. Normal `uObjModel::vf90` **0x00885e30** inlines that
reset when actor +0x2398 bit4 is enabled: actor +0x1B98 is collection1's
count. Phase93 **0x0076b470** activates mask0x18 and poses collection1's
entries; pass-C **0x0076b300/0x0076b660** process masks0x180/0x7FFFFE6F
before the camera producer. This identifies the object-adjustment phase
as a separate snapshot from the later attack/hurt-volume collection.

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
gates. It computes `shift = normalize(target - eye) * offset` and adds it
to the eye. If `length(shift) - 16 < offset`, it also adds
`shift * offset / length(shift)` to the target. Disassembly
**0x007e19da..0x007e1a5b** confirms the comparison uses the shift length,
not remaining eye-to-target distance. For the producer's positive offsets
the target therefore normally moves by the same shift as the eye.

`mtcamera::actor_offset` implements the query construction, per-frame
maximum accumulator, scalar smoother and pre-stage-sweep pose shift
independently per view. Its
`EligibleEntryProvider` boundary explicitly requires the actor gate,
selected active entries and broad/narrow collision tests; there is no
default provider that silently substitutes all hurt volumes. Four unit
regressions cover query start/displacement and contact reduction,
max/tie/reset behavior, and time chunking plus velocity preservation on
snap/bypass, and the strict pose-shift gates/shift-length condition.
The full mtcamera library's six tests (including two fade tests) passed
independent lane review; targeted clippy passes. Geometry-provider
correctness is not established by these synthetic contact fixtures.
This path is not wired into the
viewer: it needs the selected, animated
actor sphere/capsule entries and their source eligibility state. Using
render bounds or an invented generic actor radius would not reproduce it.
Next action: expose the authored actor collision entries to a per-view
camera query, preserving activation and virtual-slot64 gates, then
translate the shape contacts and pose shift. No live/original comparison was performed
for this trace. Ghidra drops part of the producer as unreachable; the
shape loop and contact accumulator were checked against disassembly.

Provider handoff: the opening dragon is a concrete first candidate.
`uEm5800` constructor **0x00a49880** calls **0x00a3e5e0**, whose instruction
at **0x00a3ecb8** sets +0x1FC0 to4. Its installed vtable **0x015d3448**
slot64 resolves to the ordinary enemy filter **0x00aaa3d0**, so the
object-adjustment bit must be removed when actor +0x2748 bit8 is set.
This is a separate field from the existing dragon step's +0x2738 actor
flags and must be supplied explicitly, not inferred from those flags.
Next provider work should preserve the phase93 selected `mIsObjAdj`
entries and pose, including their original shape kind (source kind4 must
not become camera-eligible merely because its placed volume is a capsule).
Apply the complete per-view actor gate above, implement the source
sphere/capsule contact queries, and only then connect the existing
accumulator/smoother/pose shift before the stage sweep. The existing
`Body::push_volumes` can guide placement but loses source kind and callback
state; it cannot be used directly as the complete provider. No dragon
runtime or viewer files were changed for this handoff.

### Separate collision anchor and view axis

A direct entry snapshot of **0x007de570** confirms ESI is the candidate
eye, EDI the collision anchor, and argument 1 the normalized outward view
direction. The anchor can differ from the look target. **0x007e1400**
selects the anchor from camera settings +0x224/+0x228/+0x22c; for normal
camera conditions it switches to actor model position plus settings +0x70
when pitch is below **-0.8** (or the actor-avoidance overlap succeeds).
A live Cassardis steep-pitch sample had anchor at feet +155 cm, while its
look target lay roughly a metre to the side. That 155 cm is an authored
setting, not a universal constant: base settings constructor **0x007b64e0**
initializes +0x70 to120 cm, and `rCameraParamList` **0x007d01b0** loads the
reflected settings. Runtime camera-parameter loading/selection remains
unimplemented; the follow adapter still chooses its approximate target
as anchor.

The collision helper now accepts these inputs independently. Source
**0x007de5dd..0x007de657** computes radius as `near*1.3 +
(1 - abs(dot(view_outward, normalize(eye-anchor))))*10`. The near-plane
offset uses the original view direction even after ray correction.
A synthetic perpendicular-anchor test covers both distinctions. The
private diagnostic accepts optional anchor/direction columns for exact
helper-input replay. No sampled poses are embedded.

With those inputs, a genuine Cassardis lintel-edge contact matches eye and
target to **0.000702 cm**; two clear neighboring poses match exactly.
The earlier steep-pitch sample's 13.479 cm mismatch used the wrong anchor
and does not establish a convex-query defect. The wall sample rerun after
this refactor matches to **0.001041 cm** (small floating-point differences
from the previously collinear shortcut). Eight camera-filtered tests pass
(including one existing HUD test), one private diagnostic is ignored by
default, and release build passes. These are installed-geometry collision
comparisons, not full rendered follow-camera acceptance: authored target
height, runtime anchor selection, camera lag and actor avoidance remain
incomplete.

### Authored camera parameter reader

`mtcamera::params` reads normal-camera records from the installed
`rCameraParamList` XFS, preserving array slots and non-normal/null entries.
Loader **0x007d01b0** uses the reflected object reader; **0x00420e90**
loads `param\cam_param` into `sCamera+0xcec`, and `uCameraGame::vf22`
**0x007dc8a0** selects its first parameter slot for the default processor.
The reader exposes distance, height, FOV, actor-target selector and screen
adjustment type/flags/height. No authored table is copied. Two synthetic
reader tests pass, installed-resource validation reads **60 unique paths /
167 normal records**, and strict crate clippy passes. Viewer integration
and anchor-selection translation are the next step.
