# L131: profile attribution

Assigned to codex-11 by head/user, 2026-10-13. Branch
`task/L131-profile-attribution`, based on `task/fps-line`.
Scope: `scripts/sample_profile.py`, synthetic Python tests, this brief.

## Results

- Fixed guest-address attribution from raw Rust v0 symbols with `R` or `_R`,
  demangled paths, partially decoded paths and `ddlift_hot_00` frames.
  Generic helper symbols that merely mention a lifted crate do not count.
- Per-thread nearest lifted caller owns each non-wait sample, including
  all non-lifted runtime callees (memory, Flags, x87, SSE helpers, etc.).
  Inclusive counts each guest address once per sample, including recursion.
  Two- and three-level chains end at that nearest caller and print outer
  caller -> inner callee, skipping host helpers but retaining recursion.
- Added exact comma-separated `--threads` descriptions, applied before
  suspension and when reporting. Descriptions refresh during collection.
  Existing `--seconds`/`--hz` remain. No low-CPU thread suppression in reports.
- Added `--save-stacks` JSON (raw symbol table and leaf-first stacks) and
  `--input` offline replay. Keep captures in the private research directory.
  Empty stacks and terminated threads with unknown CPU time report safely.
- Tested: **7 Python regressions pass**, including an actual offline CLI
  replay without process attachment. Command:
  `python -m unittest discover -s scripts -p test_sample_profile.py -v`.
- Existing `profile_c15b.txt` and `profile_c16b.txt` contain aggregate text,
  not stacks. No companion stacks found alongside them. Read-only matcher
  check recognized **271/271 rows (150 addresses)** and **261/261 rows
  (132 addresses)** respectively. Cannot reconstruct helper ownership or
  call chains from those summaries; no new gameplay ranking claimed.
- No Rust changes/builds, shard builds, GPU tests, game runs, install writes,
  copied game/research data, or push. Synthetic fixtures only are committed.
- Limitations: percentages are shares of sampled non-wait stacks, not exact
  CPU seconds. Existing active-thread heuristic and OS-wait classification
  remain; optimized/inlined/truncated stacks can omit guest callers. Requested
  Hz is a target; reports show achieved sampling rate (old captures: 12-13 Hz).
- Milestone update delegated to head because codex-7 holds
  `docs/MILESTONES.md` (board #11420, acknowledged #11425).

## Next action: head capture during gameplay

Only the head runs the game. After the L129 CPU window, keep the game alive
through symbol resolution. Run this PowerShell command while already in
representative gameplay (all threads retained so every job thread is covered):

```powershell
python E:/ddda-rs-wt/L131/scripts/sample_profile.py --name ddplay.exe --seconds 60 --hz 100 --depth 96 --top 40 --save-stacks E:/ddda-re/notes/runs/profile_L131.json > E:/ddda-re/notes/runs/profile_L131.txt
```

For focused capture append
`--threads "guest main thread,guest thread 0x1070"`. If PDBs are not adjacent
to the executable, also supply `--symbols <directory-containing-this-build-PDBs>`.
If several ddplay processes exist, use `--pid <target-pid>`.

Offline focused report (no live target required):

```powershell
python E:/ddda-rs-wt/L131/scripts/sample_profile.py --input E:/ddda-re/notes/runs/profile_L131.json --threads "guest main thread,guest thread 0x1070" --top 40 > E:/ddda-re/notes/runs/profile_L131_main_job.txt
```

Next: head supplies that capture to codex-10/L130 for exact hot-function selection.
