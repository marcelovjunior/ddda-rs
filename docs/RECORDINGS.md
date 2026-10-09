# Recordings of the original game

Measurements taken from the running original (Steam build, see
METHODOLOGY.md). Each one cites its session, so tests and docs can use the
numbers as an oracle. The raw recordings are private
(`E:\ddda-re\notes\moverec\`) and never committed.

**Tools:**
- `E:\ddda-re\live\session.py`: scripted keyboard input while the window
  is focused, recording memory at about 200 Hz.
- `moverec.py`: memory reads only.
- The in-process oracle ([ORACLE.md](ORACLE.md)) supersedes both for
  unattended work.

Field names come from the exe's reflection data (`research/dti/dti_props.py`).

## The clock (2026-10-09, session 005707)

`sBbsRpgMain` (the runtime `sMain`) at 0x080EBC70:

| Field | Offset | Value |
|---|---|---|
| `mFps` | +0x38 | 30.0, the base rate |
| `mMaxFps` | +0x3C | 150.0 |
| `mActualFps` | +0x44 | 46-96 measured (render rate) |
| `mDeltaTime` | +0x70 | about 30 / `mActualFps`: 0.312 against 30/96.34 = 0.311 in one read. Across 2,165 samples of session 005707, dt × fps had a median of 28.3 and ranged widely, and no sample matched exactly (codex-11, board #488). `mActualFps` is probably a smoothed rate while dt is per frame; the reads were also sequential, not atomic. |
| `mDeltaTimeBorder`, `mDeltaTimeLimite` | +0x74, +0x78 | 2.0 each |
| `mGlobalSpeed` | +0x80 | 1.0 |

`cUnit` has its own `mDeltaTime` (+0x14), the per-object step that
per-frame rules multiply by (e.g. `0x00AC51D0`, air movement).

**Conclusions:**
- The original simulates in units of 1/30-second frames, with a variable
  per-frame step `mDeltaTime` that averages about 30 / fps and is probably
  capped at 2.0 frames (`mDeltaTimeLimite`). The exact per-frame formula
  (from the frame timer) is not traced; see the clock work in the Codex
  lane's weather-clock-bindings brief.
- So per-frame parameters (`pljumpparam`, `m_pl000_00` speeds, stamina
  rates) are per 1/30 s, and the game is frame-rate independent by
  scaling.
- `ddsim`'s fixed 1/120 s step (`TICK` = 0.25 frames) is the same model
  at a constant step.
- An FPS unlock needs no change to the rules.

## Stamina (session 005707: female Arisen, mage, max stamina 640)

`cPlayerInfo` at 0x0DAD7EF0:
- `mHp` +0x29C (455.17);
- `mHpMax` +0x2A0 (542);
- +0x2A4 (455.17; recoverable HP?);
- `mStamina` +0x2A8;
- `mStaminaBase` +0x2AC (520);
- `mStaminaLv` +0x2B0 (120; 520 + 120 = 640).

| Phase | Measurement |
|---|---|
| Sprint (velocity 22.5 cm/frame) | -45.3 stamina/s (620.01 → 574.71 over 1.0 s), about 1.51 per 1/30-s frame |
| Run, walk, idle | no drain |
| After the sprint ends | stamina holds for about 0.5 s (482.87 from 5.09 s to 5.6 s) |
| Recovery while idle | +48.9 /s (523.13 → 572.07 over 1.0 s), about 1.63 per frame; full again about 4 s later |
| Sprint stop | speed 22.5 → 10.35 → 5.26 → 0 cm/frame over about 0.9 s |

Compare with the stamina rules traced from the exe in [PARAMS.md](PARAMS.md).

**Refined** (codex-12, from the raw 005707 samples):
- The drain is exactly -1.500 per `mDeltaTime` unit.
- Recovery is +1.625 per unit.
- The hold after sprinting lasts 0.590-0.614 s, bracketed by samples.

**Passive oracle read** (codex-1 #577, the idle actor, later that night):
- body and load class 1/1, recovery factor 1, stamina debt 0;
- status masks 0;
- `cUnit.mDeltaTime` = `sMain.mDeltaTime`;
- `sWorkRate` rates 0 and 1 both 1.0.

This supports the neutral factors for the current idle state only. It
does not establish the 005707 sprint conditions.

## Turning with the keyboard (session 005454)

Running (10.17 cm/frame, town run), then switching from W to D: the
velocity heading changes by 90° between two samples 50 ms apart, with the
speed unchanged. With digital input the original sets the movement
direction at once. `ddsim`'s instant turn matches it.

The about 70°/s turning in the earlier recording (232805) was most likely
a gradually moved analog stick. Not yet checked with a pad.

## Ground speed and gaits (session 232805, maintainer playing)

See [MOVEMENT.md](MOVEMENT.md):
- run velocity 10.165 cm/frame (town);
- sprint velocity 22.5 cm/frame;
- locomotion motions advance 34.5 frames/s;
- jump apex 93.1 cm, 0.325 s after takeoff.
