# Stamina timing and rule evidence

Evidence: DDDA.exe 2017-12-13 Steam, private unpacked analysis copy identified
in METHODOLOGY.md. Read-only Ghidra decompilation checked against disassembly,
shipped parameter values in PARAMS.md, and existing private recording
`notes/moverec/005707_sprint_open.csv`. No new game-process interaction was
performed by this task. No core simulation integration.

## Implemented and tested

`ddparam::stamina` contains small, actor-specific arithmetic helpers: scaled
continuous delta, recovery numerator/denominator and gates, recovery-debt
advance, dash rate and dash-jump amount, locomotion row adjustment, resolved
stamina addition/exhaustion/clamps, and reduced cap. These are fragments for
later integration, **not a complete stamina update**. The caller must resolve
engine gates, status/equipment modifiers, effective max and action transitions.
The helpers do not invent a timer or assume a singleton player.

Nine new regression tests cover float row truncation, separate continuous and
one-shot units, recovery suppression/override, exact and overshooting debt
boundaries, exhaustion crossing and forced recovery, upper/lower caps,
recovery-before-drain order and independent actors. Full `cargo test -p ddparam
-- --nocapture`: 21 tests pass (9 new, 3 existing synthetic, 9 existing install),
including all 1,021 copies of 61 player-parameter paths.
`cargo clippy -p ddparam --all-targets -- -D warnings` also passes. These are executable
arithmetic and data checks; they do not establish original-game branch coverage.

## Clock and order

The singleton at `0x018fa524` is **sWorkRate**: its construction in
sBbsRpgMain::ctor writes the pointer at `0x0041cd32`. Reflection
`0x004c5e10` defines `mRate[6]` at +0x20, `mMoveTimer[6]` at +0x38 and
`mPrio[6]` at +0x50. Thus +0x24 is **mRate[1]**, not an unidentified clock.
All rates initialize to 1. `sWorkRate::vf06` (`0x004c5fc0`) decrements its
rate-restoration timers by sMain.mDeltaTime and restores a rate to 1 once its
timer crosses below zero. This is separate from actor recovery debt.

Disassembly of `0x00b81450` establishes:

1. Compute recovery (`0x00b81810`), then signed continuous drain
   (`0x00b81920`), keeping both f32 results.
2. If actor +0x4ce4 is positive, subtract **cUnit.mDeltaTime (+0x14) times
   the already-computed recovery rate**. On <=0 clear +0x4ce4/+0x4ce8/+0x4cec.
3. Apply recovery, then drain, each as
   `((mRate[1] * mRate[0]) * sMain.mDeltaTime) * selected_rate` through
   `0x00b81e30`. Both rates were computed before debt clearing.
4. Update low-stamina state; on a newly exhausted transition invoke the
   additional nearby-character event path `0x00b81600` (not translated here).

Each MULSS/DIVSS/ADDSS/SUBSS rounds to f32. Continuous rates are per base
clock-frame unit; the constructor sets sMain.mFps to 30 and RECORDINGS.md
observes variable frame steps. A rate is not spent once per rendered frame.
Do not multiply one-shot amounts by this step, or use cUnit.delta in place of
the sWorkRate product for ordinary stamina application.

## Selected recovery and drain

Body class `actor+0x34c8` is selected by `0x00b81120` against PlEditWeight
boundaries. Load class `+0x34cc` is selected by `0x00b81270` using the
body-specific PlTotalWeight table and body/capacity adjustments. See PARAMS.md
for the indexed resources and shipped rows. Neither index was recorded in
005707.

Normal recovery (`0x00b81810`) is
`RecoverNormal[body].mStamina / (RecoverNormal[4+load].mFrame +
RecoverNormal[body].mFrame)`, then multiplied by actor float +0x2b44.
Motion flag words +0x854/+0x5d4/+0x494 ORed with mask 0x20000000 suppress it,
as do action IDs 0x02060000..2 with actor+0x38b0 != -1. For dash action5/0x7d,
vf190 (vtable +0x2f8) bit0x20 or sGameSys+0xb8844 bit0x400000 reloads the
base recovery despite those gates. Finally, positive +0x4ce4 replaces the
base rate with +0x4cec (zero means no recovery); +0x2b44 applies afterward.

This corrects an easy misreading of the older PARAMS.md prose: a nonzero debt
rate is an **override**, not merely permission to use the original base rate.
With positive debt and zero override the ordinary update cannot reduce that
debt. Another action must change the override; it is not a seconds countdown.

Walk/run/dash first add truncation-toward-zero of `0x00b80a10(10)` to the load
class; actor+0x26a4 bit0x80 subtracts one and clamps at zero. Vocation5 selects
Assassin tables. Walk/run set motion speed without an additional switch cost.
Dash uses `-(row.mStamina * ability_multiplier / row.mFrame)`, in that order;
ability0x6c supplies adjust parameter0x1db, otherwise multiplier1. Town flag
sGameSys+0xac720 bit0x10000000 or pawn flag actor+0x206d bit4 zeroes dash
cost; vf190 bit0x20 or manager+0xb8844 bit0x400000 suppresses it too.

`cPlActDashJumpBegin::vf05` (call at `0x00acdac5`) selects
PlStaminaDashJump **directly by load class**, negates mStamina, and calls
`0x00b81e30(delta,0,0)`: no mFrame or timing multiplier. It gates on actor
flag+0x206d bit2 and vf190 bit0x20. `cPlActEnemyClimbAttackSwordB::vf05`
(call `0x00ae8ab4`) similarly spends EnemyClimbAttack[load].mStamina once.
Other weapon/skill/climb-jump consumers are not exhaustively translated.

## Negative modifiers and effective maximum

The arithmetic helper `apply_resolved_delta` is deliberately downstream of
these guards and modifiers. `0x00b81e30` blocks negative spending for
actor+0x26a4 mask0x2, actors outside flags+0x206d mask6, several sGameSys/stage
conditions, and actor+0x4cf0. Actor+0x31ea blocks either sign. The exact gameplay
names of all those flags remain unresolved; callers must not ignore them.

Unless the call's third argument is set, `0x00b82060` handles nonpositive delta:

- Actor+0x3ef0 bit0x20 multiplies by sGameSys's adjust resource (+0xb875c)
  parameter5; bit0x200 then multiplies parameter10.
- `0x00b82360` takes the greatest u16 required-level value at equipped item
  runtime record+0x66 from slots0/1, subtracts cPlayerInfo level(+0x700), and
  checks integer-truncated actor adjust thresholds0x1b0,0x1af,0x1ae in that
  descending order to produce tier3,2,1, otherwise0.
- Actor+0x3ef0 bit0x20000 reduces the tier by one and zeroes the amount if
  the reduced tier <=0, except while climbing (+0x273c bit0x80) or actions
  5,10,11,12,123,125.
- Actor+0x3ef8 bit2 multiplies the global adjust parameter4.
- Call mode1 zeroes cost at tier0, otherwise clamps tier to3 and multiplies
  actor adjust0x1b3+tier. Other modes leave tier0 alone and use0x1b0+tier.
  The x87 multiply is stored back to f32 before return.
- A final save/actor/stage/action gate at `0x00b821f0` can apply another
  multiplier. Its gameplay meaning and recorded applicability are unresolved.

No localized status/augment names are guessed from these masks. This whole
modifier pipeline is documented but not translated as a supposedly complete
API until those inputs and integration gates are established.

Positive normal recovery is zeroed while actor+0x4ce1 is set. A forced positive
call bypasses this and can notify cPlActTired; it does not itself clear that
flag. Addition is f32, then upper clamp, then lower clamp. Exhaustion is set
only if previous stamina >0 and the resulting stamina <=0. Starting at zero
and applying another negative amount does not create a new crossing.

The cap comes from `0x00782390`, not necessarily base+level alone. Its
disassembly adds cPlayerInfo.mStaminaLv and mStaminaBase, ability-derived
amounts (IDs0x2c,0x10,0x37), equipment amounts at record+0x78, applicable
actor parameter entries of kind0x26, and cPlayerInfo+0x6a4/+0x644. The stamina
writer multiplies that effective cap by0.6 for actor+0x26a0 bit0x200.
`status_cap` accepts the already-resolved effective maximum.

cPlActTired init/update (`0x00acb8d0`, `0x00acbaf0`) clear +0x4ce1; its phase3
sets debt/initial debt to adjust0x9d times the selected base recovery and sets
+0x4cec to that base rate. Its destructor (`0x00acc3e0`) restores the base debt
rate and conditionally updates exhaustion for next actions0xa1/0xf7.
StatusRefresh (`0x00b0fc6c` inside vf06) can replace +0x4cec with adjust0xf6.
The complete tired animation/action machine is not implemented here.

## Comparison with existing recording 005707

Observed character metadata from RECORDINGS.md: female Arisen, mage/staff,
max640, base520 and level component120. Raw CSV header records marks idle0,
turn-left0.501, sprint1.043, recover5.063. Columns include time, position,
velocity, motion/frame, HP, stamina, sMain delta and FPS; **no action ID,
body/load index, sWorkRate rates, recovery gate, equipment or augments**.
CSV reads are asynchronous, so adjacent rows are not guaranteed to be one
complete engine update. The recorder source `live/session.py` confirms that
its `dt` column reads sMain+0x70, not cUnit+0x14.

| Window | Raw endpoints | Result |
|---|---|---|
| Sprint samples1.50235..2.49526 | 620.00739 ->574.70984 | -45.62100 stamina/s over 0.99291s |
| Idle samples6.02375..6.99606 | 500.26520 ->547.51703 | +48.59749/s over 0.97231s |
| Idle samples7.00048..7.99787 | 547.51703 ->595.79797 | +48.40728/s over 0.99739s |

For adjacent **changed-stamina** samples in [1.5,2.5), dividing the stamina
change by the newer row's sMain delta gives median -1.5000102: 55 of 57 ratios
are within 0.0001 of -1.5. Two outliers can combine missed/non-atomic updates;
they are not used to infer a new modifier. In [6,8), all 96 changed-sample
ratios are within 0.0001 of 1.625 (median1.6249943). This is strong evidence for
those **effective update rates**, but does not separately identify each
multiplicative factor or selected parameter row.

A compatible candidate is Dash row1 (1.5/1), RecoverNormal body1 (1.625/1),
load1 (extra frame0), neutral actor/work/status/equipment factors. It predicts
45.0 spent and 48.75 recovered per 30 clock-frame units. The -45.3/+48.9 rounded
one-second summary is consistent with sampling windows, but selecting that
candidate is **inference**, not a proof of historical body/load/augment state.
The test uses it as a shipped-data arithmetic case, not a claimed captured
state regression. Later nominal sprint windows include motions43,74,20,85;
they cannot be treated as uninterrupted sprint merely from keyboard marks.

The last decrement into482.86722 occurs between t5.07420 and5.09430; first
increase from that plateau occurs between5.68383 and5.68853. Their difference
bounds the observed hold to **0.58953..0.61433s**, subject to non-atomic reads.
The prior "about 0.5s" summary is not evidence for a fixed fifteen-frame timer.
No source for such a timer has been established. Motion/flag suppression and
recovery debt cannot be distinguished using this CSV.

## Blocker and next observation

**Historical row/modifier/gate selection is missing.** Requests on the board
ask claude-4 only for existing recordings or read-only observations during
ordinary maintainer play, explicitly excluding injected input, setters, calls,
teleport or any process writes. A new snapshot describes its own character,
not the past recording.

Capture together: actor action+0x2dd4; body/load+0x34c8/+0x34cc; recovery
factor+0x2b44; motion flag words+0x854/+0x5d4/+0x494; debt+0x4ce4/+0x4ce8/
+0x4cec; status+0x26a0/+0x26a4/+0x3ef0/+0x3ef8; actor cUnit.delta; both
sWorkRate.mRate[0..1]; sMain.delta; cPlayerInfo stamina/base/level component,
level and six equipped ability IDs; equipment required levels and applicable
adjust values. Observe sprint, stop and idle through the plateau boundary.
That will identify the selected rate and suppression branch without fitting
constants to the answer. Core integration remains blocked on those inputs.
