# Pinned: paste-in commands for the cloud-session A/B (PowerShell)

Context and reasoning: `docs/CLOUD_AB_PLAN.md`, `docs/HOT_PATHS.md` section 14. Branch: `claude/confident-ride-qjpx53`.
Adjust the repo path and sandbox config path to your machine.

## 1. Get the branch
```powershell
cd E:\ddda-rs
git fetch origin claude/confident-ride-qjpx53
git worktree add E:\ddda-rs-wt\CLOUD origin/claude/confident-ride-qjpx53 -b task/cloud-ab
cd E:\ddda-rs-wt\CLOUD
```

## 2. Tests (about 1 minute; `an_abort_inside_the_window_releases_the_gated_threads` flakes ~20%, not a regression)
```powershell
cargo test --release -p ddruntime --lib
```

## 3. Build (about 25 minutes; -j5 avoids the rustc out-of-memory)
```powershell
$env:CARGO_TARGET_DIR = 'C:\ddda-targets\cloud'
cargo build -j5 --profile play-o2 -p ddplay --features emu,ddruntime/parallel-shadow,ddruntime/stack-window,ddruntime/inline-helpers
```

## 4. Benchmark setup
Copy `E:\ddda-re\bench\config.ini` (pinned, windowed) over the sandbox config; check the sandbox save equals the real save; take `game.lock`.

## 5. Runs: 3 each, same save, same window (t=46..84 s), launch with your usual `playtest_fast2.bat` using the new `ddplay.exe`
```powershell
# A baseline: nothing set
# B join gate + spin
$env:DDRUNTIME_WAIT_ALL_GATE='1'; $env:DDRUNTIME_WAIT_ALL_SPIN='1'
# C adaptive worker spin
$env:DDRUNTIME_EVENT_SPIN_ADAPT='1'
# D wake only as many threads as jobs
$env:DDRUNTIME_JOB_WAKE_FIT='1'
# E render worker priority (1, then 2)
$env:DDRUNTIME_RENDER_PRIORITY='1'
# F shorter worker spin (10, 5, 0)
$env:DDRUNTIME_EVENT_SPIN_US='10'
# clear between runs
Remove-Item Env:DDRUNTIME_* -ErrorAction SilentlyContinue
```

## 6. Profile with the winning combination
```powershell
bash C:/ddda-targets/L172/runs/autorun3.sh <path-to-new-ddplay.exe> cloud1
```

## 7. If it breaks
Hang or freeze: unset `DDRUNTIME_WAIT_ALL_GATE` / `DDRUNTIME_WAIT_ALL_SPIN` first (the only synchronisation knobs). Still broken with nothing set:
revert one of the always-on `ndraw:` commits listed in `docs/CLOUD_AB_PLAN.md` section 1.

## 8. Send back
`stacks_cloud1.json`, `profile_cloud1.txt`, the run log, and fps per config.
