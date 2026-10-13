# L119 (N2): the native path through the nDraw executor

Assigned by the head to claude-2 (2026-10-13; board #11087 for the split
with codex-9's L126 worker). Branch `task/L119-native-executor`.

## Brief

Put a sink behind the executor port's device-facing actions
(crates/mtdraw, L112). The default sink is the exact port. A native sink
(`DDDRAW_NATIVE=1`) takes the main 1920x1080 scene passes' draws: state
objects decoded through crates/mtstate (L117), identities = object address
plus hash at +0x0C, sent as `d3d9cmd::NativeJob`s through the same
DeferredBackend FIFO (L126, codex-9 owns transport and worker). Everything
else falls back to the D3D9 path; the runtime's D3D9 state stays
consistent. Verify pixel equality per pass (DDDRAW_NATIVE=0 vs 1) and
measure the per-draw cost.

## Design

- `mtdraw::sink::CommandSink` (crates/mtdraw/src/sink.rs) is offered every
  record the executor dispatches, after a pending merged draw was decided;
  nested lists are walked by the executor. A handled record skips its
  applies and device calls: neither the device nor the executor context's
  applied-state record changes, so the default path and the 213 direct
  D3D9 call sites see a consistent device.
- `crates/ddruntime/src/ndraw_native.rs` decodes a draw record by running
  mtstate's four apply semantics against a zeroed scratch context (the
  "nothing applied" branches emit every call of the draw's state) and
  folding the calls into the state the device would hold. From that it
  builds the `DrawCall` the D3D9 path makes (same shader/declaration Arcs,
  same texture bindings and sampler states, constants, targets, viewport)
  and, for `DDDRAW_NATIVE=1`, a `NativeDraw` with the same content.
- A draw is taken only where the full-state decode equals what the delta
  path leaves on the device: every register and sampler the shaders read
  is set by the draw's own binding; no blend-factor blending (RS193 is
  only updated when the BlendState pointer changes); the viewport is not
  the "invalid" one. Refusals are counted by reason.
- Unresolved (labelled in the code): the sampler filtering option
  ([0x018D1D20]+0x34, values 1..5) overrides anisotropic samplers only in
  0x00DA7250's non-null branch; a taken draw assumes that branch, which
  differs on the first use of a stage in a frame.
- Native jobs: consecutive taken draws into the same targets form a pass;
  the passes since the last default command form a job, sent before that
  command or at the frame's end. A rejected job is sent as its DrawCalls.

## Results

- Step 1 (0c3edc68): the sink. Differential test: no sink = the lifted
  executor (400 frames); a sink taking nothing changes nothing; a sink
  taking 1,787 draws gives the lifted result with those records as no-ops.
- Step 2a (2ca6e0c1): `DDDRAW_NATIVE=census`.
- Step 2b: `drawcall` and `1` modes. Tests: unit tests of the decode, the
  shader read sets and the census; `tests/ndraw_native.rs` (needs the
  installed exe) builds real D3D9 objects and nDraw objects, makes the
  apply calls on the runtime's device and draws, and checks the sink's
  DrawCall equals the D3D9 path's (render states: the backend-read subset
  with equal values; constants: every register the shaders read), and the
  NativeDraw/pass grouping built from it.
- combo13 (head run, d5bc371d, `drawcall`): 3,425 scene draws taken per
  frame, 134 refused (90 instanced, 44 FloatsUncovered); images normal;
  but 22-28 us per decided draw (7.9 fps avg vs 14.7 default): ~38% of
  the render thread was heap allocation (mtstate's Vec per call, every
  draw decoded from scratch).
- Step 2c: decode cached. Across frames by identity: fixed-state objects
  (address + hash at +0xC, raster variant), shader metadata (address;
  its shaders/declaration re-checked against the runtime once a frame),
  SamplerStates (address + hash), shader read sets, render-state lists.
  Per frame by address: target, stream and shader bindings (the shader
  binding with its textures and both constant banks assembled). A draw
  does four lookups, its own parameters and the flush; guest reads go
  through the page table. Debug-build cost from the caches: 2.5 us per
  draw (oracle test). Also: RS15 for an alpha-test-off binding is 0 (the
  executor's context init sets 15/24/25 each frame, so the device holds
  what the last apply set), not the device's value.
- combo15 (head run, step 2c, `drawcall`): 14.2 fps avg, 15.3-16.0 at the
  end, equal to the default path; the render thread is no longer the
  bound (65% busy, 90% of its waits on the start event).
- `DDDRAW_NATIVE=verify`: the D3D9 path draws; every DrawCall it makes
  for a scene draw the sink would take is compared field by field with
  the decoded one (render states, the registers and samplers the shaders
  read, streams, indices, shaders, declaration, viewport, scissor when on,
  targets); the report counts comparisons, merged draws, and mismatches by
  field (the first 20 printed). Hook: d3d9draw's DrawPrimitive and
  DrawIndexedPrimitive services call `ndraw_native::observe` (one relaxed
  atomic load when not verifying).
- combo17 (head run, `verify`): nearly every draw's samplers differed:
  0x00DAA8B0's sampler section depends on the command history (filtering
  option 3: anisotropy 4 vs 16; textures applied by object). Step 2e
  (ad821975): taken draws make that section on the executor context and
  the device (a port tested against mtstate::shader on 3,000 random cases)
  and take textures and sampler states from the device; returning to the
  default path clears the context's last fixed/shader bindings and writes
  the taken draws' constant uploads to the device.
- combo19 (head run, 4bcef3fa, `verify`): ~420,000 compared per 10 s, all
  equal, no mismatch; ~7,700 merged by the D3D9 path (sRender+0x30).
- Step 2f: `drawcall` is the default (`DDDRAW_NATIVE` unset); `0` or `off`
  turns the native path off. The summary prints only with the mode set
  or `DDDRAW_NATIVE_EVERY`.
- Not yet: the per-pass pixel comparison of native jobs (`compare`, L127).

## Frame threads (question from the head, 2026-10-13)

Read-only reading of the executable (addresses as in docs/NDRAW.md) and
the combo15 profile (E:/ddda-re/notes/runs/profile_c15b.txt):

- 0x00DBE560 is not the render handshake. It is sMain's parallel-for:
  it sets each participating worker's start event, runs jobs itself
  (0x00DBD8D0: a shared counter taken with InterlockedIncrement over the
  job array at sMain+0xB8), then WaitForMultipleObjects on the workers'
  done events (wait-all, INFINITE). 0x00DBDF30 creates the seven workers
  (ideal processors 1..7); their loop is sMain::vf20 0x00DBDC90 (same
  job loop). Callers include sRender's scene functions (0x00DB63E0,
  0x00DB7350, 0x00DB67B0), the command sort 0x00FEE130 and game
  systems (0x004792C0, 0x004AA270, 0x0116BAB0, ...).
- The render handshake is sRender+0xD0 (start) / +0xD4 (done). The main
  thread kicks the render worker 0x00DAC090 in 0x00DA9CD0 (flip the
  command slot, ResetEvent(+0xD4), SetEvent(+0xD0)) and, before Present,
  0x00DAED10 only polls +0xD4 (WaitForSingleObject timeout 0) while the
  frame-time budget lasts; if the render frame is not done it returns
  without Present and keeps the frame pending (no new kick until done).
  It blocks on +0xD4 only in 0x00DAECC0 (called from 0x00E10410 and
  0x00E127D0, shader-table changes) and in the synchronous mode. So the
  main thread already builds frame N+1 while the render thread runs N:
  the original's overlap is in place in our runtime.
- In combo15 the main thread's waits are 99% in 0x00DBE560's wait-all on
  the job workers (0x1070..0x1084, each 33-46% busy), none on +0xD4; the
  render thread waits on the main thread. The bound is the main thread's
  lifted game code plus the job join.
- Proposal (not implemented): measure per 0x00DBE560 call the job count,
  the main thread's share, each worker's wake latency and job time, and
  the join wait (a wrapper over the lifted function, as L111 does for the
  executor), to tell a long tail job (only faster lifted code helps) from
  wake latency (our event wait: the workers show 7-12% self time in
  wait_for_single_object). Readable ports of the hottest jobs are the
  other lever. No game-visible change is needed for either.

## L141: instanced draws in the sink (2026-10-13)

- The sink takes executor types 2 and 3 (51f5cc0c): types 0 and 1's
  DrawCall with the streams 0x00DAB480's frequency loops leave on the
  device (instance data 0x80000001 at step * stride + offset, geometry
  0x40000000 | instances). Oracle test: a type 3 DrawCall equals the
  runtime D3D9 path's after the stream apply and those calls.
- A taken one changes nothing on the device: the executor sets every
  frequency back to 1 and the instance sources back to the stream apply's
  after the draw. That holds only if nothing else sets frequencies. Static
  check (DDDA.exe analysis copy): the only code with an instancing
  immediate (0x80000001, or OR 0x40000000) within 256 bytes of a vtable
  +0x198 (SetStreamSourceFreq) load is 0x00DAB875..0x00DABAB0, the
  executor's own; 19 other sites have one without the other. A heuristic,
  not a proof.
- Not yet compared in game: needs `DDDRAW_NATIVE=verify` (0 mismatches).
