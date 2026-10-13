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
At this first checkpoint the shader port, enlarged coverage and timing
remained in progress. Step 2 below completes those items. No FPS or
image-equivalence claim; only the head runs the game.

### Step 2: complete shader wrapper and shared native sources

All four wrappers are implemented and registered in `mtdraw::abi::PORTS`
(already chained by ddports). Packed DA7510/DA77D0/DA7B30/DA7250 remain
calls through the original dispatch boundaries. The colour conversion
DA68A0 retains the lifted-call boundary. No fallback branch is replaced
by a guessed state or a synthesized default.

- **1,568 original ABI cases / 15,892 boundary calls**, each replayed by
  both the readable function and its unchanged extracted lifted body.
  Checks: all eight GPRs, all eight XMM registers, x87 control/status/depth,
  call targets/return addresses/arguments, bytes at each call boundary,
  and the final written-byte map. This follows L112's machine contract;
  EFLAGS and x87 instruction/data-pointer bookkeeping are not compared.
- Every feasible conditional edge is exercised. The only two untaken
  edges are the second null tests at DAAADB and DAAFAB: reaching either
  requires the same EAX already to be nonzero, and no intervening
  instruction changes EAX. Whole bodies include these equivalent paths.
- Unicorn does not set x87 precision status for the reciprocal/store
  sequence. Tests independently execute the recorded original
  FILD/FLD1/FDIVRP/FSTP sequence on the host, preserving its previous x87
  image, and compare the port and lifted body's status at each call and
  return. Original Unicorn data/GPR/XMM/memory checks remain exact.
- **811 L117 original cases** validate the pure object decoders' stream,
  surface, packed fixed-state, shader, texture and constant arguments.
  L117's complete **3,584-case / 17,928-call** oracle and four contract
  tests still pass after extracting shared constant range/dimension helpers.
- ddports registration/mod-replacement test passes. Runtime light serial
  `--no-fail-fast`: **171 passed, one existing fullscreen gamma Reset
  failure, three ignored**. Strict no-dependency clippy passes with only
  the existing `executor.rs` single-match lint allowed; no new lint
  suppression is added to source. Dependency-inclusive strict clippy hits
  pre-existing ddnative-base warnings.

Pure API (agreed with claude-2 in board #11041/#11050/#11196/#11200):

- `apply::decode_targets(m, binding)`: selected raw viewport/scissor,
  dimensions and view/surface identities. Surface formats are resolved
  from runtime objects; view format storage is not always initialized.
- `decode_fixed`: identities/hashes plus packed blend/depth/raster payloads,
  winding flip, blend RGBA and stencil reference; use L117 `mtstate::packed`.
- `decode_streams`: four object/handle/byte-size/offset/stride records,
  index object/handle and instancing flags. Declaration comes from shaders.
- `decode_shader`: metadata/declaration/VS/PS identities, value array and
  descriptor ranges. `.textures(m, values)` and `.constants(m, values)`
  are allocation-free iterators in original descriptor order. A constant's
  `.range()` uses the shared L117 tagged-pointer cap/conversion decoder;
  `.dimensions(m)` uses the shared dimensions/reciprocal decoder. Unknown
  derived kinds and counts above one require fallback because the original
  reads retained/adjacent stack scratch. The ABI wrapper preserves them.

### Measurement

`python research/native/check_apply.py --bench` builds only four extracted
bodies privately. Lifted code is opt-level 1, readable mtdraw is opt-level 3;
ddnative-base/host/mtcore are opt-level 3. Real PageTable-backed memory,
synthetic deterministic callee mocks, 5,000 calls per sample, three
alternating pairs. Fixture reset and CPU construction are outside timing;
timer overhead and calls are included. No D3D9 backend/GPU/whole-frame work.

Final-run medians (microseconds per apply):

| Wrapper | identity lifted/readable | transition calls | transition lifted/readable | most calls | most-call lifted/readable |
|---|---:|---:|---:|---:|---:|
| targets | 0.062 / 0.041 | 1 | 0.651 / 0.167 | 7 | 0.800 / 0.480 |
| fixed | 0.091 / 0.082 | 5 | 0.263 / 0.158 | 6 | 0.372 / 0.208 |
| streams | 0.061 / 0.041 | 4 | 0.472 / 0.243 | 5 | 0.662 / 0.254 |
| shaders | 0.066 / 0.042 | 5 | 1.395 / 0.412 | 96 | 29.484 / 17.949 |

The 96-call shader samples are unstable: lifted/readable pairs
11.563/17.949, 29.484/17.875, 33.939/36.499 us; two pairs regress.
**No reliable improvement is established for that constant-heavy case.**
Earlier transition measurements also varied under shared-machine load;
these microbenchmarks do not predict an FPS improvement.

Reproduce original recording through the heavy slot:
`E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex-10 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/record_apply.py`.
Fixtures live only in `E:/ddda-re/notes/native-L125/apply.jsonl`; JSONL
replay keeps one case in memory. The independent lifted runner is
`python research/native/check_apply.py`. No generated Rust, vectors or
game bytes are committed. No game, shard or GPU run was performed.

Next action: head integrates both commits, checks boot and the same save
against the previous build, then records a matched frame/profile. L119
can adopt the pure decoders independently of the ABI override path.
