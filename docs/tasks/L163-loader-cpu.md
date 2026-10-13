# L163: boot waits and loader CPU

Owner: codex-6. Branch: `task/L163-loader-cpu`, worktree L163.
Head assignment: merge L156, classify the four roughly seven-second boot
intervals from original evidence, then rank the forthcoming loading-window
all-thread raw stacks and optimize the top exact item. Runtime-only, four
build jobs, own C: target; no shard builds or game execution by this lane.

## Findings

Detailed source chains, timing classification and profile rank are in
[LOADING.md](../LOADING.md). The boot intervals mix original logo holds,
authored GUI duration, autoplay input cadence and work; there is no single
seven-second autoplay delay. Title entry/exit nominally total 5.3 s at the
resource-selected 30 Hz. Autoplay cadence is 1.66 s. Storage mode 7's
120-base-frame countdown can end early through original input.

Combo30 decoders: 16.968750 CPU s / 14 s window; 207/675 non-wait samples.
Nearest lifted ownership: CF02E0 76 (36.7%), CFEEC0 59 (28.5%), DB9F40 52
(25.1%). The skip loop is the largest new exact target. Backend shader
parsing/typing was reported to head separately.

## Diagnostic implementation

`DDRUNTIME_LOAD_LOG=1 DDRUNTIME_BOOT_DETAIL=1` adds sparse `boot_detail`
rows before the existing aTitle dispatch. It reports GUI state, sequence
completion and result for the two known vtables, or storage substate,
dialog mode and timer bits. It records region setting at title entry.
No post-dispatch object reads; no timer/input changes. Timer ticks do not
create rows unless their positive/nonpositive classification changes.
The integer classification avoids altering host FP status on a NaN.

These rows **do not reset** transition cost baselines and the offline
parser excludes them from milestone phases. They are sampled at title
dispatch boundaries; a transient within one dispatch may be missed.
They are not accepted-key or visible-pixel timestamps. Defaults unchanged.

`DDRUNTIME_ARCHIVE_SKIP=1` batches original buffered skip CFEEC0, retaining
refill dispatch/failures, flags, registers and stack. No default game-logic
change. CPU timeline now uses cached handles after one host discovery at
first Present, with process residual covering undiscovered host CPU.

## Evidence receipts

SHA-256:

- `run_c26p1.log`: `8fb7f9abe47bccc947d434b2a7f91cc9f9e8f23543d35313695e790f3d1b27f7`.
- `run_c26p4.log`: `55fd3a6ff34cab622687ccaaa86d7c1bcf14821e91e4cbad448c6fb2d6a7f8a3`.
- `ddplay/src/input.rs`: `937f64e854d9103f6b6863b165fcfeaefa917760e433fdfcd613bf6563cf1bc9`.


- `stacks_c30load.json`: `f66d421c71ad16179b41a75d3359d0076b276ff522d68a506c63fc0c07ad7b0f`.
- `profile_c30load.txt`: `c62ae3b6e9b7dd1dd5f4c533dc4607f7c8592bc8ea9284c9e82199725dabdc88`.
- `run_c30load.log`: `7a4a8cd1728bcea93240f487b3361306c878264eebc9a1c523e4ca909aa99356`.
- Private `native-L163/skip.bin`: `985db5adbc0057922762ccdaa31a58f2bd744481e6d9475388f64d848baf3cd3`.

## Results

- L156 merged as `5e71af89`, only additive RUNTIME documentation conflict.
- Final runtime library: **93 passed, 0 failed, 5 ignored**, both boot
  diagnostics enabled, required private vectors present. New skip oracle
  executes 192 original-instruction cases; scalar differential 192 cases;
  maximal-u32-span and exited-worker-cache regressions pass.
- After a final FP-state guard on boot timestamp formatting, focused
  load-log tests: **10 passed**, including non-default guest MXCSR.
- Parser: **16 passed**; Python oracle compiles and generated private
  vectors. `git diff --check` clean.
- Earlier full light suite: 234 passed, 4 failed, 7 ignored. Two new boot
  fixtures lacked initialized Sparse fields (fixed); inherited Present
  timing test passes in final library rerun. Inherited gamma Reset failure
  remains outside this task. Unchanged inflater integration tests passed;
  no full suite repeat after diagnostic/skip edits, final library rerun
  covers all new runtime code.
- No shards, GPU tests or game window runs in this lane. Implemented and
  regression-tested; not yet game-image/control/load compared. Head asked
  for skip 0/1 under identical FAST=1 settings, then logging-off timing,
  plus BOOT_DETAIL=1 once to partition the remaining boot gates.

Next action: head integration run with ARCHIVE_SKIP=1 and cached timeline;
retain opt-in until image/control and load-time acceptance.
