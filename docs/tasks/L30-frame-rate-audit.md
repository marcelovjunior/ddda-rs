# L30: frame-rate independence audit (what an unlocked frame rate breaks)

- **Owner:** claude-7
- **Branch:** `task/L30-fps-audit` (worktree `E:\ddda-rs-wt\L30`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md). This is research, mostly
  Ghidra and the lifted sources. It is CPU-light, so it may run while
  heavy work is paused.

## Why

`docs/FRAME.md` section 1a describes the game's clock (`sMain::vf06`,
0x00DBFA90).
- Gameplay advances by `sMain.mDeltaTime`, in units of 1/60 s (`mFps` =
  60).
- The frame rate is capped only by `mMaxFps`; the `MaxFPS` option allows
  30-1000.
- So an unlocked frame rate is a setting in the rebuild.

The player wants a high, unlocked frame rate. That works only where every
system scales its per-frame work by `mDeltaTime` (or a derived step).
A system that adds a fixed amount per frame runs faster at 120 fps than at
60. One that integrates with a frame-rate-dependent error (for example
`x += v` with a damping factor applied per frame) drifts. Both are
frame-rate bugs, whether or not the original had them.

## Task

1. **Find the per-frame consumers.** Use readers of `sMain+0x70`
   (`mDeltaTime`), `+0x7c`, `+0x38` (`mFps`), the per-unit
   `cUnit.mDeltaTime` (+0x14) and `sMain.mFrameTimer` (+0x30) (FRAME.md
   lists more). Map who reads them, by system: units/actions, motion and
   animation blending, physics/collision, cloth/hair, cameras, effects
   (`mEffectFps`), GUI timers, sound timing, AI timers, network.
2. **Find the frame-counted logic.** Look for code that counts frames
   instead of time, or that applies a constant per call to code run once
   per frame. Typical signs:
   - integer counters incremented per update and compared against
     thresholds;
   - lerp and damping factors applied per frame without a dt term;
   - fixed per-frame velocities;
   - `mFps`-specific constants (60, 30, 1/60, 1/30).
   Start from the systems most likely to matter: camera, motion blending,
   cloth, effects, GUI.
3. **Classify each finding** with its address and evidence (decompiled
   snippet summary, not raw dumps):
   - *dt-correct*: scales with `mDeltaTime`;
   - *frame-counted*: a fixed per-frame step, so its rate scales with the
     frame rate;
   - *dt-approximate*: uses dt, but in a way whose result depends on the
     rate (non-linear damping, Euler integration with large error);
   - *locked*: runs on its own fixed clock.
   Estimate the visible effect at 120/144/240 fps.
4. **Check against the original where possible.** The game's own
   `MaxFPS` option (30-1000) and the community's knowledge of what breaks
   at high frame rates are leads. Label them leads; the evidence is the
   code.
5. **Write docs/FRAME_RATE.md:** the classified list, per system, with
   addresses. For each *frame-counted* or *dt-approximate* item, the fix a
   high-frame-rate mode would need in the rebuild, as a proposal (e.g.
   run that system at a fixed 60 Hz substep, or scale its step by
   `mDeltaTime`). The rebuild stays 1:1 by default; these become an
   opt-in mode.

## Results

claude-7. The full audit is in [FRAME_RATE.md](../FRAME_RATE.md). This
was static analysis only: Ghidra over HTTP and scans of the lifted
sources, with no build and no live measurement.

### Main points

- **The step unit is 1/30 s.** `sBbsRpgMain`'s constructor stores
  `mFps` = 30.0 at 0x0041CEC9, over `sMain`'s default of 60 (agreeing
  with RECORDINGS.md's live read). FRAME.md 1a says 60 and should be
  corrected. `sMain+0x7c` is 30 / `sMain.mActualFps`, not another
  singleton's rate.
- **Most systems are dt-correct:**
  - the unit step, hit-stop and slow motion (`sWorkRate`);
  - the clocks, status timers, air control and turning;
  - animation frames and blends, AI intervals and timers;
  - combat recovery and decay, stamina;
  - grass and wind, GUI and fades, sound.

  Input and rumble use real milliseconds. Hair and small chains
  (`uCnsTinyChain`) are a correct fixed-step accumulator.
- **Frame-rate bugs found:**
  1. **sRecognition** (0x00487B00): seen/unseen timers advance by one
     frame's time per check interval. Enemies are recognised after
     2 s × fps/30 and forgotten after 10 s × fps/30 (8 s / 40 s at
     120 fps). The list feeds the pawn AI's situation check
     (`cAICheckSituationCmc::vf11`) and `sPlayerManager`.
  2. **Cloth** (`uSimSoftBody::vf11` 0x00F20640): fixed 1/60-s steps, but
     the accumulator's remainder is dropped. Cloth runs at
     `F / ceil(F/60)` steps/s: 48 at 144 fps (80 %), 50 at 100, 37.5 at
     75, and borderline at 60/120/240.
  3. **Frozen and Petrify** velocity factors are applied per frame
     without the step (integration 0x0084CCC0).
  4. **Effect noise** in time-source mode 4 is driven by `sEffect`'s
     per-frame counter (23 functions).
- **dt-approximate:**
  - the explicit-Euler character integration (jump apex 95.0 cm at 60 fps
    and 91.4 cm at 240);
  - the camera springs (substeps of `min(rest, 1.1)` base frames);
  - the end credits and two timers on the 1-s averaged rate (`+0x7c`).
- **Lead:** Capcom's PC FAQ says the release caps at 150 fps to avoid
  physics and rendering issues, and `mMaxFps` read 150 live.
- **Proposals** for an opt-in high-frame-rate mode, per item: FRAME_RATE.md
  section 6. The default stays 1:1.

### Not done

- No live measurement. The checks to make: sRecognition timers, cloth
  step counts, and jump apex at 60/144/240.
- Not located or traced:
  - the long-chain (`uCnsChain`/`uChain`) step;
  - FRAME.md's 150-frame AI-sense timer;
  - network timers;
  - the remaining `cCamImp` lerps.
- 687 functions that load 30, 60, 1/30 or 1/60 are listed as candidates
  (mostly unit conversions); only samples were classified.
