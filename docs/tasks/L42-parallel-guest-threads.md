# L42: parallel guest threads (lifting the one-thread-at-a-time limit)

- **Owner:** claude-4
- **Branch:** `task/L42-parallel-threads` (worktree `E:\ddda-rs-wt\L42`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md), merge-train rule

## Why

`crates/ddruntime/src/threads.rs` runs one guest thread at a time (the
guest lock). Threads hand over only in services: waits, contended
critical sections, Sleep, and API calls after a 2 ms slice. That model is
exact and simple, and it was the right first step. It also means the
game's worker threads never run in parallel. sMain's job threads
(`mJobThreadNum`; FRAME.md section 3: units on a line run as jobs), the
renderer worker 0x00DAC090 and the resource loaders all take turns on one
core. At gameplay that caps the frame rate, which the player cares about.

The lifted code's synchronisation is all imported calls (Interlocked*,
critical sections, events, semaphores; docs/RUNTIME.md, threads row).
Guest memory is shared. So true parallelism may be possible without
changing lifted code, if the memory model and the runtime's own state
allow it.

## Task

1. **Analysis** (Results):
   - Which runtime state is touched from guest threads (services, the
     registry, GuestMemory's page tables, lazy mapping, the D3D9 layer,
     audio) and how it is locked.
   - What x86's memory model (TSO) requires of the lifted code's plain
     loads/stores when it runs on the host. Rust compiles them as
     non-atomic accesses through `Mem`. Check whether x86-64 hardware
     gives TSO for them as emitted, and where compiler reordering could
     break it. The game's lock-free patterns, if any, decide this: find
     any `lock`-prefixed instructions and the lifter's translation of
     `xchg`, `cmpxchg` and `xadd`.
   - Which threads would benefit, and how much (job threads per frame).
2. **Design:** parallel guest threads, with a fallback to the current
   exact mode by a switch. Keep the exactness arguments explicit.
3. **Prototype:** behind the switch; tests: the existing thread stress
   test in both modes, plus new tests that run two guest threads
   concurrently on Interlocked counters and critical sections.
4. **Measure:** the static initializers and a synthetic job workload,
   serial vs parallel.

## Results

(open)
