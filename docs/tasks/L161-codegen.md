# L161: game-thread code generation

Assigned to codex-10 on `task/L161-codegen`, base `3e72b984` (combo29).
Scope: offline main/job profile attribution, emitter optimization, isolated
microbenchmarks, regenerated shards, required-vector/inventory gates at
shard opt0 and opt1. No game execution; head owns image/FPS acceptance.

## Evidence and selection

Only combo26 raw captures were retained in `notes/runs` at task start.
Combo29 has text reports. The census uses `stacks_c26vk.json`, 669 sampling
rounds over 30.030 s, and its matching combo26 executable. These are sample
shares, not CPU durations. Wait leaves are excluded using the profiler's
existing classification; unresolved leaves remain in the denominator.
There are 145 main and 1,680 job-worker non-wait samples (guest workers
`0x1070` through `0x1084`, inclusive, step four).
The combo29 VK text report cross-check retains E12A20 (77 pooled nearest
samples), FEE320 (57), FEFB90 (30), FF1920 (26) and FEDD60 (12). Its printed
main/job leaf rows contain 326 `fast_u32`, 115 `fast_set_u32` and 16
`fast_u8` samples. Printed top lists are censored, so these are not a new
raw-stack census or a complete denominator.

The ten entries below rank by pooled nearest lifted ownership: the guest
body plus its non-lifted helpers. Inclusive counts each owner once per
stack. Scheduler `DBDC90` consequently includes runtime synchronization.
The extra `E0E5E0` hash loop is also measured, coordinated with L158.

| Entry | Main nearest / inclusive / self | Jobs nearest / inclusive / self | Guest instructions | Scalar accessor sites | ESP memory instructions | GPR context operands / stores |
|---|---:|---:|---:|---:|---:|---:|
| DBDC90 | 0 / 0 / 0 | 211 / 1505 / 5 | 176 | 68 | 3 | 264 / 121 |
| E12A20 | 9 / 12 / 0 | 77 / 105 / 1 | 692 | 201 | 79 | 961 / 406 |
| FEE320 | 11 / 20 / 4 | 59 / 128 / 10 | 763 | 355 | 33 | 907 / 317 |
| 1054700 | 1 / 4 / 0 | 39 / 124 / 1 | 83 | 25 | 9 | 98 / 41 |
| FF1920 | 4 / 7 / 2 | 35 / 72 / 9 | 678 | 300 | 37 | 778 / 265 |
| FEFB90 | 5 / 5 / 2 | 30 / 30 / 8 | 324 | 160 | 43 | 380 / 121 |
| FEDD60 | 0 / 0 / 0 | 29 / 29 / 11 | 297 | 131 | 84 | 408 / 130 |
| D7CF70 | 0 / 1 / 0 | 21 / 47 / 6 | 1001 | 427 | 217 | 279 / 32 |
| FF14D0 | 4 / 13 / 0 | 16 / 70 / 1 | 306 | 137 | 31 | 52 / 22 |
| 10A35D0 | 0 / 2 / 0 | 20 / 131 / 0 | 369 | 100 | 87 | 146 / 37 |

Guest instruction counts are linear decoding of the inventory's function
extent, not executed instruction counts. Scalar accessor sites are the
current combo29 generated Rust's explicit `fast_u*`/`fast_set_u*` calls;
packed, stack and x87 helpers add accesses internally. Assembly columns
use the matching combo26 optimized image and bounded PDB/unwind symbols;
they are not a byte-identical assembly comparison with combo29 source.
Some split parts are separate symbols, so sampled-entry assembly counts
are lower bounds for their whole guest function. Host RSP operands are
not automatically guest-local spills: they also include ABI arguments
and callee saves. None of these static counts establishes safe guest-stack
promotion; address-taking and callee access require a separate proof.
Host RSP-reference counts in the sampled assembly bodies are respectively
49, 207, 196, 17, 209, 116, 68, 126, 10 and 141. They are reported rather
than mislabeled as a count of promotable guest slots.

All ten generated bodies use `cpu.eax` ... `cpu.edi` and `cpu.esp` directly.
The optimized assembly retains substantial context traffic. For example,
FEDD60 keeps Cpu in RSI, reads/writes GPR fields at RSI+0x100..0x11c,
and reloads stack-derived addresses around scalar helpers. FEE320 reloads
ESP between adjacent pushes and writes each intermediate ESP back.

Direct `call_ext` sites / all `call_ext` sites in the same order are
0/17, 25/42, 7/7, 1/3, 10/13, 2/2, 0/0, 1/2, 6/9 and 1/5.
Direct calls still enter the TLS Rc handler and runtime dispatch; the
address being constant does not remove overrides/services/preemption
contracts. No bypass is introduced.
Identified `call_ext`/runtime-dispatch **leaf samples** charged to these
owners are respectively **4, 4, 3, 3, 1, 2, 0, 0, 3, 0**. The corresponding
identified scalar-memory helper leaf counts are **3, 67, 40, 19, 16, 12,
16, 5, 6, 8**. The saved stacks do not record the dynamic target or argument
registers, so direct-only dispatch nanoseconds cannot be recovered; these
sample counts include indirect boundaries and are not multiplied by
static call-site counts.

Reproduce with `scripts/l161_profile.py`; it writes assembly and JSON only
outside the repository. Raw data and executable bytes are not committed.
Capture SHA-256: `745cf2d17b508d8d771da9ac87131d53eec6560101ab1dd2aaf0f57c25b62036`.
Image SHA-256: `f35336458e84ec8d6a2b8eee75a74010ce042f16069537fb5af120ff7b8f757d`.

Explicit scalar accessor sites per linearly decoded guest instruction,
in table order: **0.386, 0.290, 0.465, 0.301, 0.442, 0.494, 0.441,
0.427, 0.448, 0.271**. This excludes accesses inside packed/stack/x87
helpers and does not purport to measure dynamic ratios. A conservative
post-emission flag rescan proves one remaining dead flag write each in
FEE320 and FF1920 and none in the other eight; zero means unproven, not
that every remaining flag write is dynamically consumed. E0E5E0 has the
additional redundant SHR producer described below.

## Experiments and chosen change

An isolated GPR-local cache prototype flushes/reloads at known full-Cpu
calls and exits, and rejects unknown uses. It is a research experiment,
not enabled in the production emitter. Its first replay caught missing
flushes at comma-terminated split-part exits; a regression now covers that.
The repaired prototype passes the selected 31 original vectors at both
caller opt0 and opt1, with three real lifted dependencies and memcpy.
Whole-function panic state is not a restart checkpoint, as with existing
dead-flag elimination; restartable instruction emission is untouched.

The GPR experiment did **not** show a general speedup. First opt1 paired
fixture medians were E12A20 1.023x, FEE320 0.990x, FF1920 0.950x,
FEFB90 0.971x and FEDD60 0.904x (baseline/candidate). Longer E0E5E0 loops,
16..4096 bytes at offsets 0/1/4093, also stayed near parity: 0.939..1.066x
across 15 medians. These measurements do not justify changing defaults.

The smaller candidate fixes a missed elimination opportunity: removing
a dead INC/DEC removes its input-carry observation, which can make the
preceding flag producer dead. With `LIFT_FLAG_FIXED_POINT=1`, `dead_flags`
now repeats until stable. The independent generation-time switch defaults
off and preserves the previous single pass when unset or disabled.
Each changing pass removes a flag write; accesses and arithmetic stay in
their original order. Call, live-flag and control-flow barriers remain.
E0E5E0's unused SHR Flags constructor is a concrete affected site.

`scripts/l161_check.py` compiles only selected bodies against base/host
opt3 and their original fixtures. `--variant registers` at the original
L161 commit reproduces the rejected cache experiment (L161c replaces
it with the production switch); the default `flags` uses the real generator
on the private cached instruction arms. Its optional benchmark restores
fixtures outside the timed call, alternates order for seven rounds, and
keeps mocked callees inside the timing. These are fixture-path timings,
not gameplay speedups.

## Results

Fixed-point isolated replay: **31 original cases pass at opt0 and opt1**,
for both baseline and candidate. Base/host use opt3. Seven alternating
pairs per fixture give these median baseline/candidate ratios:

| Entry | Opt0 | Opt1 |
|---|---:|---:|
| DBDC90 | 0.998x | 1.051x |
| E12A20 | 1.082x | 0.978x |
| FEE320 | 1.003x | 1.059x |
| 1054700 | 0.967x | 1.023x |
| FF1920 | 1.085x | 0.980x |
| FEFB90 | 1.004x | 0.983x |
| FEDD60 | 0.996x | 1.048x |
| D7CF70 | 1.140x | 0.903x |
| FF14D0 | 0.975x | 0.951x |
| 10A35D0 | 1.007x | 0.993x |
| E0E5E0 sparse fixtures | 0.993x | 1.019x |

Longer E0E5E0 loops (16, 64, 256, 1024, 4096 bytes; offsets 0, 1, 4093)
have **1.032..1.824x opt0** and **1.150..1.879x opt1** median speedups.
92/105 opt0 and 96/105 opt1 pairs improve. The benchmark also checks final
register and flag equality for every long-loop pair. Timing is wall time
on the shared machine, with no affinity in these first runs. Short
fixture timings include timer and mock overhead; small differences are
not established gameplay gains. `scripts/l161_summary.py` verifies paired
round completeness and successful replay/benchmark completion.
The hash-loop gain is **not additive** to L158 when `JOB_HASH=1` replaces
that lifted routine. The wider emitted change still needs head gameplay
measurement; the sparse fixtures do not establish a gain in the other
ranked game paths.

Opt0 log SHA-256:
`d32ff6b9779a4b09322b0db8966766a62f3d9eecee775e5e52083016f157e2d1`.
Opt1 log SHA-256:
`4039e8fdde40e279759adfbe7b429b918aaa5d5ad9d69875840d976b5b28a702`.

The first full regression launch exposed an inherited `mem/host.rs`
child-module path failure in standalone rustc fixtures. The shared
`rust_base.modules` wrapper now resolves that child explicitly; runtime
memory code is unchanged. **18 lean + 8 lift + 8 CFG flag + 9 cache tests
pass**, including synthetic full CPU/memory/access/call-state replay and
the flag oracles at opt0 and opt1.

Regeneration: **927.60 seconds**, 59,734 whole functions, 1,476 modules,
33 crates and 44 step functions. The normalized source audit proves all
44 restartable step bodies unchanged, the same function inventory, and
unchanged memory-helper counts per function (**2,528,853** total sites).
Flags constructor/ALU/INC-DEC sites decrease **262,561 -> 259,323**, a net
**3,238** reduction. 1,655 bodies have fewer counted flag sites. Two cold
bodies (`01303288`, `00B38C70`) each gain one net site: removing a shared
successor kill makes the existing conservative CFG analysis retain
predecessor flags. 1,735 bodies differ textually, including import-boundary
changes from module repacking. No new CFG or memory-promotion rule is used.

Generated receipt SHA-256:
`0e2506bf90dcf647978d4705f725be46c1ad92847c231bcf48965f33aa4d48d5`.
Source audit SHA-256:
`20d6a7416b9383a41f4f5e6faa7222eed3fcaa44e5b1bb6ffe9d35784a8cedef`.

Full opt0 native/vector/inventory gate: **107 pass / zero failures /
nine ignored**, with the same five inventory gaps. All 33 Cargo artifact
records confirm fresh opt0 shards. Full prebuild takes **370 s** including
dependencies/linking; shard rlib artifact span is **227.01 s**, totaling
**432.14 MiB**. These are absolute build measurements, not a paired
compile-speedup claim. The vector corpus contains **178,130 cases across
59,734 functions**. The opt1 full build was stopped on head board message
13496 (shared integration train; no further per-agent full gates).
Isolated opt1 proofs pass; full opt1 validation belongs to the head's train.
The opt0 receipt predates the independent switch; its enabled algorithm
and generated output are unchanged. Switch-off compatibility and enabled
synthetic replay at both optimization levels were retested afterward.

Runtime light suite: **232 pass / two timing failures / seven ignored**.
Both `present_runs_without_the_guest_lock_in_parallel_mode` and
`interlocked_does_not_wait_for_the_guest_lock` pass isolated serial retries
(0.47 / 0.54 s). No runtime implementation changed. No FPS, loading or
original-image improvement is claimed.

Next action: head's combined regeneration with `LIFT_FLAG_FIXED_POINT=1`,
opt1 native gate, then same-route image/FPS comparison. The checked-in
full-gate script records historical reproduction and is head-owned under
the new train rule. No further shard build was launched.

## L161b: adjacent-result zero predicates (2026-10-13)

User continuation: generator-wide game-code speed, isolated opt0/opt1
proofs only, one independent switch and a generator-only train commit.
Head board #13696 subsequently groups this lane with the codegen redesign
and codex-6's forthcoming 50-function benchmark corpus.

### Selection and rejected experiments

The retained profile above attributes FEDD60 29 nearest job-worker samples,
11 self samples and 16 scalar-memory helper leaves. FEE320 has 70 pooled
nearest samples, FEFB90 35. These counts motivate inspecting their loops;
they do **not** establish the dynamic share of the specific predicates below.
Memory calls remain the larger measured cost and belong to the broader
redesign. Existing same-shard lifted calls already use `call_local`;
`call_ext` boundaries also carry services, steps and fixture mocks, so a
blanket static redirection is not a contract-preserving local change.

Three experiments were rejected; none remains in the production emitter:

- Adjacent push pairs: 31 original cases and 1,584 ordered fault/access
  cases pass at both optimization levels, but opt1 long-sequence medians
  include substantial regressions (0.686..1.488x). Opt0 improves
  1.012..1.692x. Not retained.
- Sole-predecessor block chaining: synthetic opt0 gains ~2.1x, but no
  actual qualifying merge in the selected hot bodies. An initial report
  of four changed bodies was only formatting and was corrected on board.
- Inner Rust loops for self edges: synthetic state/call/fault tests and
  original replay pass, but hot fixture timings remain near parity.
  Hash-loop opt1 0.990..1.087x is not sufficient and is not additive to
  `JOB_HASH=1`. Not retained.

### Implemented candidate

`LIFT_RESULT_ZF=1` specializes an immediately adjacent JZ/JNZ predicate
after an explicit 32-bit full-register ADD/SUB/AND/OR/XOR or INC/DEC
result. The register already contains the masked result tested by
`Flags::zf_value`; compare it directly with zero instead of calling the
generic helper. **Every flag write stays**, including INC/DEC's defined
or undefined incoming carry. Memory, arithmetic, register stores, calls,
preemption, control-flow edges and function entries stay unchanged.
No extra pass across a call, partial-register operation, or unknown
producer is inferred. Strings are masked before matching. Restartable
step bodies do not run this pass. Default is off and the switch is
independent of `LIFT_FLAG_FIXED_POINT`.

For the selected fourteen bodies, the exact predicate-only source audit
removes **11 ZF calls**: FEE320 3, FEDD60 2, FEFB90 4, and supporting
D1B540 2. The other ten bodies are byte-identical. The concrete FEDD60
loop branches are at blocks `00FEDF26` and `00FEDF44`; FEFB90 also has
the masked-result loop branches at `00FEFCB0` and `00FEFCF0`.
There is no whole-corpus regeneration/count or per-agent shard build.

### Tested and measured

- **47 focused Python/generator tests pass**, including **13,608** paired
  CPU/flags/access/fault cases at each of opt0 and opt1. Tests cover all
  seven producer kinds, zero/nonzero branch forms, overflow/extreme
  operands, preserved and undefined carry, faults, barriers, narrow-width
  rejection, switch-off compatibility and fixed-point independence.
- **31 original cases pass** for baseline and candidate at both opt0 and
  opt1. Both use fixed-point flags on and base/host opt3. The harness
  checks the candidate text equals only `result_zf(baseline)` before
  compilation; no call or memory lowering difference is hidden in the pair.
- Two recorded FEDD60 loop arms are emitted privately with an exit stub
  and synthetic buffers/counts. Lengths 16/256/4096, offsets 0/1, seven
  alternating pairs per scenario, pinned logical CPU 5. Each pair checks
  full exit registers, flags and output bytes; both copies also match the
  input bytes. These are extracted-loop timings, **not additional original
  game vectors or whole-function/gameplay gains**.

| Loop | Opt0 median paired thread-cycle ratios | Opt1 median paired thread-cycle ratios |
|---|---:|---:|
| FEDF26, six count/alignment scenarios | 1.074..1.213x | 1.005..1.151x |
| FEDF44, six count/alignment scenarios | 1.054..1.134x | 1.098..1.155x |

74/84 opt0 and 75/84 opt1 alternating pairs improve. Windows thread cycles
exclude descheduled time; frequency, code layout and shared-machine
interference still limit these measurements. Wall time is retained too.
Short whole-fixture timings remain mixed, including unchanged controls;
do not extrapolate these loop ratios to frame time. Unlike the previous
hash-only result, these measured loops are outside E0E5E0. Whole-function
ports can still supersede individual affected sites.
Specifically, codex-6 reported a FEDD60 port in progress on board #13712:
the measured loop gain is **not additive** when that port is enabled.

Reproduce with `scripts/l161_check.py --variant result-zf --opt 0 --bench
--cpu 5` (then opt 1), `CARGO_TARGET_DIR=C:/ddda-targets/L161`,
`CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`. `scripts/l161_summary.py`
requires complete alternating samples, original replay and benchmark
completion; it reports thread-cycle ratios for the extracted loops.
Private logs: `result-zf-cycles-opt0.log` / `result-zf-cycles-opt1.log`.
SHA-256 respectively:
`bb3bdbeacb19b7478d3a2ddcc7aea21fc56188747cfef2b0369343200ac2b32a`,
`5d3f051f4dd30bcb84ce4a82fdf99d5548d7128038a996a66232e01b7ac7f399`.

During this batch the light runtime suite passed **234 / zero failures /
seven ignored**, and base passed **39 / zero failures**. These ran during
the rejected push experiment with an unused additive helper; that helper
was removed and final runtime/base sources are unchanged from the starting
branch. The final generator-specific checks are those above.

**Not tested:** full generated corpus, head's combined native gate, game
image, FPS or loading. No game/GPU execution, generated shard build, game
data commit or push. Next: head cherry-picks this generator/evidence
commit only (not historical regenerated snapshot `f32bf0ca`), regenerates
the shared train with `LIFT_RESULT_ZF=1`, and gates/compares it. The wider
redesign should use codex-6's new corpus before further codegen changes.

## L161c: guest registers and flags as host locals (2026-10-13)

User continuation: structural codegen behind independent default-off
`LIFT_REG_LOCALS=1`, including opt2 assembly and the DD42C0, DB63E0,
EFD240 and FEE320 hot bodies. No generated-shard build in this lane.

The whole-function emitter caches every referenced GPR and the complete
lazy `Flags` value in Rust locals. Memory accessors and field-specific
x87/SSE helpers do not receive `Cpu`, so those calls leave the locals
live. Before known full-Cpu lifted/platform/service dispatch and the
PUSHFD/POPFD/string/scan helpers, it writes cached state back; afterward
it reloads the callee's state. Existing dispatch still handles overrides,
services and preemption. Push/pop narrow to the same ordered memory
access and local ESP updates; no guest stack bytes are promoted.

Every explicit normal or split-part return flushes locals. Split wrappers
keep their existing CPU interface; each part owns its cache. Instruction
step emission is unchanged. Unknown Cpu observers, unsupported expressions
or returns retain their entire original body. Whole-function memory
faults remain nonrestartable: event order and prior memory writes match,
but cached registers are not committed at each faulting accessor. CPU
state at CPU-taking call boundaries (including callee unwinds) is exact.
This is the same whole-function versus instruction-step distinction used
by the existing dead-flags pass, not a new checkpoint contract.

`scripts/l161_check.py --variant registers --extra-hot --opt 2 --bench
--cpu 5 --asm` builds only eighteen selected entries against base/host,
with the same fixed-point flags setting on both sides. Caller opt0/1/2
is selected separately; base/host remain opt3. It replays forty original
cases, then measures seven alternating pairs per case using restored
input pages. Thread-cycle and wall-time readings include the same mock
callee dispatch. Each first iteration also compares complete Cpu state
and the fixture-address bytes across variants. These are sparse fixture
paths, not a real gameplay corpus or FPS measurements.

### Results

- **51 focused tests pass**, the previous 47 plus four locals-specific
  tests. Compiled differential fixtures now cover opt0, opt1 and opt2.
  New coverage includes 121 full-state/access/call cases through unsplit
  and split bodies, and 112 fault/callee-unwind cases per optimization
  level. Flag-image, string and scan helper boundaries are exercised;
  unknown observers and nested boundary expressions stay conservative.
- **40 original cases across 14 entries pass for both variants at each
  opt0/opt1/opt2**, plus four real lifted dependencies. The initial
  EFD240 failures were identical in baseline and candidate: its recorded
  path executes FED8F0, which was missing from the isolated dispatch
  table. Including that dependency resolves all three cases without
  changing vectors, generator semantics or checks.
- Seven alternating benchmark pairs per fixture pass full Cpu and
  fixture-address byte comparisons. Pinned logical CPU 5; Rust
  `1.99.0 (b940084d7 2026-09-28)`. Median paired **thread-cycle** ratios
  (baseline/candidate), pooling the 21 pairs for each entry:

| Entry | Opt0 | Opt1 | Opt2 |
|---|---:|---:|---:|
| DD42C0 | 1.052x | 0.984x | 0.974x |
| DB63E0 | 0.985x | 1.010x | 1.004x |
| EFD240 | 0.977x | 1.003x | 0.982x |
| FEE320 | 1.024x | 0.964x | 1.007x |

These sparse paths include timer-query overhead and mocked callees;
DD42C0/FEE320 cases are particularly short. The numbers establish **no
general speedup**. Real captured gameplay inputs remain a separate gate.

Opt2 assembly proves the requested residency in DD42C0: guest ESP stays
in host R15 across five consecutive pushes. Static Cpu ESP loads/stores
fall **23/14 -> 3/3**, and EDI loads **6 -> 3** (EDI stores **2 -> 3**).
Entry and two full-Cpu boundaries account for the candidate loads;
memory-accessor calls do not reload those fields. Other values can spill
to the host stack: locals do not promise enough physical registers for
all guest state simultaneously.

There is a material compiler interaction: the default opt2 baseline
removes DD42C0's dispatcher, while the locals candidate retains one
indirect jump-table site. Filtered assembly grows **467 -> 641 lines**.
This reproduces in a standalone one-function library as well as the
18-entry test crate (both one codegen unit). Raising ordinary jump-
threading block/PHI thresholds to 1000 does not fix it. The isolated
compiler flag **`-Cllvm-args=-dfa-max-cloned-rate=32`** does; 8 does not.
No Cargo profile or compiler default is changed by this commit.

The clone-rate-32 diagnostic also passes all forty original cases and
paired state checks. Its opt2 DD42C0/DB63E0/EFD240/FEE320 ratios are
**0.983/0.980/0.993/0.984x**, so dispatcher removal alone does not establish
a win on these sparse paths. This is evidence for the head's combined
codegen investigation, not a recommendation to enable a broad LLVM knob.

Runtime light: **234 passed / zero failures / seven ignored**, including
all 36 reported test binaries/doc-test results. No runtime code changed.
No game/GPU execution, shard generation/build, full-corpus gate or push.
Next: head's combined train and real captured worker corpus, evaluating
locals together with the separate inline-flags lane and the dispatcher
interaction before any default change. Only this generator/evidence
commit should be adopted, not historical generated snapshot `f32bf0ca`.

Private receipts in `C:/ddda-targets/L161`, SHA-256:

- `reg-tests.log`: `b24dd4b26dca39de9199d016ec7d96137319d9ead85aa22568dd46d8c23fcb12`
- `reg-opt0.log`: `72e493279aa094a8cb2e475c8bd4605e6ea91c6ce20852949ee7434bd5b7cb2e`
- `reg-opt1.log`: `78327ed37079411f4f32216760b7055bf89447101f564bfae025ba32f645d5d4`
- `reg-opt2.log`: `97ce5131097763abdab559574d533390ca417bb05dddfa1e7f3a813e20f37369`
- `reg-opt2-dfa32.log`: `13c8083219abebc1f1950b99da8db25845392f2c39e3cad67b7d5cd5a2388664`
- `reg-runtime.log`: `c76e1dc1c87989da5a8074114fc49bdd7ee6839ca296a31f25682391263f76e5`
