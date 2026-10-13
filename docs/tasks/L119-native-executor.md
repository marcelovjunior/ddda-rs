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
- Not yet: the per-pass pixel comparison.
