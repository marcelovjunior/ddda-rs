# Frame-rate independence (what an unlocked frame rate changes)

Brief: [L30](tasks/L30-frame-rate-audit.md). Static audit of DDDA.exe
(2017-12-13 Steam) through Ghidra and the lifted sources. Nothing here
was measured on the running game unless marked [live].

**Classes used below:**
- **dt-correct**: scales with the step, and the result does not depend on
  how time is cut into frames.
- **frame-counted**: a fixed amount per frame, so its rate scales with
  the frame rate.
- **dt-approximate**: uses the step, but the result still depends on the
  frame rate (Euler integration, averaged rates, per-call accumulation).
- **locked**: runs on its own fixed clock.

## 1. The clock and its unit

`sMain::vf06` (0x00DBFA90) produces `mDeltaTime` (+0x70) in units of
`1/mFps` seconds (FRAME.md 1a).

**`mFps` is 30 in the running game, not 60.**
- `sMain`'s constructor (0x00DBE810) writes 60.0.
- `sBbsRpgMain`'s constructor (0x0041B400) overwrites it: at 0x0041CEC9
  it stores 30.0 (`[0x014EA160]`) into `*0x018D0B28 + 0x38`.
- The live read agrees: `mFps` = 30.0 (RECORDINGS.md) [live].
- So one unit of `mDeltaTime` is 1/30 s. The property names say the same,
  e.g. sRecognition's "check interval (frames (30fps))".
- At 60, 120, 144 and 240 fps, `mDeltaTime` is about 0.5, 0.25, 0.208 and
  0.125.
- FRAME.md section 1a states 60 (the base-class default). This should be
  corrected there.

**Related fields:**
- `sMain+0x7c` is `30 / max(1, mActualFps)`. FRAME.md calls the source
  "another singleton", but `*0x018D0B28` is `sMain` itself, so it reads
  its own `mActualFps` (+0x44), a one-second window average (0x00DBE6E0).
- `mDeltaTimeLimite` is 2.0 in normal play (FRAME.md step 4). Below
  15 fps the game slows down instead of stepping further. That does not
  matter for high rates.

**Leads, not evidence:**
- Capcom's PC FAQ says the PC release caps the rate at 150 fps "to avoid
  potential game-breaking issues" such as physics interaction and
  rendering errors.
- `mMaxFps` read 150 live (RECORDINGS.md), and players reported 60-150.

The findings below are the code-level candidates for those issues.

## 2. Method

**1. Direct readers of the clock.** A scan of the lifted sources
(`crates/ddlift`, one basic block per arm) finds every function that
loads `*0x018D0B28` and reads one of its timing fields:
- 296 functions in all;
- 192 read `mDeltaTime`;
- 66 read `mFps`;
- 49 read the step counter `+0x68` (pause gates only);
- 8 read `mTimer`;
- 5 read `+0x7c`.

124 of them are named through the DTI labels (class::slot).

**2. The per-unit step.** Most gameplay reads not `sMain` but the
unit's `cUnit.mDeltaTime` (+0x14) = line rate × `sMain.mDeltaTime`
(FRAME.md 3). Those were followed system by system from the update
functions in FRAME.md 5.

**3. Frame-rate constants.** 687 lifted functions load an f32 constant
equal to 30, 60, 1/30 or 1/60. These are candidates; most are unit
conversions (section 5).

**4. Frame counters.** These were scanned the same way:
- `sEffect+0x74` (23 readers);
- `sMain+0x1C274` (no direct reader found).

The scan scripts and raw lists are private (`C:\ddda-targets\L30`, not
committed).

## 3. The systems

| System | Where | Class | Evidence | At 120 / 144 / 240 fps (against 60) |
|---|---|---|---|---|
| Unit step | `sUnit::vf09` 0x00DB7350 | dt-correct | `cUnit.mDeltaTime` = line rate × `mDeltaTime` (0x00DB7384) | none |
| Hit-stop, slow motion | `sWorkRate::vf06` 0x004C5FC0 | dt-correct | six rate timers −= `mDeltaTime` | none (durations end on a frame boundary, so they get finer) |
| Game clock, weather | `sGameSys::vf06` 0x0043CEB0, `0x0043F2C0`, `0x004C0C70` | dt-correct | step = `mDeltaTime / mFps` s (WEATHER_CLOCK.md) | none |
| Status timers | `cStatus::update` 0x00836C70 | dt-correct | `timer −= rate × speeds` with the scaled step (STATUS.md) | none |
| Status Frozen (bit 0x800) and Petrify (0x1000) movement | integration 0x0084CCC0 | **frame-counted** | each frame: horizontal velocity × max(0, 1 − `+0x2864`) (Frozen); velocity × `+0x27B0` (Petrify); no dt | with a velocity that persists between frames, braking ×2 / ×2.4 / ×4 per second |
| Character integration | 0x0084CCC0 | dt-approximate | `pos += v·dt`, then `v += a·dt` (explicit Euler; push and wind × dt) | jump apex 95.0 cm at 60, 92.6 at 120, 92.2 at 144, 91.4 at 240 (normal jump: SpeedY 19, gravity −2; apex = v0²/2g + v0·h/2) |
| Air control | 0x00AC51D0 | dt-correct | speed −= DampingZ × step (linear); steering × step | none |
| Turning (yaw rotator) | 0x0084CC00 → 0x00797470 | dt-correct | rotator advanced by the unit step; enemy turn rates set per base frame (deg/s × 1/30, e.g. `cEm0100ActWalk::vf06` 0x008D5970) | none |
| Actions | `cPlAct*::vf06`, action time += step (FRAME.md 5.2) | dt-correct (sampled) | movement values per base frame × step (MOVEMENT.md) | none expected; not every action checked |
| Animation and blending | 0x01060730 | dt-correct | frame += `mSpeed` × step; the blend counter −= step and the weight is taken from remaining / total (ANIMATION.md) | none |
| AI-sense timer ("every 150 frames", FRAME.md 5.3) | `uCharacterBase::vf10` 0x0084C6A0 | dt-correct, quantised | `+0x2EC0 += unit step`; at ≥ 150 base frames (5 s) it calls `0x00855DC0` and resets to 0 | none |
| AI think interval | `0x0059E320` | dt-correct, quantised | timer += unit step, fires at the interval, resets to 0 (remainder dropped) | intervals drift less: the overshoot shrinks with the step |
| AI timers | AI.md | dt-correct | timers gain `delta / fps` | none |
| Enemy recognition (sRecognition) | `sRecognition::vf06` 0x00487B00 | **dt-approximate (bug)** | section 4.1 | "recognised" after 8 / 9.6 / 16 s instead of 2 s; forgotten after 40 / 48 / 80 s instead of 10 |
| Combat DP/BP recovery, element decay | 0x007781A0 | dt-correct | all × the owner's step (1.0 only without an owner) | none |
| Stamina | 0x00B81450 (STAMINA.md) | dt-correct | × unit or scaled step | none |
| Cameras: game camera springs | `cCamImp*` (e.g. `cCamImpNml::vf11` 0x007C8880), springs 0x007BC400 / 0x007BC800 | dt-approximate | step = `sWorkRate` rate × `mDeltaTime`, integrated in substeps of `min(rest, 1.1)` base frames (semi-explicit Euler) | small: the spring response converges to the continuous one (slightly less overshoot and lag than at 60) |
| Cameras: angle and position limits | `0x007B5030` (from `cCamImpAnimTar`, `cCamImpEmClimb`, `cCamImpLock`, `cCamImpMotTar`, `cCamImpNml`) | dt-correct | the target is first clamped to at most `param_2` from the current value (a maximum lag, not a per-frame step), then the spring `0x007BC800` runs with the step; offset ramps are `± rate × mDeltaTime × 4 or 8` (`cCamImpNml::vf11` 0x007C8880) | none beyond the springs |
| Cameras: event camera | `uEventCamera::vf08` 0x007E59A0 | dt-correct | unit step × rate factors | none |
| Cloth (capes, skirts) | `uSimSoftBody::vf11` 0x00F20640 | **locked, with a dropped remainder (bug)** | section 4.2 | 60 fps 60 steps/s; 120 60; **144 48 (cloth at 80 %)**; 240 60; 165 55; 100 50; 75 37.5. Near 60/120/240 the rate halves whenever rounding misses the step |
| Hair, small chains | `uCnsTinyChain`, 0x00E99010 | locked (correct) | seconds accumulator (frame step capped 0.05 s), fixed substeps while acc > step, `acc −= step` (remainder kept) | none |
| Constraint weights | `uConstraint::vf08` 0x0112EE40 | dt-correct | weight moves by speed × step | none |
| Long chains (`uChain`) | `uChain::vf09` 0x00F0C160, setup 0x00F07270 | dt-approximate | step = unit step (clamped to 2.0) × time scale `+0xCC` → `+0x78`; split into `+0x88` substeps per frame (`+0x7C` = step / count) | the substep shrinks with the rate while the substep count per frame stays fixed, so the solver runs ×2 / ×2.4 / ×4 as many iterations per second: stiffer constraints, and more damping if the iterations damp (the iteration loop is not traced) |
| Chain constraint (`uCnsChain`) | `uCnsChain::vf08` 0x00E42720, 0x00E31A00 | no time input found | vf08 rebuilds and collides the chain (0x011918B0 is a collision query) and queues a job whose callback (0x00E31C50) only records results; none of its slots reads a step | — |
| Grass, wind | `uGrass::vf08` 0x011BDBA0, `uGrassWind*` | dt-correct | time += `mDeltaTime / mFps` | none |
| Effects: playback | `sEffect::vf06` 0x00FB64D0; `mEffectFps` (`sEffect+0x34`, from `sMain.mFps`); per-effect `mBaseFps` (`uEffect`, `uEffect2D`, `uSimpleEffect`, `rEffectList`) | dt-correct (as far as traced) | effects run as units with the unit step; `uBaseEffect::vf28` 0x00FE1630 copies `mFps` | none expected |
| Effects: noise "time source 4" | 0x011E48E0, 0x011E7040, 0x00E6C260 and 20 more | **frame-counted** | the random or noise input is `sEffect+0x74`, incremented once per frame (0x00FB64D0) | effects in that mode flicker or jitter ×2 / ×2.4 / ×4 as often |
| GUI animation, fades | `uGUI::vf09` 0x00E0CE50, `uFade::vf08` 0x007B3540, `uGUICmcMessage`, `uGUIPrologue`, `uSimpleLoading`, `sIdCockpit` | dt-correct | speed × unit step / `mFps`; fades × `mDeltaTime` | none |
| End credits, two timers | `uGUIEndCredit2` (0x0052C3E0), `uGUIEndCreditDDN` (0x00401AA0), 0x004F9FE0, 0x00C51FE0 | dt-approximate | step = `sMain+0x7c` = 30 / `mActualFps` (one-second average), or `mFps / mActualFps` | right on average; off for about a second after a rate change |
| Input | `sQlocInput::vf06` 0x00409EE0, `sKeyboard`, `sPad`, `sMouse` | locked (real time) | repeat and hold times on `mTimer` (ms) | none |
| Sound | `sSound::vf06` 0x00DDE030 | dt-correct | step = 1000 / `mFps` × `mDeltaTime` ms | none |
| Rumble | `rVibration` (`mBaseFps`), `sVibration::vf12` 0x0104B690 (`mTimer`) | locked or dt-correct | real milliseconds | none |
| Enemy lunge `cEm5501ActAtckRushBite` (em5501, 5500 family; hydra *inferred*) | `cEm5501ActAtckRushBite::vf06` 0x00A36DF0 | **frame-counted** | while the motion frame is in (30, 60): position += direction × `+0x74E0` every frame, no step | the lunge covers ×2 / ×2.4 / ×4 the distance during the same motion window |
| Online (pawn sharing, protocol) | `sDDCaProto` region: 0x00CA20A0, 0x00CA4270, 0x00CA4310 | locked (real time) | `GetTickCount` (IAT 0x0139D0AC) timestamps and time-outs | none |
| Event manager throttle | 0x00435D90, from `sEventManager::vf06` 0x00435840 | locked (real time) | `GetTickCount`; acts once 5000 ms (0x1388) have passed | none |
| Movies | `uMovie` (0x00F7A720, 0x00F7BD90, 0x00F7C6B0, ...) | locked (real time) | `timeGetTime` (IAT 0x0139D3A8) | none |
| Physics middleware | — | — | no Havok or PhysX in the exe (no strings); all physics is the game's own code | — |

Not reviewed one by one: the 66 `mFps` readers outside the systems
above. The 687 functions that load frame-rate constants are classified
in section 5.

## 4. Details

### 4.1 Enemy recognition (`sRecognition`, 0x00487B00)

**Properties** (`sRecognition::vf03` 0x00487960; defaults from the
constructor 0x00487890):

| Field | Name | Default |
|---|---|---|
| `+0x24` | チェック間隔(フレーム(30fps)), "check interval (frames at 30 fps)" | 1.0 |
| `+0x2C` | 描画後認知する時間(秒), "seconds drawn before recognised" | 2.0 |
| `+0x30` | 非描画で認知しなくなるまで…, "until no longer recognised when not drawn" | 10.0 |

**What vf06 does:**
1. `+0x20 += mDeltaTime`. The rest runs only when `+0x20 ≥ +0x24`, and
   then `+0x20` is set to 0.
2. For each enemy, it adds `mDeltaTime / mFps` to either the drawn time
   (+0x10) or the not-drawn time (+0x14) and resets the other.
3. The enemy is recognised when the drawn time exceeds `+0x2C`, and
   forgotten when the not-drawn time exceeds `+0x30`.

**The bug:** step 2 adds only **this frame's** time, but runs once per
interval:
- At 30 fps (step 1.0) it runs every frame and is exact.
- At F fps it runs every `ceil(F/30)` frames and adds 1/F s each time.
  The timers then advance at `30/F × …` of real time: half at 60, a
  quarter at 120, an eighth at 240.
- So "recognised" takes 2 s × F/30, and forgetting takes 10 s × F/30.
- The original already showed this at its usual 46-96 fps
  (3-6 s / 15-32 s).

**Who reads it.** The recognised list (`*0x018FA4F4`) is read by:
- the pawn AI's situation check (`cAICheckSituationCmc::vf11`,
  0x00584780);
- two functions of `sPlayerManager` (`*0x018FA4EC`) called from its vf06:
  0x004112E0 and 0x00411520 (timers by `mDeltaTime` with 300 / 700
  thresholds; their meaning is not traced).

So at high rates pawns would notice enemies, and stop treating them as
present, several times later than at 30 fps.

### 4.2 Cloth (`uSimSoftBody`, 0x00F20640)

**The step:**
- "Sim TimeStep" (`+0xC0`) = 1/60 s (0.016666668) from the constructor
  0x00F12400.
- It is 1/150 s when `*(0x018D1D20)+0x14 == 2`, *inferred* to be a
  quality setting.

**Each frame:**
1. `acc += min(mDeltaTime / mFps, 0.05)`.
2. `n = floor(acc / step)`.
3. If `n > 0`: `acc = 0` (the remainder is dropped), and `n` substeps
   run.

**What follows:**
- Cloth time advances at `n × step` per `ceil(step × F)` frames, so it
  runs below real time unless F × step is a whole number.
- At exact divisors (60, 120, 240) float rounding decides whether the
  accumulator reaches the step one frame late. That halves the rate in
  that frame.
- Visible effect: capes and skirts lag and swing in slow motion, about
  80 % at 144 fps, and jitter at 120 and 240.

### 4.3 Character integration (0x0084CCC0)

**Order:**
1. Under statuses, scale the velocity (Frozen, Petrify; see below).
2. `pos += v × step`.
3. `pos += push and wind × step`.
4. `v += a × step`.
5. Clamp the velocity (FRAME.md 5.2).

**Explicit Euler.** Velocity is integrated after position, so a
trajectory under constant gravity gains `v0 × h / 2` in height (h = step
in base frames). The jump table in section 3 gives the numbers.
Horizontal air movement is linear (air control 0x00AC51D0), so it is not
affected beyond the flight time.

**Frozen (status 11, `+0x26A0 & 0x800`).**
`v.xz × max(0, 1 − f)` each frame, with `f = +0x2864`. No step.

**Petrify (status 12, `& 0x1000`).**
- The velocity × `+0x27B0` each frame, again with no step.
- The yaw rotator uses `step × +0x27B0` instead, so it is dt-correct.

Both multiply the velocity each frame:
- Where an action rewrites the velocity every frame (locomotion), this is
  a constant scale.
- Where the velocity persists (falls, knockback, sliding), the decay is
  per frame: frame-counted.

*Not traced:* which actions keep their velocity under these statuses.

### 4.4 Camera springs (0x007BC400, 0x007BC800)

**How they are called.** `cCamImp*` call these with `sWorkRate+0x20 ×
mDeltaTime`. They loop while the remainder is above 0:
1. `h = min(rest, 1.1)` (`[0x014091AC]` = 1.1).
2. Evaluate the spring (vtable +0x14 / +0x18).
3. Scale the velocity change by `h` and apply it (+0x1C / +0x20).
4. `rest −= 1.1`.

**What follows:**
- The step is respected, but each substep is an explicit integration.
- At 60 fps the step is already 0.5, so higher rates refine the
  integration rather than change it.
- The camera feel changes slightly toward the continuous spring.

*Not checked:* per-frame lerps outside these helpers in the 16 `cCamImp`
readers.

### 4.5 Effect noise on the frame counter

**The counter.** `sEffect::vf06` increments `sEffect+0x74` once per
frame.

**Its readers.** 23 functions in the effect engine read it. They select
among four time sources for a noise or random input (0x011E7040 feeds it
to 0x01197980 and 0x01197DF0):
- mode 4: this frame counter;
- other modes: the effect's own frame (+0xFC), or the emitter's
  (`+0x14` of the argument).

So effects authored with mode 4 change their noise once per rendered
frame.

## 5. The 687 frame-rate-constant candidates

687 lifted functions load an f32 30, 60, 1/30 or 1/60. All 687 were
decompiled. They were then classified by script, and the rest by hand
(the scripts `classify.py` and `accum.py` are private, with the lists).

**Pass 1: by the line the constant is on.**

| Class | Functions | Meaning |
|---|---|---|
| Scripted away: every use is with the step, an angle or a unit conversion | 97 | 28 with the step (e.g. `dt / 30`, `dt × 60`); 25 angles (with π/180 or 2π); 43 conversions of a loaded value (seconds × 30 → base frames, frames / 30 → seconds); 1 mixed |
| Integer or indirect use only | 23 | the constant's address is loaded but no float literal appears (integer frames, table copies) |
| Left for pass 2 | 567 | the constant is a value passed or stored (406 bare literals, e.g. `FUN_0043f4c0(30.0)`, `param[0x9d8] = 30.0`), a comparison, or an assignment |

**Pass 2: the risk is not the constant but a per-call update without
the step.** For each of the 567, every in-place update of a stored value
(`A = A ± x`, `A = A × k`) was listed, together with whether the
function reads a step anywhere: `sMain.mDeltaTime`, `+0x7c`, an
`sWorkRate` rate, or a unit step `+0x14` (also as `[5]` of an int
array, or through a pointer).

| Result | Functions |
|---|---|
| No in-place update: the constant is a duration, limit, distance or angle used as a value | 479 |
| In-place updates in a function that reads a step | 71 |
| In-place updates without any step read, checked by hand | 17 |

**The 17 by hand:**

| Function | Verdict |
|---|---|
| `cEm5501ActAtckRushBite::vf06` 0x00A36DF0 | **frame-counted** (section 3: position += direction × speed per frame) |
| `uEmLizardBase::vf66` 0x00914960, `uEm0900::vf66` 0x00959110, `uEmBoneBase::vf66` 0x0096A3C0, `uEm5000::vf68` 0x0097BE30, `uEm5200::vf69` 0x009A4560 | hit and damage callbacks: counters and scales once per event |
| `cPlCliffHangClimb::vf05` 0x00ADD670, `cEm5800BackJump::vf05` 0x00A77100 | action start (vf05): once per action |
| `cEm0100ActJumpJoy::vf06` 0x008DA070 | adds once, when its motion ends |
| `uEm0204::vf33` 0x008FA5B0 | a variant's one-time strengthening (×2, ×1.2, ×1.5) |
| 0x00856FC0 | damage accumulation (event) |
| 0x0090DD30 | adds the floating-origin shift (`*0x018FA528 + 0x40`): a displacement, not a rate |
| 0x008939A0 | totals from a table (setup) |
| 0x00AB5420 | a generic add helper (the caller supplies the amount) |
| 0x00AE05C0 | collision correction (a positional fix) |
| `uOmObj7010::vf115` 0x00C3F130 | a counter per callback |
| `sMain::vf06` 0x00DBFA90 | the clock itself (`mDelayFrame`, `mGlobalSpeed`) |

**Limits of the passes.**
- Pass 2 sees updates written as one statement on a stored field. It
  does not see:
  - accumulation through locals or vector helpers (the camera code; its
    helpers were checked by hand, section 3);
  - per-frame increments of integer counters compared against frame
    thresholds in other functions;
  - frame-counted logic that loads none of the four constants.
- The 71 with a step read were not checked line by line. A function can
  read a step for one update and not for another, as the integration's
  Frozen and Petrify factors show (section 4.3).

## 6. Proposals for an opt-in high-frame-rate mode

The rebuild stays 1:1 by default (the original's code and its 30-based
step). A high-frame-rate mode would change only the items below. Each
change is a deliberate deviation, to be labelled as such.

| Item | Proposal |
|---|---|
| sRecognition timers (4.1) | Add the elapsed interval (the accumulated `+0x20 / mFps`) instead of this frame's step; or run vf06's body at a fixed 30 Hz. The result equals the original at 30 fps. |
| Cloth (4.2) | Keep the remainder (`acc −= n × step`), as `uCnsTinyChain` already does. Optionally cap n. Cloth then runs at 60 Hz at any rate. |
| Frozen and Petrify velocity scaling (4.3) | Replace the factor `k` per frame by `k^step` (`(1 − f)^step` for Frozen), which equals the original at step 1 (30 fps). |
| Explicit Euler integration (4.3) | Either keep it (a few percent on jump height between 60 and 240 fps), or run the character integration in fixed 1/60-s substeps so jumps match the original at 60. To match the original at a chosen reference rate, substep at that rate. |
| Camera springs (4.4) | Keep (already step-aware); optionally fix the substep at 0.5 (60 Hz) so the feel equals 60 fps. |
| Effect noise, mode 4 (4.5) | Advance the counter by `mDeltaTime` (×2 for a 60-Hz reference) instead of 1 per frame, or update it at a fixed rate. |
| em5501 lunge (section 3) | Multiply the per-frame displacement by the unit step (equal to the original at 30 fps). |
| Long chains (`uChain`) | Keep the substep size fixed (e.g. one substep per 1/30 s × `+0x88`) and vary the count with the step, so iterations per second stay as at the reference rate. |
| End credits / `+0x7c` users | Use this frame's `mDeltaTime` instead of the 1-s average. |
| `mMaxFps` | Raise the cap only in this mode; the default keeps the original's 150. |

Everything classified dt-correct needs no change. Locked systems run at
their own rate. Rendering can interpolate between simulation steps if a
system is moved to fixed substeps.

## 7. Open

- Live checks at 60 / 144 / 240 fps:
  - sRecognition's +0x10 / +0x14 per enemy;
  - cloth step counts (`uSimSoftBody+0x158`, the steps this frame);
  - jump apex;
  - the em5501 lunge distance.
- Not yet traced:
  - the `uChain` iteration loop (does it damp per iteration?);
  - what `uCnsChain`'s job computes;
  - the 71 step-reading candidates of section 5, line by line;
  - integer frame counters outside the constant candidates.

## 8. Readable ports and models (L39)

`crates/mtframe` is the 1:1 baseline that the opt-in high-frame-rate mode
will change, one small diff per system. Brief and results:
[tasks/L39-readable-frame-systems.md](tasks/L39-readable-frame-systems.md).
Nothing below is implemented: the default stays the original.

Each function is in one of three states:

- **Strict port, wired.** `lift.py gen` puts it over the lifted body, as
  for mtmath and mtcore (docs/MATH.md, docs/CORE.md). It reproduces every
  register, xmm lane, x87 state and written byte, and the machine state at
  every call (stack window included). It is verified against the lifted
  vectors and edge cases recorded from the original
  (`crates/mtframe/tests/differential.rs`, `research/native/vectors_frame.py`).
- **Observable-equivalent model, not wired; the high-frame-rate mode's
  baseline.** The lifted body stays: a strict port would have to replay
  dead state (scratch registers carried across loops into later callouts,
  as with L24's blend). The model is verified against recordings of the
  original on its observable behaviour (`crates/mtframe/tests/models.rs`):
  - every byte of memory outside the original's own stack frame;
  - the order, targets and arguments of its calls;
  - the use of their results.

  It does not reproduce register values nobody reads. The mode deviates
  on purpose, so a model is the right base for it.
- **Lifted.** No readable version.

| System | Function | State | What the mode changes |
|---|---|---|---|
| Clock | `sMain::vf06` 0x00DBFA90 | lifted (dead state: the x87 stack carries mFps and the counter frequency across ftol, `__allmul`/`__aulldiv` and Sleep) | nothing in the code: raising `mMaxFps` (+0x3C) is a data change |
| Enemy recognition | `sRecognition::vf06` 0x00487B00 | model `recognition::update` | the timers add this frame's step: 0x00488298 `movss xmm0, [eax+0x70]` / 0x0048829D `divss xmm0, [eax+0x38]` (drawn) and 0x004882BD / 0x004882C2 (hidden). The mode adds the elapsed interval instead: the timer value at the gate (0x00487B51 `addss xmm0, [ebp+0x20]`, the base frames since the last run) / `mFps`. In the model: phase 5's `step` becomes `t / fps`, with `t` the gate's sum. At 30 fps and interval 1, `t` = `mDeltaTime` and the result is unchanged |
| Cloth | `uSimSoftBody::vf11` 0x00F20640 | lifted; its step schedule (0x00F20847..0x00F208FC) is the model `cloth::schedule` | the remainder: 0x00F208D9 `xorps xmm1, xmm1` / 0x00F208DC `movss [esi+0x150], xmm1` drop it. The mode keeps it (`acc −= n × step`, which the loop at 0x00F208B0 has already done), so the step `if steps != 0 { acc = 0 }` goes. Optionally cap n. The 0.05 s clamp (0x00F2085E) stays |
| Frozen, Petrify | integration 0x0084CCC0 | strict port `integrate::update` | the factors per frame: Frozen at 0x0084CD0D / 0x0084CD15 (`mulss` by max(0, 1 − `+0x2864`)), Petrify at 0x0084CD53 / 0x0084CD5B (by `+0x27B0`). The mode uses `k^step` with the unit step (`+0x14`, base frames): in the port, `brake(m, c, k)` takes `k.powf(step)`. At step 1 (30 fps) it is the same |
| Explicit Euler | integration 0x0084CCC0 | strict port | optional: run `integrate::update` in fixed substeps of a reference rate (section 6) |
| em5501 lunge | `cEm5501ActAtckRushBite::vf06` 0x00A36DF0 | strict port `lunge::update` | the per-frame displacement: 0x00A36ED3 / 0x00A36EDC / 0x00A36EE5 `addss` the velocity to the position. The mode scales it by the enemy's unit step (`enemy+0x14`): in the port, `position[k] = p + velocity[k] × step`. The stored velocity (+0x74D0) stays per base frame |
| Effect noise, mode 4 | `sEffect::vf06` 0x00FB64D0 (counter), readers 0x011E48E0, 0x00E6C260 (+21) | strict ports `effect::update`, `element_time`, `emitter_time` | the counter: 0x00FB64DC `inc dword [esi+0x74]`. The mode advances it by whole base frames of game time: it keeps the fraction aside (a field of the mode's own, since sEffect has none) and adds floor(fraction + `mDeltaTime`) per frame, or ×2 for a 60 Hz reference. The readers stay |
| Long chains | `uChain::vf09` 0x00F0C160 | strict port `chain::update` | the split: 0x00F0C3C5 `mulss` (time = scale × step) and 0x00F0C3DE `fdivr` (substep = time / `+0x88`). The mode keeps the substep at the reference rate's and varies the count with the step. The solver (0x00F01390, lifted) reads the count from `+0x88`, so the mode sets a per-frame count there and restores it afterwards; whether the solver reads it elsewhere is not traced |

**Verified on this branch** (`cargo test -p mtframe`, release, vectors required):
- 2,400 edge cases, 400 per strict port, including the x87 precision
  control for the chain's substep and quiet and signalling NaN inputs where
  SSE meets them.
- The lifted vectors that stay inside the port. Three vectors of
  `sEffect::vf06` enter another lifted body; the integration build's
  replay checks them.
- The two models against 400 recordings each. Cases where the original
  crashes are dropped: null records, and loops beyond the emulator's
  instruction budget.
- Mutation checks for every port and model fail between 19 and 400 cases
  each.

**Pending the merge train:** the full ddnative replay with the ports
wired, and the benchmark.
