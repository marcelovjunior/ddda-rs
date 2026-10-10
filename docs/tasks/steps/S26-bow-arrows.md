# S26: The bow, part 2: arrows that hit

- **Branch:** `step/s26-bow-arrows` (after S25)
- **Files:** `crates/ddsim` (the shot), `crates/ddenemyrt` (arrow shells
  like the staff's, `crate::shell`), tests
- **Kind:** one feature

## Context

vf06 (0x00B17940) fires on release paths through 0x00B1A730 (a shell
spawn when the parameter record's +0x120 is 0) or a model-driven variant;
0x00B19EE0 / 0x00B1AB70 / 0x00B80650 follow (recoil motion, state reset,
stamina?). The staff's WandShot shells (`ddenemyrt::shell`, `Fight::shells`)
are the existing pattern for projectiles.

## Do

Trace the arrow: its shell parameters (speed, gravity, range, damage
source), how many per shot and the draw-time effect (a charged shot?), the
stamina cost, and the aim direction (camera vs facing). Implement the shot
in ddsim (timing) and the arrow shells in the fight so they travel and hit.

## Done when

A fight test: a Strider aims and releases at a goblin 10 m away and the
arrow hits it (damage from the traced formula), plus a miss test.

## Results — source preparation (codex-58, 2026-10-09)

Implementation is pending the reviewed S25 dependency. This checkpoint is
research only; no shot, fight test or original-game comparison is claimed.

- The release helper `0x00B1A730` is **effect dispatch**, not the arrow
  allocator: the disassembly passes actor +0x1034 and the parameter's
  signed +0x12D to actor vf44. The actual arrow scheduler is
  `0x00B1BF90`, called after the phase handler by `0x00B1AC90`.
  It traverses `shootCtrlList` in authored order, compares `waitFrame`
  against action +0x168, emits rows whose `chargeLv` matches +0x170,
  advances the cursor even for unmatched rows, and marks completion once
  it reaches the end. Group comes through EAX from shot +0x0C; index is
  shot +0x10. `0x00B1D640` allocates from actor +0x35C0 through
  `0x004AB970`.
- `0x00B1BE20` snapshots selected charge +0x1A4 into +0x170, resets
  elapsed/cursor/completion, debits the row's `stamina` (+0x60) through
  `0x00B81E30(-cost, 0, 0)`, and computes actual stamina spent times
  `staminaAtckAddRate` (+0x64). `0x00B1D640` copies that additive to shell
  +0x29B4. The subsequent vf26 debit uses adjust row 0x1BC
  (`0x00B1C2C0`); the exact interaction with S25's stamina regeneration
  remains an integration check.
- Installed `param/bow_act_param_06` row 0 has `maxChargeLv = 0`,
  stamina and attack-add rate zero, and exactly one shot row: wait 0,
  charge 0, group 0, shell 0, authored angle and spread zero. Holding this
  **basic** shot does not select a charged projectile. These are observed
  values, to be read from the install rather than embedded.
- Secondary weapon category 10 selects `param/shl_param_06` in
  `uPlayerBase::vf198`, `0x00B5BA20`. Group 0 / entry 0 is
  `cShlParamArrow`, with `mCollisionPath = collision/pl/m0006`.
  `uShlSpeed::vf05` (`0x00C12240`) initializes speed/gravity;
  `0x00C12F70` moves using current values, applies gravity once
  `gravityDist <= travelled`, then advances/clamps speed and gravity.
  Installed values are speed 200 -> 100 cm/frame, acceleration -5;
  initial vertical component +5, acceleration -0.5, floor -100;
  gravity distance 0 and death distance 9000 cm. Auto-lock and floating
  movement are false. Distance death and hit/world death bits are
  decoded by `0x00C12D60`; distance is multiplied by the runtime augment
  0x46 result installed at shell +0x2A04 (`0x00B1DA6A..0x00B1DA87`).
- `uShlSpeed::vf112` (`0x00C13290`) selects collision phase 0 below
  `colTimer_0`, phase 1 at the threshold, and phase 2 at `colTimer_1`.
  Basic arrow sequences are 1, 2, 3 with thresholds 1000 / 2500 cm;
  `collision/pl/m0006` maps them to attack rows 1, 2, 3. These are
  radius-10 cm spheres, collision kind 5. The paired installed
  `collision/pl/m0006_00` has the distance-dependent attack rows; do not
  substitute dagger or staff collision values.
- Basic `shootType = 0` does **not** spawn at the camera. The origin is
  authored joint 0 of the bow model at actor +0x3568, looked up by
  `0x00421ED0` (byte remap +0x368, joint records +0x364, stride 0xA0).
  `0x00B1D010` obtains the camera aim target (or lock-on/raycast result),
  and `0x00B1D640` normalizes target minus bow origin. `shootType = 2`
  has the separate camera-origin branch; type 1 uses the actor matrix.
  `0x00BD6D20` stores the supplied origin and direction on the shell.
- Zero authored spread is insufficient to omit accuracy: `0x00B1DCB0`
  additionally applies a random cone `(vf18 + action +0x154) * 4` degrees,
  except augment 0x48 / secondary category 0x0C. `vf18` is the zero getter
  `0x00B16AE0`; +0x154 is the smoothed accuracy, whose target +0x14C is
  selected from `accurateWait/Walk/Run/Jump/Fall` by `0x00B17DA0` and
  updated at `0x00B1E480` through `0x007BC800`. Initial smoothed value is
  1.0 (`0x00B16FE0`); stationary target is 0.25. The smoothing and cone
  smoothing still needs integration; cone rotation is now covered by the
  original-helper synthetic regressions described below.
- Damage enters the existing `ddcombat::damage::player_attack`
  (`0x0088AA20`) with the **secondary bow equipment slot**, the installed
  collision row and goblin region/defence. `uShlBase::vf111`
  (`0x00BD92E0`) and `uShlArrow::vf111` (`0x00BCF1E0`) project shell
  additions and special-arrow overrides. The basic shot's **stamina**
  additive is zero, but the normal-arrow selector uses the embedded
  override-table entry 120 at `0x015F76B8`. `0x00BCF1E0` adds installed
  rows 120/121/122 to the current phase's attack, preserves shrink when
  the extra type is zero and blow when the extra type is three, and
  replaces six status fields. At `repelledDist`, it clears raw attack,
  stagger and knockdown values. This occurs before player valuation;
  simply using collision row 1 alone would omit the normal-arrow bonus.
  The existing Strider kit only supplies dagger equipment and must
  gain the installed starting shortbow slot (item 0x1F8, sourced existing
  `0x00673740` starting equipment selection).

Next action: integrate after S25 review using an ordered per-seat shot
queue, explicit bow-joint origin/camera-target/random-sample inputs,
runtime arrow parameters and the secondary attacker slot. Preserve
previous-frame hit-pass ordering and validate the 10 m hit, a deliberate
miss and two independent shooters. Accuracy smoothing, terrain contacts and augment defaults are
still source checks, not silently assumed completed systems.

### Independent arrow checkpoint

With codex-1's narrow approval (board #3985), `ddenemyrt::arrow` now
provides an installed basic-arrow reader, per-owner ballistic state,
distance collision selection, accepted actor/world contact death bits,
normal-arrow damage projection and explicit bow-origin/aim-target/RNG
inputs. It does not select a nearest enemy or invent a bow offset.
Exact vertical aim without the steady augment returns unsupported;
the original parallel-up basis fallback is not yet translated.

Validation: full `cargo test -p ddenemyrt` passed **91 tests, 1 ignored**
before the supplemental projection, then the final focused **5 unit +
1 installed** tests passed. Three oblique-aim cone fixtures execute
original `0x00B1DCB0` with original basis construction and RNG word
mapping in a private Unicorn harness (only sin/cos calls supplied by the
harness); the Rust results agree within 2e-7 per component. The installed
test verifies the arrow's resource 6 collision, phase falloff,
supplemental attack rows, trajectory order and death on accepted contact.
No original live-game comparison or Sim/Fight acceptance is claimed.

S25 is now reviewed and merged into this branch; the narrow DDSIM shot
claim is active. S29's `fight.rs` ownership still gates Fight integration.
Terrain contacts and actual bow-joint posing remain caller boundaries.

The fast-flight geometry is translated from `0x00BD8E40`: each moving
authored sphere becomes a capsule from its current world center to that
center minus shell displacement (`0x00BCC870`). Original-function
emulation confirmed both endpoints for a synthetic trajectory. The final
focused **5 unit + 2 installed tests** pass, including a 10 m contact
and deliberate miss against runtime goblin hurt volumes. This verifies
projectile geometry, not the remaining normal-input Fight/damage gate.


### Normal-input scheduler checkpoint

After S25 was reviewed and merged, the approved DDSIM slice now executes a
stationary basic shot from `Input.light`'s rising edge while bow mode is
held and Ready. Releasing bow mode exits without firing. `BowShotParam`
reads the installed row zero and its ordered `shootCtrlList` through
`ddenemyrt::arrow::load_shot_param`; bulk records are not embedded.

The per-seat `BowShotState` translates `0x00B19540`, `0x00B1C2C0`,
`0x00B1BE20` and `0x00B1BF90`: shot-wait countdown, upper reload motion,
actual stamina spent, selected-charge snapshot, inclusive row timing,
charge-mismatch consumption, cursor completion and post-emission elapsed
advancement. Lower selection still precedes the shot trigger/upper phase
handler. Upper completion returns to Ready; the charge reset boundary is
strictly greater than `chargeBgnFrameShot`. Mode-one adjust 0x1BC costs
use `Player.secondary_weapon_level_gap`, independently of primary daggers.

`BowAccuracy` translates `0x00B1E480` -> `0x007BC800` with scalar vtable
`0x01599174`. Initial value 1 and stiffness 0.15 come from the constructor;
smoothed stationary accuracy updates after emission. Eight synthetic
variable-step points from original-function Unicorn execution match the
Rust filter within 2e-7. Runtime `accurateWait` remains data-driven.

`Player.bow_shots` is an undrained host-only event queue. `BowState` and
its existing wire representation are unchanged. `Arrow::from_shot`
requires a supplied bow-model Pose with actual joint zero, resolved target,
RNG words and range augment result. Missing joint or unsupported shot
returns an error. `Arrow::fighter` accepts an explicitly secondary-valued
shooter for the ordinary hit pass. These are integration boundaries,
not a fabricated camera/feet spawn or nearest-enemy aim selection.

Validation: full `cargo test -p ddsim` **76 passed** (63 unit, 7 existing
bow, 6 new shot tests); installed `cargo test -p ddenemyrt --test arrows`
**4 passed** in 7.19 s. Normal-input tests cover ordered/mismatched rows,
delay, hold without repeat, independent seats, release without firing,
actual spent stamina, strict charge reset, secondary level tier and
missing motion. The installed gates verify runtime shot parsing and
joint-required adaptation as well as the earlier trajectory/contact tests.

Still open: root's S29 `fight.rs` scope handoff, canonical posed bow model
and upper-layer/body attachment provider, secondary bow attacker hookup,
normal Sim/Fight 10 m damage + miss acceptance, world/terrain contacts and
live-original comparison. Draw/Shot early command buffering, charging,
moving/air shots, special arrows, nonzero authored spread and the exact
vertical basis fallback remain explicit unsupported paths. This is a
tested checkpoint, not completion of the brief.
