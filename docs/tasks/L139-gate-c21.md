# L139: combo21 native gate and memory helper cost

Owner: codex-7. Branch: `task/L139-gate-c21`.

## Brief

Gate FPS line `335318fc` with the expanded native gate and next-batch
`f4bfb866` x87 discard, `4d53d724` FEDC00 and `546a052d` FED200 candidates.
Use the head's c21 profile to select helper/codegen work that also benefits
opt-level-0 generated callers. Never run the game. All shard builds use
`heavy.py`, `HEAVY_BIG=1`, four jobs, dedicated `C:/ddda-targets/L139`.

## Results

### Candidate integration

- Base: `335318fc`. Merged `task/L134-combined-gate`, retaining the expanded
  gate and x87 candidate. No earlier discarded helper experiment restored.
- FEDC00 depends on the FEDB80 test/recorder additions in `382e5f35`, absent
  from the base; dependency reported to head and included separately.
- Original candidate commits `382e5f35`, `4d53d724`, `546a052d` map to
  `9093d1ba`, `f6dacdcf`, `981fb77a`. Combined production pin: `981fb77a`.
- **GREEN** at 07:32:12: expanded play-profile gate, **100 passed, zero
  failed, seven timing benchmarks ignored**. All 33 generated crates rebuilt.
  Inventory passes: 59,450 / 59,469 non-platform functions; existing five
  coverage gaps and 17 owner-credited fragments remain.
- Per candidate: `f4bfb866` x87 discard **GREEN**; prerequisite `382e5f35`
  FEDB80 **GREEN**; `4d53d724` FEDC00 **GREEN**; `546a052d` FED200 **GREEN**.
  This is correctness acceptance, not measured gameplay performance.
- Required vector switches: `DDNATIVE_REQUIRE_VECTORS=1`,
  `MTDRAW_REQUIRE_APPLY=1`, `MTSTATE_REQUIRE_VECTORS=1`.
  Logs: `C:/ddda-targets/L139/gate.{out,err}.log`; start 07:22:02,
  end 07:32:12. Shared-machine elapsed time is not a build-speed comparison.

### Profile evidence and experiment selection

Head's private `profile_c21draw.txt` (SHA-256
`02732e5af0784063d474e6103daab2f409355baf1fb61fb748297747f4cb4940`),
30.1 seconds, 319 rounds: achieved
11 rounds/second, despite requested 40 Hz. Main thread has 180 busy
samples; `fast_u32` is 23.9% (43 samples), `fast_set_u32` 8.3% (15).
Job threads 0x1070 and 0x1074 show respectively 20.6%/8.4% and 24.8%/8.3%
for those same two helpers. These are sampled self costs, not call counts.
The profile's nearest-lifted grouping reports no frame because its symbol
recognition misses the mangled names; it is not evidence of no lifted work.

Flags and external dispatch are smaller secondary candidates. Main CF/ZF
self shares are 2.2%/1.7%; call_ext itself is 1.7%, with additional TLS
frames listed separately. Inclusive call_ext is not useful as a leaf cost.
The highest CPU thread remains wgpu-finish (74% CPU); head owns game/FPS
comparison, other agents own renderer work.

Selected experiment: compile existing SSE multiword memory helpers once
in the optimized base crate and inline their private dword implementation
within that crate. Preserve each permission/table lookup, atomic operation,
fallback call, address wrap and access order. No page-check coalescing or
cross-access pointer cache. Public scalar helpers remain out of line;
no `inline(always)` additions in generated-call interfaces.

Private baseline/candidate copies and opt0/opt1 caller drivers prepared
under `C:/ddda-targets/L139/emit-experiment`; no retained production change
or performance claim yet. Candidate oracle compares 2,304 synthetic cases
against the existing ordered scalar helpers, including fallback side effects.

Next: measure the isolated helper experiment and gate any retained change.
