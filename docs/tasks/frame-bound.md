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
