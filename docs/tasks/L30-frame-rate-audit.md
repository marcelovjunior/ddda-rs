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

(open)
