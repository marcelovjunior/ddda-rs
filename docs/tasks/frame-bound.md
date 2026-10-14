# What bounds the frame (shadow agent, 2026-10-14; benchmark save, build = L172 tree + L180 e5f70e9c/463253d5 + L190 ports)

Metric: mean stage-probe fps over t=46-84 s, unfiltered (the >45 filter leaves 1-20 probes now). Runs ~56-60 unfiltered, +-3 noise.

- Merging claude-2's e5f70e9c + 463253d5: 57.7 / 58.7 vs serial baseline 57.5 / 57.2 (kept).
- L190 ports (FA5860, B8B800) merged: 56.8 / 60.3 (within noise, kept).
- L191 haiku-trial ports (886880, 885dc0, 84c720, 886530): **crash the game at start** ("stopped: guest read of 1 byte at 0x00012626", or a guest thread reading 0x12000008). Skipping all four restores a normal run; skipping 886880 alone, or the other three alone, still crashes, so at least two are wrong. Not merged here (merge reverted); head should have the L191 agent fix them.
- DDRUNTIME_SHADOW_PRIMITIVE=1 + DDRUNTIME_SHADOW_TOP=1: panic at ddnative-base x86.rs:251 (unusable). DDRUNTIME_JOB_LOOKUP=1 + SYNC_FUTEX=1: 61.1 / 59.4 vs base 56.8 / 60.3 (noise, not shipped).

## Critical-path breakdown (DDRUNTIME_CRITICAL_JOBS timeline, 2396 frames)
The frame's big job batch (298 queued jobs, mode 1): mean 5.09 ms. The shadow job (DB7620, vtable 0160DF98) is the last to finish in
every frame but is NOT claimed last: it starts at +0.06 ms (queue index 10-17) and runs 5.0 ms on one worker; all other jobs end by
+3.26 ms. So the shadow job alone extends the batch by ~1.75 ms (~10% of a ~17.5 ms frame). That matches the "no traversal at all"
ceiling (skip mode: 64.5 / 62.3 vs 57.5 / 57.2 fps). Moving the shadow job to the front of the queue (DDRUNTIME_JOB_SHADOW_FIRST=1,
job_order.rs) changes nothing: 55.6 / 55.4 vs 55.9 / 55.3.
The shadow job = ~10 EE2C90 calls per frame (one per cascade/light, 0.45 ms avg, 16.3 s per run), of which the nested
DB63E0 traversal is ~75%.

## Thread load (sampling profile, 30 s, heavy window, ~40-55 fps)
| thread | CPU | note |
|---|---|---|
| render worker 0x1034 | 22.6 s (75%), waits 26% | draw_call 24% self, Mirror::textures 6%; NOT saturated now, becomes the next bound at ~57 fps |
| main | 16.7 s (56%), waits 44% (94% inside DBE560 = the batch join) | passes A/B/C 22%, render pass daf000 30% |
| job workers | 9-13 s each, 60% waiting | idle during the shadow tail |
| d3d9-backend / wgpu | 10 / 7 / 4.5 s | not on the critical path |
So: main waits ~5 ms/frame for a batch whose tail is one serial shadow job; the rest of the frame is main-thread serial lifted
work (passes + render pass) and the render worker keeps up with ~25% slack. A >=3% gain needs ~0.6 ms off the shadow job (faster
units inside it) or off the main thread's serial passes; there is no scheduling fix (the job is already first). Profile with
symbols: C:/ddda-targets/L172/runs/profile_prof3.txt (autorun3.sh; the symbols were unresolved because ddplay.pdb in the run dir
was older than the exe: copy the pdb from deps after every build).

## Round 2 (symbols fixed, tables, one fix)
Profiling procedure: `l172_replay_build.sh --ddplay` now also copies `ddplay.exe` AND `ddplay.pdb` from `deps` into
`C:/ddda-targets/r172/runs/shadow2/` (a stale pdb is why round 1's table was unresolved). Profile with
`autorun3.sh` after exporting the launcher env (it has none of its own); the per-stack re-attribution script is
`scratchpad/stk.py` logic: filter `stacks_*.json` samples by guest chain (e.g. contains 0x00ee2c90), 1 sample =
wall/rounds ms, divide by frames. Tables from `profile_prof4.txt` / `stacks_prof4.json` (~50 fps phase, 1500 frames):

Main thread, serial (not waiting): 12.7 ms/frame CPU. Self (leaf): `fast_u32` 1.80, `fast_set_u32` 1.03 (memory helpers of lifted code),
`d3d9dev::presentation` closure 0.72, `semantic::dispatch_event` 0.46, `fast_u8` 0.41, `dispatch install closure` 0.31,
`x87::real_memory_dyn` 0.21. Nearest lifted function (self + host callees): 0x00dbe560 1.21 (DBE560's own join/wake/spin machinery),
0x00daed10 1.16 (Present wrapper; 0.72 of it was the closure above), 0x00ddbdc0 0.46 (dispatch_event), 0x011318f0 0.23,
0x0109d480 / 0x01077500 / 0x00f75520 / 0x00f20640 / 0x00fe64f0 0.13-0.15 each.
Shadow job (samples whose stack contains EE2C90, all threads): 3.9 ms/frame CPU, self `fast_u32` 0.44, `fast_set_u32` 0.28,
`shadow_traversal::traverse` 0.10, `apply_pass_state` 0.10, begin_material/primitive_material 0.08 each; nearest lifted:
0xefdc70 (traverse/apply_pass_state/begin_material) ~0.5, 0xfeef20 (shader_lookup/commit_primitive_state) 0.3, 0xc86520/0xf1f2b0
(set_pass_ats) 0.23, 0x8868e0 model_upload_state 0.10, 0xffe5c0 draw_model_mesh 0.10, 0x113fd00/0x113d520/0x113c590 (cull_bounds)
0.2. It is a flat profile: no function over 0.2 ms/frame, so no single port gets 0.6 ms.

**Fix shipped in the tree (commit "Present's presentation()"):** `d3d9dev::presentation()` scanned every live D3D object each Present
(0.72 ms/frame of main thread, visible as the hashbrown iteration in the profile); it now reads a cached device. After it the closure
is gone from the main thread's profile. fps: 55.7 / 59.4 / 59.6 (median 59.4) vs 56.8 / 60.3 before = ~+2%, inside the noise, so not
shipped as a launcher (needs >= 3%).

DDRUNTIME_SHADOW_PRIMITIVE=1 alone runs fine (58.7 / 58.2, no change); the panic (ddnative-base x86.rs:251) is DDRUNTIME_SHADOW_TOP=1.

**Candidate list for port agents** (ms/frame = CPU, main thread unless noted):
1. DBE560 join/wake self 1.2: the batch join in `wait_for_multiple_objects`/`signal_key` (runtime, not a port): try a short spin
   before sleeping on the done events / avoid the extra SetEvent+WaitOnAddress round trip (7.5% RtlWaitOnAddress self).
2. Render worker (0x1034, now 63-77% busy, the next bound): `ndraw_native::draw_call` 21.8% self (~2.8 ms), `mi_malloc_aligned`
   5.3% (~0.7 ms: per-draw allocations in the sink; pool/reuse), `Mirror::textures` 4.7% (~0.6), `shadow_samplers` 2.5%, hash lookups
   (`contains_key`, `equivalent<u32,u32>`) ~4% (~0.5 ms; FxHash maps -> direct-index tables). Each is L180's.
3. `fast_u32`/`fast_set_u32` 2.8 ms main, 0.7 ms in the shadow job: generic lifted memory helpers; any port that removes lifted bodies
   shrinks them (largest lifted users in the main profile: 0x011318f0, 0x0109d480, 0x01077500, 0x00f75520, 0x00fe64f0).
4. `semantic::dispatch_event` (0x00ddbdc0) 0.46: port/inline the event dispatch.

## Round 3
- DDRUNTIME_JOB_INLINE=N (job_order.rs; top-level batches of 2..N jobs run on the caller, skipping the SetEvent/WaitForMultipleObjects round trip of
  every enabled worker): N=3 56.3/60.2, N=8 44.8 (harmful) vs 60.4/59.5 control. Off by default; the wake path is not a cheap win
  (DDRUNTIME_JOB_STATS=1 prints batch time by queue size). The remaining DBE560 self time is the join wait itself.
- L191 (c6c8df77) merged: 885DC0 (obj_model_vf08) still crashes the game at start (WinMain read 0x00012626); 886880, 84C720, 886530, 1165B40
  run clean (59.8/59.8). fast2 ships them with 885DC0 in DDRUNTIME_SEMANTIC_SKIP.

## Round 4: render worker (profile_prof5, 0x1034 busy 77%)
Self items: draw_call 21.7% (one 600-line function with the cache lookups inlined: no line-level profile available here),
memcpy (VCRUNTIME 0x1c010/0x1bc50/0x1e570) ~9% (callers: draw_call 46%, Native::draw 22%, `[Constants;2]::default()` 20% = the per-draw
`mem::take(&mut c.banks)` scratch), Mirror::textures 4.8%, hashbrown u32 lookups ~5% (kept_part target/stream, Caches::meta, contains_key
in the dirty check), mi_malloc 3.3%, `Instant::now/elapsed` ~4% (two QPC reads per taken draw).
Done: the clock reads are gated by DDRUNTIME_NDRAW_STATS (commit "L180: no clock reads per taken draw"); fps 58.5 / 58.9 / 60.0 vs 59.8 / 59.8
(noise: the worker is not the bound in the measured phases). Shipped in fast2.
Left for whoever has a line-level profiler (perf/VTune/`--profile` with `debug = line-tables-only` + dbghelp SymGetLineFromAddr):
split draw_call into non-inlined phases to attribute its 21.7%; replace the scratch `[Constants;2]` take/default with a reused pair;
per-draw Arc clones (render_states, textures, targets array clone, vertices) -> one Arc<DrawParts> per kept binding.
