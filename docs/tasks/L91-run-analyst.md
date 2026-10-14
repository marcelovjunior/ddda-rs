# L91: automatic run analyst

Assigned to codex-8 by head through the session brief. Branch
`task/L91-run-analyst`, worktree `E:/ddda-rs-wt/L91`; base
`task/L88-quick-render`. Scope: offline tooling only, no game/GPU runs,
no runtime edits or shard builds. Files: `scripts/analyze_run.py`,
`scripts/test_analyze_run.py`, this brief. Follow `AGENT-RULES.md`.

## Usage

```text
python scripts/analyze_run.py RUN_LOG PROFILE [GPU_CSV] [--locks LOCK_STATS]
python scripts/analyze_run.py BEFORE_LOG BEFORE_PROFILE BEFORE_GPU --compare AFTER_LOG --after-profile AFTER_PROFILE --after-gpu AFTER_GPU
python -m unittest discover -s scripts -p test_analyze_run.py -v
```

GPU files and separate lock files are optional on both sides; use
`--after-locks` for the second run. The tool reads files only and uses the
Python standard library. UTF-8 (including BOM) and UTF-16 BOM logs work.
Missing optional data is explicitly unavailable, never zero.

## Measurement contracts

- FPS groups use stage names and occurrence numbers when stage events exist;
  otherwise 20-second windows. Frame deltas divided by printed interval FPS
  estimate elapsed time, then frames / elapsed gives the aggregate FPS.
  This follows `ddplay::present::Stats::take`; integer rate timestamps cannot
  measure subsecond intervals. The first interval is omitted because a partial
  capture cannot recover its frame delta. Obvious gaps, counter resets and
  zero-rounded FPS intervals are excluded. Missing rows with changing rates
  cannot always be detected; complete logs give the best estimates.
- Intervals go to the stage/window containing the endpoint. Boundary intervals
  can mix phases. A marker has only a bracket between adjacent rate messages;
  its exact time is not invented. Stages with no rate observations have no FPS.
- **Epoch caveat:** `ddplay` starts its rate clock at its first rate emission,
  not process startup. Stage timestamps must use that epoch. `--stage-offset`
  adds seconds to stage timestamps; `--after-stage-offset` handles the second
  capture. No automatic alignment is claimed. Head notified on board #10240.
- The frame-bound candidate is the highest measured CPU thread, not a proved
  critical path. Busy and wait percentages refer to sampled stacks (the
  sampler can filter idle threads), CPU to measured thread CPU / wall time.
  WAITS-site percentages refer to waits only. They are not CPU percentages.
- Inclusive function costs overlap. Entries are grouped by recognizable path
  identifiers, including raw Rust v0 names and truncated generic paths; no
  subsystem totals are fabricated. Thread, dispatch and unlock wrappers are
  omitted from the top-cost display; lifted root ancestors remain inclusive.
  Generic context classification is approximate and unknown symbols remain
  `other`. The script does not pretend to fully demangle v0 backreferences.
- Lock reports rank each service's latest cumulative waited milliseconds;
  repeated snapshots are never added. Each row includes its snapshot time.
  These are all-thread lock acquisition totals, not kernel sleep durations
  or evidence of the selected thread's individual contention.
- GPU is the sample mean/range of the second CSV column, utilization.gpu,
  accepting both three- and four-column nvidia-smi captures with/without units.
  It describes the whole file, can include other processes, and is not aligned
  to stages or the profile. Supply captures of equivalent phases.
- The likely-bound line is explicitly heuristic: mean GPU >=90% suggests
  GPU/mixed; highest CPU <50% suggests waits/pacing/unsampled work; otherwise
  CPU execution, with graphics translation indicated if a graphics inclusive
  entry is >=20%. These are reporting heuristics, not original-game constants.
- Comparison shows both complete reports plus FPS deltas for matching stage
  occurrences/windows, candidate CPU/busy/wait changes and GPU mean changes.
  Equal elapsed window labels alone do not establish equivalent game scenes.

## Results

Implemented and tested, 2026-10-13:

- **16 Python tests pass**, including **4 read-only private capture tests**:
  quick run/profile/GPU, waits profile, raw v0 render profile, lock-wait table.
  Other tests cover stage revisits/offsets, rounding, frame weighting, missing
  data, truncated lines, gaps/resets, cumulative stats, UTF-16 and comparison CLI.
- Private capture tests skip when files are absent; override paths with
  `DDDA_ANALYST_EXAMPLES` and `DDDA_ANALYST_NOTES`. Captures stay outside the
  repo. Synthetic fixtures are authored in test code; no private dumps copied.
- Existing `run_quick1.log`: **460 rate rows**, first row excluded, **12 windows**.
  Complete gameplay windows from 100 through 220 s: **4.10–4.80 FPS**.
  `profile_quick1.txt`: guest render thread **0x1034, 84% CPU,
  1061/1157 = 91.7% busy, 8.3% sampled waits**. `gpu_quick1.log`:
  **119 samples, 28.0% mean GPU** over the entire capture, not gameplay alone.
- Separate `lockstats_gameplay.txt`: top wait service
  **EnterCriticalSection 27,917.6 ms**, then **WaitForSingleObject 16,153.8 ms**.
  That separate historical lock capture is not asserted to be the quick run.
- Real `play5` versus `quick1` comparison CLI exercised successfully. No
  performance improvement is attributed to this tool or asserted from those
  unaligned phase windows.
- **No game launched, GPU tests, Rust builds, or original-game comparison.**
  This is Python-only tooling; the focused Python suite is its applicable test.
- Milestone receipt sent to head/codex-4 (#10260): `docs/MILESTONES.md` is
  claimed by codex-4 and was not edited across that ownership boundary.

Remaining limitation: new stage events have only synthetic format coverage;
clock alignment needs a real head capture. Next action: head runs one capture
with aligned stage events and matching-phase profile/GPU, then feeds it to
the analyst. No new game run is needed to use the existing-log report.
