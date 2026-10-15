# A/B plan for the 2026-10-15 cloud session changes

Branch `claude/confident-ride-qjpx53` (merges `task/L196-simfast` and `task/L193-decouple`, then the commits below). Nothing here was
run in the game: the cloud box has no game install, GPU or Windows. Everything was unit-tested (`cargo test --release -p ddruntime --lib`) and
the Windows-only code was type-checked with `cargo check -p ddruntime --no-default-features --target x86_64-pc-windows-gnu`.

Evidence for what to change: the gameplay capture `stacks_b1_prof.json` (~34 fps, ~5,000 draws/frame), analysed in `docs/HOT_PATHS.md` section 14.
Summary: the render worker (`0x1034`) is the critical path (non-wait in 94% of samples, on a CPU 72% of the time); the main thread is mostly
spin/wait in the `DBE560` join; the job workers' profile is flat (no single function above 3.3%).

## 1. Always on (no switch): render-worker allocation and per-draw lookups

All in `crates/ddruntime/src/ndraw_native.rs`. Expected: a few percent of the render worker. If the game misbehaves, bisect with these commits:

| commit subject | change |
|---|---|
| ndraw: keep the previous job's capacity for the next Pending job and draw batch | `Pending::take`/`send_batch` start with last job's size (no regrowth of `Vec<DrawCall>`) |
| ndraw: native path skips flush_reads when none of the draw's reads is dirty | same `dirty_count()`/`reads_dirty` guard the DrawCall path already had |
| ndraw: passes start with the previous pass's draw capacity; texture key hashes two states per step | `pass_hint`; `Mirror::texture_key` |
| ndraw: lazy sampler-table copy in Shadow::prune; box the cached Mirror | no 1.2 KB table copy when a draw has no sampler-state change; the 1.3 KB `Mirror` is no longer moved out and back per draw |
| ndraw: the scissor-test flag is found once per cached fixed state | was a scan of ~200 render states per draw |

Also merged in (already in your notes): `task/L196-simfast` (feature `ddruntime/inline-helpers` is still OFF unless you build with it; `signal_key` now
notifies after releasing the mutex, `DDRUNTIME_WAKE_INSIDE=1` = old) and `task/L193-decouple` (all its env knobs are off by default).

## 2. Opt-in knobs to A/B (all off by default)

Use the pinned config (`E:/ddda-re/bench/config.ini`), 3 runs each, fps mean over the same window, same save, and `game.lock`.
Order below = my expected value, highest first. Change one knob at a time first, then combine the winners.

| knob | what it does | try |
|---|---|---|
| `DDRUNTIME_WAIT_ALL_GATE=1` | the job join (`WaitForMultipleObjects(all)` on the workers' done events) is woken once, when all are set, instead of once per worker | 0 / 1 |
| `DDRUNTIME_WAIT_ALL_SPIN=1` | that join spins `event_spin()` before parking | 0 / 1 (with the gate) |
| `DDRUNTIME_JOB_WAKE_FIT=1` | a batch of `n` jobs wakes/joins only `min(participants, n)` threads (rest of the workers stay parked) | 0 / 1 (`DDRUNTIME_JOB_STATS=1` prints batch sizes) |
| `DDRUNTIME_RENDER_PRIORITY=1` or `2` | render worker above-normal / highest priority (it is preempted ~a fifth of the time) | 0 / 1 / 2 |
| `DDRUNTIME_EVENT_SPIN_ADAPT=1` | a wait for one event spins only while spinning on that event keeps succeeding (4 misses in a row turn it off; one wait in 16 probes again); keeps the spin where it helps, drops the wasted 20 us where the gap is longer | 0 / 1 (try before lowering the spin value) |
| `DDRUNTIME_EVENT_SPIN_US=N` (exists since L196) | how long a worker spins on its start event before parking; **only 150 and 400 were tried (worse); try shorter** | 20 (default) / 10 / 5 / 0 |

Why the spin value matters: with 7 workers, ~84 batches per frame and a 20 us spin each, the workers can spin for ~12 ms of CPU per frame on a
16-logical-CPU machine that also runs the render worker, GPU encode threads and the backend. Shorter spins free CPU for the render worker
but cost a wake-up (syscall) when the next batch comes quickly: the measurement decides.

Environment line to paste (PowerShell), e.g. all candidates together:

```powershell
$env:DDRUNTIME_WAIT_ALL_GATE='1'; $env:DDRUNTIME_WAIT_ALL_SPIN='1'; $env:DDRUNTIME_JOB_WAKE_FIT='1'; $env:DDRUNTIME_RENDER_PRIORITY='1'; $env:DDRUNTIME_EVENT_SPIN_ADAPT='1'
```

If a knob hangs the game (a lost wake-up would show as a stuck loading screen or a frozen frame): unset it. The gate and spin are the only ones that touch
synchronisation; the others only change counts and priority.

## 3. What to send back

A fresh `stacks_*.json` (`autorun3.sh`) from the best combination, and the `run_*.log`. Compare: render worker busy/cpu in the profile header, main
thread sync/wake share (`agg.py main`), fps at gameplay.

## 4. Known test flake

`threads::tests::an_abort_inside_the_window_releases_the_gated_threads` fails ~20% of full `ddruntime` lib runs, on a commit that predates these
changes too. It passes alone. Not a regression.
