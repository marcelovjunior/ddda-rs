# L108: nDraw map

Assigned to codex-10 by head via session brief, 2026-10-13. Branch
`task/L108-ndraw-map`, based on `task/fps-line`.

Scope: research and `docs/NDRAW.md`; no builds, game runs or Ghidra changes.
Map D3D9 owners, resource layouts, commands, thread/frame ordering, shader
binding and ranked native-renderer hook candidates. Commit in steps and
post each hash to head. Do not commit private research or raw dumps.

## Results

Delivered `docs/NDRAW.md`: 216 identified D3D9 call sites, 46 methods and
42 owners; all 11 requested state/resource identities/layouts; packed-state
decoding, command types 0..7, nested lists, executor context, worker/main
thread handoff, sorted scene/filter ordering, shader registers and five
ranked native-renderer hook boundaries with verified apply/executor ABIs.

Checkpoints: `c8ff6ef5` (initial map), `2ead4aaa` (state/resource layouts),
then the final command/frame/shader/census extension in this branch.
All 216 call addresses were checked as CALL instructions in their owner
disassemblies: zero mismatches. `git diff --check` passes. Evidence came
from read-only Ghidra and private DTI/census metadata; only authored
findings and short evidence snippets are committed.

No builds, runtime implementation/tests, game run or visual comparison
performed. The census is not an exhaustive escaped-pointer alias proof.
Complete configured Plug ordering, InputLayout descriptor details, named
texture eviction and CommandCache invalidation consumers remain explicit
gaps. L111 instrumentation is summarized as companion work, not validated
by this task. Head assigned tiny-target simulation tracing to L113;
no duplicate implementation or tracing is included here.

Milestone integration is deferred to head while another lane owns
`docs/MILESTONES.md` (board message 10585).

Suggested milestone entry: L108 mapped the nDraw object-to-D3D9 boundary
(216 sites / 46 methods / 42 owners), state layouts, commands, threading,
shader registers and hook ABIs in NDRAW.md. Static evidence only; no image
or FPS result. Next action: correlate L111 head-run target/type statistics
with a D3D capture and differential-test L112's executor port.
