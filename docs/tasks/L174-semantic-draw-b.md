# L174: semantic material draw and dependency hash

Assigned to codex-10 by head; branch `task/L174-semantic-draw-b`, worktree
`E:/ddda-rs-wt/L174`. Base `task/fps-line`, fast-forwarded to the requested
L171 template `a4a4791d`. Follow `AGENT-RULES.md` and `NATIVE_PIPELINE.md`,
with the head's current ABI-level semantic-port and isolated-train policy.

## Scope

Replace `00E0E5E0`, `010655A0`, and `01066C60` with clear native Rust using
`ddnative_base::mem::Pages`. Keep the original guest callees at explicit
boundaries. Register under `DDRUNTIME_SEMANTIC=1`; no default change.
No game, GPU test, shard build, push, installation write, or shared Ghidra
mutation. Private vectors and extracted original/lifted code stay private.

Owned source/test: `crates/mtcore/{src,tests}/semantic_draw_b.rs`.
Recorder/checker: `research/native/l174_{vectors,check}.py`.
Additive module/registration/documentation hunks coordinated on the board;
head resolves the other semantic branches' corresponding additions.

## Implementation and evidence

- `dependency_hash`: whole salted word FNV-1 loop, original floor(length/4)
  and wrapping arithmetic. Multiplier/salt are executable immediates at
  E0E603/E0E609. Retains the three saved-register writes because the existing
  oracle includes an input pointing at those slots. EAX and ESP are the
  only changed ABI registers. Supersedes `DDRUNTIME_JOB_HASH` when both
  switches are set; existing deduplication gives one override per address.
- `apply_material`: material dependency key, color packing, render flags,
  old-state command recording and twelve-byte override records. Thin ABI
  adapter calls `apply_material_state`, then original `01062F20`. Reads
  float constants from guest image memory. COMISS unordered handling and
  hardware CVTTSS2SI conversion preserve NaN/overflow behavior; no invented
  color clamp. Loop count is reread because writes can affect source data.
- `select_material_pass`: opacity rejection, optional resource lookup,
  normal/special pass state, texture stamps, family selection and fallback
  pass logic. Keeps the original vf09/vf14 and direct calls with their
  original stack addresses, arguments, this pointers and resume addresses.
  Guest CPU state is confined to the adapter and call helper; the selection
  logic uses named values and returns its result. No per-call allocation.

Source: read-only Ghidra disassembly/decompilation of the three addresses,
and original instructions from the player's analysis executable. Existing
hash port and vectors are L158 work; no private content is committed.

## Results

Implemented and checked against original **synthetic** instruction vectors:

| Function | Original returns | Instruction coverage | Replay |
|---|---:|---:|---|
| E0E5E0 | 528 (existing L158) | existing L158 corpus | sparse, serial Pages, atomic Pages |
| 10655A0 | 204 (new) | 197/197 | sparse, serial Pages, atomic Pages |
| 1066C60 | 436 (new) | 377/377 | sparse, serial Pages, atomic Pages |

All **1,168 cases** pass in each memory mode. The isolated opt2 lifted draw
bodies independently pass the same **640** draw cases. Five isolated test
functions pass, two benchmarks ignored in that correctness invocation.
Instruction coverage is not a claim of exhaustive input/path coverage.

The ABI oracle compares EAX, EBX/ESI/EDI/EBP, ESP, all original live writes,
and rejects additional live writes. Only bytes in the fixture stack below
returned ESP are excluded: **heap addresses below ESP remain checked**.
Page replay scans complete backed fixture pages to detect additional fast
writes. At every external call it compares target, this, stack address,
return address/arguments, initialized object fields and previous live writes.
It replays documented synthetic callee effects and volatile clobbers. These
are caller/seam tests, not proof of the callee implementations.
An additional mutation test rejects five deliberately broken variants:
EAX, EBX, ESP, a changed heap output below ESP, and an extra heap write.

Cases cover dirty/equal state, command recording on/off, empty and 511-entry
override lists, byte tails/unaligned/page-edge/stack-alias hash inputs,
signed pass overrides, optional resource-chain nulls, every family dispatch,
callee AL zero/nonzero, aligned frames, threshold/equality cases, infinities,
NaNs, four rounding modes and denormal flush/input modes. The first draft
oracle included dead caller argument bytes in call snapshots; fixed before
acceptance to compare the actual callee arguments and live object state.

### Isolated CPU timings

Seven alternating pairs per scenario, page-backed memory, no game. Lifted
draw bodies are opt2; semantic/base/host crates opt3. Guest callees replay
predecoded synthetic effects in both variants. Draw timing excludes fixture
reset and Cpu construction; hash timing includes the same Cpu clone in both.
The shared machine contributes noise, including individual pairs below 1x.

| Port | Baseline | Serial median range | Atomic median range |
|---|---|---:|---:|
| 10655A0 | isolated current lifted body | 1.546–2.111x | 1.573–1.962x |
| 1066C60 | isolated current lifted body | 1.285–1.507x | 1.221–1.486x |
| E0E5E0 | existing exact JOB_HASH port | 1.130–1.753x | 1.039–1.511x |

Six recorded scenarios per draw function/memory mode, **24/24 scenario
medians improve**, including the 511-record apply path. Final log:
`C:/ddda-targets/L174/bench-final.log`. The separate hash measurement is
against the existing exact JOB_HASH port, not an unported lifted baseline;
see `C:/ddda-targets/L174/hash-bench-final.log` for the seven sizes/mode.
These are isolated caller/leaf improvements, **not measured FPS gains**.

### Private artifacts and reproduction

- `E:/ddda-re/notes/native-L174/010655a0.json`, SHA-256
  `16bc692b26e61dc15039369e9f215d7895fd50196e3dd5b2be0bf40dce272d1c`.
- `E:/ddda-re/notes/native-L174/01066c60.json`, SHA-256
  `7de107dd531df94028419e21f7ac9a2da0d282e505c19f60908df35ac3f56dd3`.
- Existing `E:/ddda-re/notes/native-L158/job_hash.json` is reused, SHA-256
  `af8d16a118856af29bcfce8de1a6c86c01f9dd5d15a4fb71ae4a074cd815b209`.
- `E:/ddda-re/notes/native-L174/check`: generated two-body isolated crate,
  including both split helpers of 1066C60. It compiles no ddlift shard.

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_TARGET_DIR='C:/ddda-targets/L174'
$env:CARGO_BUILD_JOBS='4'
$env:DDNATIVE_REQUIRE_VECTORS='1'
& E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex-10 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/l174_vectors.py
python research/native/l174_check.py
python research/native/l174_check.py --bench
cargo test -p mtcore --test semantic_draw_b --release hash_benchmark -- --ignored --nocapture
cargo test -p mtcore -p ddports
$env:DDRUNTIME_SEMANTIC='1'
$env:DDRUNTIME_JOB_HASH='1'
cargo test -p ddports
cargo test -p ddruntime --no-default-features
```

### Integration status

Runtime light suite: **255 pass, zero failures, seven ignored**. Final
focused semantic tests: **five pass, zero failures, two ignored**, including
the five-mutation rejection check. Full `mtcore` + `ddports`: **27 pass,
zero failures, 11 ignored** (`crates.log`). Registration test passes both default
mode and `DDRUNTIME_SEMANTIC=1 DDRUNTIME_JOB_HASH=1`, proving unique override
precedence. Logs: `runtime2.log`, `final-focused.log`, `registration.log`
under `C:/ddda-targets/L174`.

Inventory
function totals stay unchanged: all three replace already lifted functions.
No full native gate or live image/frame-time comparison run by this agent.
Next action: head merges the semantic train and runs its combined native
gate and matched image/critical-job acceptance with SEMANTIC off/on.

## L174b continuation (2026-10-13)

User-assigned follow-up: `010655A0`, `00FFC9F0`, `01140180`. Merged the
requested fps-line commit `1fcd6330` as `0b6c220d`, retaining both sides'
additive modules, registrations and milestones. `010655A0` was already
complete in `f83388b6`; its implementation and evidence above are retained.
New source, tests and recorder stay in the same owned L174 files.

### New implementations and original evidence

- `prepare_model_palette` (`00FFC9F0`, uModel::vf25): original base setup,
  resource/key binding, palette lock/unlock, row-to-column matrix output,
  alternate texture selection and final parameter allocation/copy. Uses
  Pages and named native locals. The matrix dot product executes ordered
  scalar SSE in one inline assembly block per output, preserving operand
  order, rounding and NaN payload selection without FMA. The final four
  parameters retain original x87 FLD/FSTP via the existing hardware helper.
- `draw_culled_instances` (`01140180`, cInstancingCulling::vf16): up to
  three selected ranges, optional secondary material, sphere-to-box bounds,
  command and sort records, copied parameter templates and final reset.
  Original virtual bounds/material calls, state flush, growth and memcpy
  remain explicit guest seams at their original stack/resume addresses.
- Both registrations are under `DDRUNTIME_SEMANTIC=1`. Defaults and
  inventory function totals are unchanged. No game or shard build.

| Function | Original returns | Decoded instructions reached | ABI modes |
|---|---:|---:|---|
| FFC9F0 | 133 | 641/644 | sparse, serial Pages, atomic Pages |
| 1140180 | 114 | 824/824 | sparse, serial Pages, atomic Pages |

The isolated opt2 lifted bodies independently pass all **247** cases.
Combined L174 corpus: **1,415 returns** in each of the three memory modes.
The isolated harness reports **eight passed / zero failed / two ignored**.
Existing five corruption-rejection checks remain enabled. External seams
compare this/target/stack/arguments, initialized objects, and earlier heap
writes; bounds calls also compare the pointed-to 32-byte stack bounds.
No original callee implementation or real gameplay recording is claimed.

The FFC9F0 uncovered instructions are two skipped alignment instructions
(`FFCF28`, `FFCF2F`) and the null alternate-texture write (`FFD475`), whose
earlier non-null guard is not invalidated by this corpus. This is measured
instruction coverage, not an exhaustive path/alias claim. Model cases cover
empty/one/eight matrices, both threshold branches, null lock/output,
allocation growth, countdown decrement, callee-modified counts, overlap
with the matrix/transform inputs, infinities, NaNs, all four SSE rounding
modes and denormal flushing. Instancing cases cover all eight visibility
masks, both material passes, empty ranges, recording flags, growth in both
loops/template copies, four stack alignments, tagged table pointers,
non-finite bounds, and callee mutations that invalidate bindings and limits.

**Oracle limitation found and retained:** Unicorn fails to quiet signaling
NaNs in the final x87 FLD/FSTP copy; both the isolated lift and the semantic
port execute hardware x87 and produce the quiet result. The rejected raw
corpus remains private as `model-x87-snan-unicorn.json`. Admitted vectors
keep signaling NaNs in the SSE matrix inputs, but use a finite value for
the final x87 copy in those four cases. No comparator was relaxed and no
hardware behavior was changed to match Unicorn. A hardware-oracle recording
of that x87 signaling-NaN boundary remains outside these admitted vectors.

### Isolated timings

Same seven alternating pairs / six scenarios / two Pages modes methodology
as the first batch, 500 timed calls per variant and pair. The fixture reset
and Cpu construction are outside the timed interval; synthetic guest-call
effects are replayed identically. Benchmarks now load each case's recorded
MXCSR, including denormal modes, and restore the host value afterward.

| Function | Serial median range | Atomic median range |
|---|---:|---:|
| FFC9F0 | 1.230–1.447x | 1.188–1.485x |
| 1140180 | 1.248–1.847x | 1.380–1.914x |

**24/24 scenario medians improve** versus isolated opt2 lifted bodies.
Individual pairs regress under shared-host noise; no FPS/image claim.
Logs: `model-bench-final.log`, `instance-bench-final.log` under
`C:/ddda-targets/L174`. A first matrix draft used separate scalar helper
calls; the retained ordered-dot implementation passes the same corpus and
reduces that overhead. Final ABI log: `l174b-final-abi.log`.

### Private artifacts and reproduction

- `E:/ddda-re/notes/native-L174/00ffc9f0.json`, SHA-256
  `64182b0114a97cd1c202b3547266b056cc46e409771d4dcd2fed18ac438600ef`.
- `E:/ddda-re/notes/native-L174/01140180.json`, SHA-256
  `4297504193b39946833b5fbbcbc8ac01b3b8fd36668fd385df644d4feb646bc2`.
- `l174_vectors.py --model` / `--instances` record only the requested
  original function using research Python under heavy.py. No window opens.
- `python research/native/l174_check.py` extracts four bodies (including
  their split helpers) into the private standalone crate; no shard builds.
- Set `L174_BENCH_ENTRY=ffc9f0` or `1140180`, then run
  `python research/native/l174_check.py --bench` for each bounded benchmark.
- All cargo commands use `CARGO_TARGET_DIR=C:/ddda-targets/L174`,
  `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`.

Runtime light suite with two test threads: **260 passed, zero failed,
seven ignored** (`l174b-runtime2.log`). The initial fully concurrent run
failed its Present timing assertion; the complete reduced-concurrency
rerun, including that assertion, passed. Head-owned combined integration
and matched critical-job/image acceptance remain next.

Default registration test and the combined
`DDRUNTIME_SEMANTIC=1 DDRUNTIME_JOB_HASH=1 DDRUNTIME_JOB_LOOKUP=1` test pass
with unique addresses and the expected semantic precedence. Registration
log: `l174b-registration.log`. Python scripts compile; `git diff --check`
passes. Only source, tests and documentation are committed.

Full `mtcore` + `ddports` crate gate: **38 passed, zero failed, 11 ignored**
(`l174b-crates.log`). Final source also passes the independent release ABI
replay after the cdecl cursor-lifetime review (`l174b-final-abi.log`).
