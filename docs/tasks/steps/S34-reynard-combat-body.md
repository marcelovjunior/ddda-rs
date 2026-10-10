# S34: Reynard as a combat body, with his HP gauge

- **Branch:** `step/s34-npc-combat-body`
- **Files:** `crates/ddenemyrt` (an NPC fighter), `crates/ddhost` (binding
  NPC304 to it, NpcCheck/NpcDieCheck observations), `crates/ddview/src/hud.rs`
  (the `npc_gauge` layout), tests
- **Kind:** research + one feature
- **After:** S32 (Reynard's authored motions) for the viewer part; the
  research and the host part can start now

## Context

In q0002 the goblins of group 23 attack Reynard (NPC304) on the road. The
quest's event1 and main states read his HP and death (NpcCheck 0x005c0420 /
0x005c0820, NpcDieCheck 0x005c09f0; docs/tasks/q0002-roadside.md), and his
script turns his gauge on (SetGaugeDisp, S23). Today he has no HP, cannot be
hit, the goblins only target Arisen seats (`Fight::enemies` builds targets
from `self.arisen`), and nothing draws his gauge. The head's route demo
already reaches q2:20 (rescue success) because the goblins die before they
can matter: with a real Reynard the rescue can also fail.

## Known (head, 2026-10-10)

- `uGUINpcGauge` (vtable 0x01584950, ctor 0x0069F360, 0x2A0 bytes) is
  created by 0x00BB8C10 into uNpc +0x6FD8 with +0x270 = the NPC; layout
  `id\cockpit\npc_gauge` (vf58 0x0069F540).
- vf09 0x0069F5A0, the fill: ratio = HP (uNpc[0x746] -> +8, when
  uNpc[0x743] != 0) / max (uNpc vf 0x148); hidden when ratio <= 0 or uNpc
  vf 0x1D4 is true; width = ratio x +0x274 (the bar node's width, read in
  vf05 0x0069F410 from node +0xD0), at least 3 px.
- vf11 0x0069FB00, the place: an NPC matrix (vf 0x58 with -1) times the
  local offset uNpc +0x2B70..+0x2B78, minus the body's +0xE0, plus
  0x0067C870; projected by 0x0069FA10; 40 px up. Hidden during events
  (0x018FA4CC +0x2E20) or when uNpc +0x2F86 is 0.
- vf16 0x0069FD70 drops the NPC when its unit state is not 1/2.

## Do

1. Trace where NPC304's HP, max HP, defence and hurt volumes come from
   (uNpc +0x1D18 holder at [0x746], vf 0x148, the NPC's collision), and
   whether goblins pick NPCs as targets (the goblin sensor/target choice;
   Invincible/NoDamage from S23 gate damage).
2. Give the fight an NPC fighter for a bound NPC (owner id range like
   goblins), hit by enemies and blocked by NoDamage/Invincible; let goblins
   target it where the source says they do.
3. Feed NpcCheck/NpcDieCheck from it, so q0002's success/failure branches
   read real HP.
4. Draw `npc_gauge` from vf09/vf11 in the viewer (like `Binding::Enemy` in
   hud.rs), driven by the SetGaugeDisp request.

## Done when

A headless installed test shows goblins damaging Reynard (and NoDamage
holding while set), NpcCheck reading his HP, and the viewer showing his
gauge after SetGaugeDisp. The Result lists each value's source.

## Results (body/Fight checkpoint; acceptance incomplete)

Implemented in this checkpoint:

- An independent `ddenemyrt::npc_combat` module and `Victim::Npc`, using
  `PlayerDefender` mitigation and an explicit main `RegionPool`. NPCs keep
  distinct fighter owners; they are not player seats.
- Ordinary NPC flag conversion, HP-only NoDamage, and Invincible's
  valuation gate while still recording contact history. NPC status
  build-up and stagger/knockdown run independently of the HP gate.
- Runtime reading of `param/pl/other/PlDefendParam` row zero and the
  source gauge fill calculation.
- Installed `NpcBody` reads NPC304's Look, skeleton, HumanEdit and
  BodyShape edits, NPC common/damage/authored motion banks and collision
  events. `NpcFighter` binds a distinct owner and generation in Fight;
  host motion/frame/placement remain explicit inputs. NPCs participate
  in hit/status/history passes, and unload retirement removes the owner
  without death or a kill award. Pose errors clear stale hurt volumes.
- `rAISensorExt` SNR2 records are read at runtime. The initial em0100
  group1 sphere admits NPC subtype4; a 16-target native distance-hate
  subset preserves strict ties and influences after sensor exit. NPCs
  stay out of the FSM player roster. Dynamic sensor-mask writers,
  non-spherical geometry, FOV/LOS and other hate contributions/overrides
  remain unported. Unsupported active geometry reports an error and
  selects no target. The source fallback to the primary Arisen is
  generalized to the first live registered seat as an explicit co-op
  policy.
- Host binding and an `OpeningWorld` HP adapter now connect one validated
  S32 lifetime to the Fight body, preserving HP across idempotent bind.
  Canonical motion/frame/flags are synchronized after the actor step;
  missing motion clears hurts with a diagnostic. The story phase reads
  last-frame HP through `NpcIntel.hp_positive`, the NpcCheck input.
  Typed `DeathObservation` stages update NpcDieCheck's apparent/permanent
  banks only for a dead current lifetime. Actual death-action playback,
  automatic stage observations and viewer gauge remain unfinished.
- The opening adapter now binds NPC304 combat only after successful model
  materialization and a real S32 canonical motion for the same lifetime.
  Its installed ordinary body is cached; the host allocates the fighter
  and supplies the sourced pool. Admission alone does not create combat,
  and repeat calls preserve the combat owner/HP. Other NPCs, specialized
  types and common99 bodies remain outside this narrow opening adapter.

Tested: seven focused regressions passed with actual exit 0 under the
exclusive core target lease, after a narrow `ddenemyrt` cleanup removed
stale shared-cache code. Compile 5.45 s, tests 2.96 s. The installed test
reads the native NPC main pool and confirms that `collision/npc/m0000`
has damage groups of kind 3 and main-region indices. Synthetic overlap
fixtures separately check NoDamage's status/reaction/history behavior,
Invincible's recorded contacts, and independent owner HP floor/death.
The later body/Fight checkpoint additionally passes two installed pose
tests (all four q0002 motions, actual hurt nodes and independent owner /
unit placement), three sensor tests (native tie/memory/death gates,
unsupported active geometry, and installed natural goblin combat),
three goblin unit regressions, nine installed Arisen attack regressions,
and three status regressions. All ran under the actual exclusive core
lease and exited 0. The natural combat test runs 360 frames for each of
ordinary, NoDamage and Invincible: the goblin selects Reynard, actual
posed attack/hurt volumes damage ordinary HP, NoDamage holds HP while
events remain, and Invincible holds HP without valuation events while
contact history is observed. Final sensor compile 1.88 s / test 7.99 s;
body tests 3.63 s, attacks 5.01 s, status 5.56 s. Two intermediate test
failures corrected the archive class name to `rAISensorExt` and changed
the history assertion to observe contacts throughout the attack cycle
(release correctly clears history before the next attack).

Later host checkpoint: one installed HP/lifetime/death-observation test
passed (compile 11.55 s, test 10.66 s), followed by NPC sensor3 (5.39 s),
host goblin7 (9.83 s), NPC motion6 (7.81 s) and NPC placements9 (2.68 s),
all actual exit0 under a separately acquired exclusive core lease. This
checkpoint merges reviewed main322456e and preserves `Goblin::placed`
and PlacementRates. The ordinary natural-combat test stops after the
first observed real hit; protected cases keep the 360-frame bound.
The host death-bank test uses an explicit zero-HP fixture and explicit
death-stage observations. It verifies the adapter, not actual autonomous
death animation, timer or original rescue failure.

Compared against the original: source tracing only; no live original-game
recording or complete original rescue comparison yet.

### Value and behavior sources

- HP/max **1000**: `uNpcIntel::ctor` **0x00BBBFA0**, offsets +0x3D8/+0x3DC
  (`0x447A0000`). **0x00BABB00** copies these into body cNpcInfo
  +0x29C/+0x2A0. The ordinary party-slot -1 branch of **0x00BBE910**
  reads the body maximum, calls **0x00782040**, and initializes the intel
  pool. Reynard is an ordinary NPC, not a recruited pawn or human enemy;
  those separate initialization branches are outside this constructor.
- Intrinsic physical/magick defence **1** and empty stat addition arrays:
  **0x0077FA50**. `uPlayerBase::vf68` **0x00B7AD90** is inherited by uNpc;
  **0x00B87450** skips the party equipment sum for NPC class bit 8 and
  **0x00B87940** reads the cleared additions. Enemy subtraction would
  therefore be the wrong mitigation for Reynard.
- NPC pool: shared manager **0x0047E46D** loads `rPlDefendParam`;
  `uPlayerBase::vf196` **0x00B5F0A0**, ordinary branch
  **0x00B5F468..0x00B5F508**, uses row zero's `mShrinkMax`,
  `mShrinkSpeed`, `mBlowMax`, `mBlowSpeed`. **0x00770A90** constructs the
  pool. Embedded inputs are damage/hit-stop **1**, surface **1**, attr
  **0**; resets come from `param/game` entries **0x1BF/0x1C0**. Writes
  **0x00B5F6BC..0x00B5F744** replace thresholds 0..17 with **1000**;
  thresholds 18/19 retain **999/999.9** from **0x00776BA0**.
- Body collision resource `collision/npc/m0000`: NPC constructor
  **0x00BAB3A0**, table **0x014EAC20**, and loader **0x00B5F0A0**.
  `NpcBody` now poses the installed canonical motion, skeleton and
  body-edit scale. Look/base-model selection is **0x0083FA60**; the
  reviewed S32 bank registration is **0x00BB6060**, authored bank12
  fallback **0x0080D750**. Skeleton sampling and constraints use
  **0x01051B60** / **0x00860AC0** and existing source-marked ddlook
  HumanEdit/BodyShape/face constraint helpers.
- Canonical flag pointer: `uNpc::vf180` **0x00BB4730** uses intel +0x5C0
  when present. `uNpc::vf93` **0x00BB21E0** maps NoDamage `0x08000000`
  to inline collision +0xCFC bit **1**, NoDeath `0x04000000` to **2**,
  Invincible `0x10000000` to **4**, SuperArmor `0x02000000` to **8**.
  The same source tests unresolved script bit `0x00080000`, giving **9**.
- Invincible: **0x0076B7E0**, test at **0x0076BD76**, suppresses
  valuation/application. Contact recording **0x0076EB60** runs afterward.
  NoDamage instead gates HP in **0x00776F50**, preserving the hit and its
  status processing. The NPC victim reuses **0x0076DB10** region scaling,
  break, HP and frozen-reaction helpers after player mitigation.
- Gauge: **0x0069F5A0** uses HP/max, requires absolute max at least
  `f32::EPSILON`, hides nonpositive ratios/dead bodies, and sets the
  authored bar width times ratio with minimum **3 px**. It does not
  clamp ratios above one. Full placement/visibility still awaits wiring.
- NPC target eligibility: `uNpc::vf115` **0x00BAF2D0** calls
  **0x00889110** with subtype **4**, stored in sensor target +0x44.
  Target constructor **0x005B0790** initializes active bit 2 and class
  mask 7. `uEm0100` **0x008D2930** queries **0x005AFC50** with class mask
  **2**, subtype mask **0x16** (including NPC subtype 4). Candidate
  Geometry **0x005AF1F0** and resource transformation **0x00CA8EC0**
  implement the installed initial simple sphere branch. Resource reader
  **0x00CA8D90** uses 16-byte header / 80-byte records; **0x005B0160**
  enables groups, initialized to mask1 by uEm0100 vf115. Distance
  influence **0x00AB5A50** uses strict distance below **3000 cm**, score
  `abs((distance / 3000 - 1) * 300)` capped at **300**, and 16 target
  records. **0x00AB52E0** preserves the first strict maximum; the
  implemented ordinary distance subset retains influences until owner
  retirement and rejects dead candidates. Other influence types,
  grabbed/cached targets and dynamic sensor groups remain explicit limits.
- Death bank timing: `cNpcActDead::vf06` **0x00B98AE0** phase1 waits for
  the actual layer end before **0x00BB8B10 -> 0x00437670** writes the
  apparent bank. Phase2 respects canonical bit **0x00400000** and waits
  intel+0x4E0; phase3 fades intel+0x484 using unit step / **30**, then
  **0x00BB57E0 -> 0x00446460** writes both banks. Constructor
  **0x00BBBFA0** initializes that wait to **60 x sMain.mFps**;
  **0x00BBCB80** (uNpcIntel vf08) decrements it after apparent death.
  Body destruction **0x00BB3D70** only marks apparent when HP is not
  positive; ordinary alive unloading does not establish a death.

Remaining limits: the hit minimum inherits the existing pass's untraced
attacker +0x14 assumption of 1; script gating of damage-over-time is not
traced. NPC death-bank behavior must be sourced separately from HP zero;
retiring an unloaded owner must never fabricate a death. No host
autonomous NPC death-action or viewer gauge acceptance is claimed by
this checkpoint. The headless installed body/Fight combat and HP adapter
tests pass; full rescue/original recording comparison remains open.
The target adapter's `alive` input currently supplies only HP positivity;
native actor flags+0x2360/+0x2364/+0x2368, redirect actors, augment hate,
and further influence types remain unsupported. Root travel, target yaw
and ground integration are still S32 actor-system limits.

Scale/gauge follow-up: **0x0081DB50** writes HumanEdit entry0 to
uCoord+0x60/64/68. The posed joints already included that scale once,
but the initial model-matrix and hurt-radius paths used placement scale
alone. Corrected both to include HumanEdit without applying it twice
to joints. Two installed pose tests and three sensor/contact tests pass
after correction (actual exit0; compile6.49 s, pose4.72 s, sensor6.29 s).
The placement multiplier remains an explicit adapter transform; dynamic
actor overrides and original LOT/HumanEdit precedence beyond NPC304's
unit LOT scale remain unverified.

Compared generic gauge geometry against original NPC7 and NPC4 by
read-only oracle on PID23408, with save backup before the probe; no
writes, keys, function calls or travel. Native **0x00FFA620** unions
posed joint spheres using runtime MOD bone record+4 (`Bone.length`) and
the largest joint axis length when model+0x108 bit0x40 is set. The
active-joint filter +0x96 bit1 is essential: 42 of 224 joints in these
NPCs. The translated scaled/active sphere-union reproduced world center
and extents with maximum errors **0.000038 cm** (NPC7) and
**0.000320 cm** (NPC4). NPC1 moved between snapshots, so its residual
does not establish a comparison. Raw matrices/bone data remain private
in `E:/ddda-re/notes/steps/S34/live-native-bone-bounds.json`.
Reynard was absent; this is not a NPC304 gauge/rescue comparison.
The generic gauge anchor is **0x0067C970** mode0 center+extentY;
**0x0069FB00** adds transformed local offset minus actor position,
then camera pull **0x0067C870** and 40px upward projection. The pull
radius from **0x005371B0** is the scenery capsule radius at inline
cObjCollision+0x9D0, not a model AABB radius. Active-joint-mask source
and viewer materialization remain the next source inputs.

Bounds follow-up: `NpcBody::bounds` now reads MOD bone radii and four
native detail-mask rows from the player's install/executable at runtime;
no active joint list or bulk mask is copied. `uPlayerBase::vf100`
**0x00B531B0** reads **0x014F2AF0 + detail x 256**, selected by
actor+0x5720; **0x00B8D980** writes the active flags. Detail3 matches
all 42 active joint IDs in each of the private NPC1/7/4 live samples.
The detail row remains an explicit adapter input: dynamic LOD selection
and constraint overrides are not implemented. The bounds port covers
the scaled active-joint branch of **0x00FFA620**, including the fourth
component in each axis norm; the unscaled branch's parent spheres remain
outside this adapter. `NpcBounds::gauge_anchor` translates
**0x0069FB00 / 0x0067C970 / 0x0067C870**, taking each view's camera,
local offset and actual scenery capsule radius explicitly. It does not
substitute model extents for that radius. Two pure regressions passed
(compile3.67s/test0.00s), and the two installed Reynard pose tests passed
with bounds translation/scaling assertions (compile7.48s/test4.80s),
all actual exit0 under a valid exclusive core lease, then released.
Viewer gauge placement and original NPC304 comparison remain open.
Bounds review follow-up: the mask key is the MOD `anim_map_index`, not
the bone array index: **0x00B8D980** passes joint+0x93 to vf100, and
uModel vf23 copies MOD record byte0 into that field. Matrix/radius arrays
stay in parallel MOD order. Invalid negative radii or any nonfinite
matrix input now fail explicitly before bounds calculation. The two
pure tests and two installed pose tests passed again, actual exit0
(unit compile6.48s/test0.00s, installed compile3.39s/test4.61s); core
was actually released after process completion.

Opening materialization validation: the existing installed field-intel
test now also checks admission without a body, a real q0002 NPC graph and
canonical motion, actual combat owner/HP/hurts, idempotent binding and
stage-unload retirement. This is an explicit installed graph/body-ready
fixture, not a full route or screenshot comparison. The focused viewer
test passed actual exit0 under the valid exclusive M5 lease, using the
existing release target (compile31.75s/test7.94s), then the lease was
released. An initial accidental default-debug command was stopped with
actual exit1 before this release check; no new target was created.

Next action: compose actual death-action playback/stages and finish the
scenery-radius input before wiring the viewer gauge. Validate the actual
ordinary rescue route with the materialized combat body after review.
