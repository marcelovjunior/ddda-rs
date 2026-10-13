# L82: render-path guest-lock waits

Owner: codex-3. Branch: `task/L82-render-lock`, from `task/opt-int`.

## Brief

Instrument render-thread services per frame, contention frequency and lock
holders. Remove only proven unnecessary serialization, preserving guest
ordering. Report before/after synthetic contention measurements and pass
`cargo test -p ddruntime --no-default-features`. Only head runs the game;
no shard builds, game data, private research artifacts or pushes.

## Evidence and implementation

- `profile_gameplay_render2.txt` identifies render guest thread `0x1034`;
  the assigned profile attributes about 8.8% inclusive to lock acquisition.
  This is a sampled profile, not a measurement of this patch's FPS gain.
- D3D objects/device state use `Runtime::d3d: GuestCell<Objects>`. It is
  shared, not render-thread-local, and includes COM reference counts and
  resource lifetimes. Blanket `Binding::LockFree` would violate its safety
  contract. Keep state lookup, snapshots, resource flush and command-order
  selection under the guest lock.
- Ordinary draws and Clear perform only backend work followed by CPU/stack
  return after their final command is prepared. The final backend mutex can
  remain held while dropping the guest lock through service return. This
  removes the unnecessary final acquire/adopt/release. UP draws still reset
  bindings/release references afterward and must reacquire.
- `signal_preempt` enabled the lifted-call slow hook and scanned suspended
  threads twice per acquisition even though `preempt` already refuses to
  yield after `ANY_PARALLEL` becomes true. Apply the identical guard before
  computing/enabling that hook. Serial-only behavior is unchanged.
- `DDRUNTIME_LOCK_STATS=1` adds per-thread/service acquisition counts,
  condition-variable blocks and service time; blocker edges identify the
  waiting and holding guest thread/service. Holder lookup uses host thread
  identity so simultaneous sessions' reused guest IDs cannot misattribute
  the holder. A Present call (including a failed call) delimits per-thread
  frame reports of calls/blocks/wait/service microseconds. Pre-first-Present
  initialization is included in frame 1. Nested timings are inclusive;
  counts of blocks include reblocks after a wake. Existing wait duration
  includes uncontended acquisition overhead, not only blocked time.

## Results

Render-tail checkpoint complete. Deterministic synthetic scenario, statistics on,
debug test profile, 32 calls, second thread holding the lock for a synthetic
4 ms during each render submission:

| Measurement | Before | After |
|---|---:|---:|
| Guest-lock acquisitions | 64 | 32 |
| Render condition-variable blocks | 32 | 0 |
| Total acquisition wait | 140.959 ms | 0.0137 ms |
| Mean measured service call | 4.479 ms | 71.496 us |

The fixture waits for the worker to finish outside the measured service
before starting the next frame; it isolates the removed tail wait and is
not a workload/FPS forecast. Channel receives have a five-second timeout.
Assertions use acquisition counts, block counts and stack return, not a
wall-clock speedup threshold. Serial/direct/nested ownership is checked.

Initial full light-suite attempts hit existing L64/L65 benchmark sample
count assertions under shared-machine load (the faster path had lower
latency but fewer samples). No assertions were weakened. Final full light
suite: **138 passed**, using `-- --test-threads=1`. The headless real-COM
regression verifies one acquisition each for DrawPrimitive,
DrawIndexedPrimitive and Clear, two for UP, and unchanged recorded commands.
Nested/direct/serial ownership, unwind cleanup and parallel preemption
policy pass. No game or GPU was run.

The stale draw-file claim was avoided: `Binding::ServiceTail` is registered
for IDirect3DDevice9 slots 43/81/82 in `d3d9dev::install`. Normal dispatch
and the step executor use the same policy. `d3d9draw.rs` is unchanged.

Head redirect #10190: gameplay measured 1,284,189 GetAsyncKeyState calls in
120 seconds and 14,719 ms guest-lock wait. Next action: atomic input polling
snapshots, preserving hook suppression and per-thread queue key state.
`threads.rs` is frozen for claude-2's L86 work. Gameplay/FPS comparison of
the render-tail checkpoint remains for head.
