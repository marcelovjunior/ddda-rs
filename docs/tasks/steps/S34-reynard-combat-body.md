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
  policy. Host quest observations and viewer gauge are not wired yet.

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

Remaining limits: the hit minimum inherits the existing pass's untraced
attacker +0x14 assumption of 1; script gating of damage-over-time is not
traced. NPC death-bank behavior must be sourced separately from HP zero;
retiring an unloaded owner must never fabricate a death. No host
NpcCheck/NpcDieCheck or viewer gauge acceptance is claimed by this
checkpoint. The headless installed body/Fight combat acceptance passes;
full rescue/original recording comparison remains open.

Next action: bind the reviewed S32 host lifetime/motion to the Fight NPC
body, trace death-bank writes, and expose real HP to quest observations;
then wire the sourced viewer gauge.
