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

## Input convoy redirect results (head #10190)

Implemented `Binding::LockFree` for GetAsyncKeyState, GetKeyState,
GetKeyboardLayout, MapVirtualKeyA and ImmGetVirtualKey. No further changes
to threads.rs after render-tail checkpoint `3ce658f5` (L86 owns that file).

- `HostInput.events` is now a cloneable `EventSender` with the same `send`
  call shape. Successful sends publish an atomic pending-event flag after
  enqueueing. The host does not touch GuestCell or guest memory. A racing
  pump may drain twice but cannot lose an unpublished channel event.
- GetAsyncKeyState reads/consumes an atomic accepted-key snapshot when no
  events are pending. Pending events alone enter `threads::with_lock` to
  run the existing pump, including hooks on their owning guest thread.
  Deferred hook events keep the pending flag set. Publishing raw physical
  keys immediately on the host would break suppression and the hook's
  required pre-update observation, so accepted keys publish after hooks.
- Generic/left/right modifiers share one atomic word, preserving exact
  shared pressed-bit consumption across concurrent generic/specific polls.
  This also eliminates the previous per-poll temporary Vec allocation.
- GetKeyState reads a cached per-session/per-guest-thread atomic queue
  snapshot. Only message removal updates it, including toggle bits and
  modifier aliases. The snapshot map has its own mutex on cache misses;
  the hot path never takes the guest lock. Host physical input does not
  bypass queue semantics.
- GetKeyboardLayout, MapVirtualKeyA and ImmGetVirtualKey use constants,
  local tables, CPU and guest stack only. XInput/DirectInput/device and
  message-queue mutation services retain their current synchronization.
- `ddplay/src/input.rs` only changes its three event-sender type annotations
  and import. No game/shard build or run was made; head verifies integration.

Busy guest-worker scenario (statistics on, debug profile, 32 polls each,
synthetic 4 ms locked work): the same snapshot poll bound as Service versus
LockFree gives **32 -> 0 acquisitions**, **32 -> 0 blocks**, total acquisition
wait **142.0 ms -> 0**, mean call **4.461 ms -> 15.003 us**. A real guest
worker verifies independent queue-key state. New key-up is processed on
the slow path; queue state changes only after removal. This isolates lock
policy cost and is not a gameplay FPS claim.

Validation: **141 tests pass**, full
`cargo test -p ddruntime --no-default-features -- --test-threads=1`, jobs=4,
incremental off, `C:/ddda-targets/L82`. Existing hook suppression, queue
state, extended modifiers, mouse and disconnect cases pass. A 20,000-step
reference-model test matches the previous locked key behavior; concurrent
generic/right-modifier polling consumes the press edge exactly once.

Next action: head merges both L82 commits and compares gameplay with
`DDRUNTIME_LOCK_STATS=1` / `DDRUNTIME_LOCK_STATS_EVERY=120` (per-thread frame
reports are verbose), then measures FPS with diagnostics off. New pending
input still intentionally takes the guest lock; unchanging polls should
have zero acquisitions. Original-game/input-feel comparison remains open.
