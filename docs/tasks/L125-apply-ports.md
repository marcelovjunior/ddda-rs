# L125: nDraw apply ports

Assigned to codex-10 by the maintainer, on task/L125-apply-ports from
task/fps-line. Scope: whole ABI ports of DA6FE0 (targets), DAA5F0 (fixed
state), DAA7C0 (streams), DAA8B0 (shaders/constants), using L117 mtstate
semantics. Preserve original registers, xmm/x87, memory writes and calls.
Private Unicorn vectors and isolated lifted-body differential tests are
required before PORTS registration. No game or shard builds/runs.

API agreement: claude-2 board #11041, #11050. Pure object decoders in
mtdraw::apply; no context cache reads or calls. L119 owns sink.rs and
executor.rs. Apply ABI wrappers retain the D3D9 fallback cache contract.

## Results

Step 1: targets, fixed state and streams implemented with pure binding
decoders and ABI wrappers. **384 cases** compare each readable wrapper and
its extracted lifted body separately with original Unicorn results: all
GPRs, XMMs, x87 control/status/depth, final written bytes, ordered call
targets/return addresses/arguments/registers. Packed helpers are mocked
boundaries (their call semantics were independently checked by L117).
`research/native/check_apply.py` extracts only four existing generated
bodies into a private small Cargo workspace; no shard build is needed.
Shader port, enlarged coverage and timing remain in progress. No FPS or
image-equivalence claim; only the head runs the game.
