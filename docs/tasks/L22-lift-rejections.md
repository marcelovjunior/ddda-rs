# L22: the remaining lift rejections and quarantines

- **Owner:** claude-6
- **Branch:** `task/L22-lift-rejections` (worktree `E:\ddda-rs-wt\L22`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

58,628 of 59,290 functions are verified (main, `docs/INVENTORY.md`). Of
the 662 others, claude-3 (L17 B/C) handles functions that are lifted but
lack fixtures: SEH helpers, worker loops and runaways. This brief takes
the other two groups:

- **Lift rejections** (about 145): functions the lifter refuses. Causes
  include x87 returns, a jump table the recognizer cannot bound in
  `memcpy`, segment-register reads, CPUID (the CPU name/clock the registry
  service answers from a recorded machine), and a few unsupported
  instructions.
- **Quarantines** (ten): x87 status-word differences between the host and
  Unicorn.

## Task

1. **Census:** list every rejection and quarantine with its reason
   (`scripts/inventory.py`, the lifter's reject reasons, and
   `E:/ddda-re/notes/native/quarantine_evidence.json`), grouped by cause.
   Put it in Results.
2. **Fix by cause**, largest group first, in `research/native/lift.py` and
   the lifter's Rust output conventions (`crates/ddnative-base`), each
   cause with a general rule:
   - CPUID through a runtime service that answers like the host CPU (the
     registry service `crates/ddruntime/src/registry.rs` reports a
     recorded CPU name and clock, so keep the two consistent);
   - segment registers through `Cpu` (it has `fs_base`);
   - x87 returns as the lifter's x87 model defines them
     (`docs/tasks/L04`, `L05`);
   - jump tables per L01.
   For the quarantines, find which side is right on real hardware (run the
   instruction sequence natively, for example in a small C/asm probe under
   `research/`) and make the lifter, or the recorder's model, follow the
   hardware.
3. **Record and verify** with the normal pipeline (lift, vectors, gen, the
   required-vector replay; see `docs/NATIVE_PIPELINE.md`) on your own copy
   of the shared data: copy `E:/ddda-re/notes/native` to
   `E:/ddda-re/notes/native-L22` and point `LIFT_DATA`, `INVENTORY_DB` and
   `INVENTORY_EXTRA` at it. Existing records and cases must stay unchanged
   (prove it, as L14/L15 did).
4. claude-3 (`l17bc-ab`) works in the same pipeline. Keep your lifter
   changes separate from its recorder/strategy changes, and tell it when
   you change a shared file. Write adoption instructions for the head in
   Results.

## Results

Branch `task/L22-lift-rejections` (main 59f55a9 and task/L28-crt 3bf9559
merged in). Private data: `E:/ddda-re/notes/native-L22` (round-1 work copy,
forked from the shared set at 58,913 records / 175,425 cases), `native-L22b`
(intermediate rebase on L17 final) and **`native-L22c` (the hand-off:
native-L28b1 plus all L22 additions)**. Nothing shared was written. Under
the batched-verification rule the Rust replay of the additions is the merge
train's; what is and is not verified is in section 9.

### 1. Census at the start

`structured_inputs.py prepare` on the copy: **157 lift rejections** and
**10 quarantines** among the 662 unverified non-thunks.

| Cause | Count | Examples |
|---|---:|---|
| x87 read at depth 0 (`x87-return`) | 61 | 25 game bodies ending in a register tail jump (0x009C4E00, 0x006051C0, 0x00CF91D0, ...); 36 CRT float intrinsics taking st0 (`_ftol2` 0x012F05C0, `__cintrindisp2` 0x01304690, ...) |
| Approximate reciprocals: RSQRTPS 24, RCPPS 9, RCPSS 1 | 34 | 0x00C2A570, 0x012CA440 |
| x87 forms not modelled (fyl2x, frndint, fxam, fldpi, fptan, fpatan) | 9 | 0x00E22520, 0x012FC184 |
| Jump tables the recognizer cannot bound | 9 | unbounded byte remap 0x0044D1F0/0x004593E0/0x004594A0; register bound 0x00686AD0; Ghidra `switchD_*::caseD_*` export entries among the cases 0x00561040/0x00B5A030/0x00D96F50; negative index: memcpy 0x012EFEA0, 0x012EDE40 |
| STMXCSR/LDMXCSR | 9 | 0x012EF110, `___set_fpsr_sse2` 0x012FC1DC |
| RCR 6, PMAXSW 3, PSLLDQ 2, FNCLEX 2, PAUSE 1, REPNE SCASB 1 | 15 | `__aulldiv` 0x012F2B80, `_strrchr` 0x012EF010 |
| CPUID 2, RDTSC 1, PUSHFD 1 | 4 | 0x00DBD140/0x00DBD170, 0x011BA3C0, 0x00DBD110 |
| Segment-register reads | 2 | CRT CONTEXT captures 0x012EDCAC, 0x012FF13C |
| x87 overflow (x87-consuming mocked callees) | 2 | 0x00F83C00, 0x013156B0 |
| Not code (address-taken / vtable admissions of ASCII tags or packed constants) | 12 | 0x00534658 "XFS", 0x00707270 "prp", 0x00736370 "pcs", 0x00746963 "cit", 0x004E0004, 0x00740000, 0x01010000 |

Quarantines: six x87 status/control differences (0x012FC159 `__ctrlfp`,
0x012FCEBC, 0x01314930, 0x013149B0, 0x013179A0, 0x01317A00), the
large-argument FSIN body 0x00F276C0, and three own-return fixtures
(0x00C1D790, 0x00D9EF20, 0x00FB9A10).

### 2. Quarantines: the hardware decides

`research/native/probe_x87.py` runs the instruction sequences of the
quarantined bodies natively as one sequence (no FSAVE/FRSTOR in between),
one instruction at a time through the FRSTOR/FNSAVE route the lifted Rust
takes, and in Unicorn. **Native equals the Rust route in 23/23 cases;
Unicorn differs in 14**: it sets no PE (0x20), DE (0x02) or C1 (0x200)
status bits, FLDCW keeps reserved bits (0xAFFF stays 0xAFFF where the CPU
reads 0x0F7F), and FSIN(3e18) differs. So the lifter was right and the
recorder now follows the hardware: `x87_host.Oracle` replaces every x87
result in a recording with the host's (state and stored operands; FCOMI
flags compared, never rewritten mid-block). With it the recorder reproduces
the exact values the Rust produced in L14's `diagnose-six.log`. The seven
x87 quarantines were re-admitted and record. The three own-return
quarantines are fixture problems, not x87, and stay.

### 3. Fixes by cause

Each is a general rule with synthetic tests; NATIVE_PIPELINE.md, "Hardware
oracles and machine instructions (L22)", has the details.

- **x87 returns** (61). (a) A non-switch indirect jump in ext mode no longer
  propagates x87 depth to every known start (25 bodies). (b) x87 entry
  contract (ext only): values read from the x87 stack before any call or
  return are the caller's; the count is part of the body key, the recorder
  seeds random doubles (`st_in`, 80-bit) and the replay loads them (36 CRT
  intrinsics). (c) FLDPI, FPTAN, FPATAN, FYL2X, FSCALE, FPREM, FPREM1,
  FRNDINT, FXAM, F2XM1 and FNCLEX run on the host x87 like the other forms.
- **Approximations** (34): the host instruction on both sides; cases that
  ran an approximation or an x87 transcendental carry the host CPUID brand
  (`cpu_dependent`), and the replay compares them only on that processor and
  lists the others.
- **Jump tables** (9): ext-mode open tables. Ext code reads the slot at run
  time and leaves the body for unknown targets, so discovery only decides
  coverage: cmp-imm, mov-imm+cmp-reg and AND-mask bounds (`table_bound`),
  case-label entries allowed under a proven bound, table-only otherwise, a
  runtime jump when no forward slot is valid (memcpy's negative index).
  Closed bodies keep the strict rules. Both recognizers now look past up to
  four instructions between the bound and the jmp that neither branch nor
  write the index (claude-3's 0x0061FDA0, `push ebx; push edi`), and an ext
  indirect jmp continues locally at every block leader of its body, as the
  CPU and the recorder do (0x0061FDA0's slot 0x0062019C).
- **MXCSR** (head's request; 16 bodies). Lifted code runs with the guest
  MXCSR live in the host register: the SSE helpers (sse2.rs, and x86.rs's
  scalar ones, agreed with claude-9) are non-pure asm of the original
  instructions under it, so rounding, FTZ/DAZ and sticky flags are the
  hardware's. STMXCSR/LDMXCSR move it. `Cpu::mxcsr` (0x1F80) holds it
  outside lifted code; ddruntime runs every service under 0x1F80
  (`Registry::call`) and loads the guest value in `call_guest` and
  `Session::call`. COMISS/UCOMISS/COMISD/UCOMISD use hardware compares;
  `gen` retargets the emulated compares of cached bodies from the original
  instruction (`exact_compares`), so no record changes. The recorder's SSE
  oracle runs every flag-setting SSE instruction natively under the guest
  MXCSR (Capstone ModRM/immediate offsets re-encode a memory operand as
  `[r11]`); Unicorn sets no MXCSR flags (0x1F80 where the CPU has 0x1FA0,
  tested). New cases record `mxcsr`; the replay starts each case under
  0x1F80 and compares it.
- **CPUID/RDTSC** through `platform::Machine`: `ddruntime::machine::Recorded`
  answers with the recorded machine's leaves (processor 0, measured by
  `probe_machine.rs` in a 32-bit process) and counts RDTSC at the registry's
  `~MHz`; unit tests tie brand, vendor and family/model/stepping to
  `crate::registry`. The recorder answers CPUID from the host and records
  it; the replay plays the events back.
- **Segment registers, PUSHFD/POPFD**: `Cpu::seg` (CS 0x23, FS 0x53, others
  0x2B) and `Cpu::sys_flags` (NT/AC/ID), all measured; CPL3 POPFD semantics;
  the recorder (CPL0) rejects POPFD values that would differ. PUSH Sreg
  stays rejected (model-specific store width).
- **Instructions**: RCL/RCR, SCAS (REPE/REPNE), PMAXSW/PMINSW,
  PSLLDQ/PSRLDQ, PAUSE, XLATB; host tests in `ddnative-base`.
- **NaN guards retired** (evidence in section 4a).
- **Constant-loop budget** (head's request): a body whose every loop has a
  proven constant trip count records with `max_insn` = 20,000 + trips x loop
  length (ceiling 2M; `insn_budget` in the body key). Input-bound loops of
  the same shape get nothing (tests). Calls inside a proven loop are
  allowed when they cannot change the counter (a mocked callee with a
  callee-saved counter, a platform model, or a closed callee whose own
  loops are proven); nested loops multiply; `why=[]` records the reason for
  a refusal. Of the 60 bodies that were runaway on every seed in round 1, 8
  qualify, incl. the boot-path initializers 0x00CFC150 (4096-entry FSIN
  table, 89,632) and 0x0137E490 (142,880). The others are refused for
  stack-slot counters (22), calls without a bounded cost (12) and
  register-derived starts (9) (census: `native-L22b/budget-census.json`).
- **Missing IStream methods** (head's request): see section 5.

### 4. Still rejected

Of the 157, three remain rejected for a lifter reason; the final set has
20 rejections in all, the other 17 being non-code entries. STMXCSR/LDMXCSR
lift since the MXCSR work (section 3).

| Cause | Count | Why |
|---|---:|---|
| Not code | 17 (the 12 above, and from the rediscovery 0x006C6D78 "xml", 0x006F0063, 0x0090FEFF, 0x00BBFEFF, 0x00BFFAFF) | ASCII tags and packed constants admitted by address-taken/vtable discovery; the admission rules should exclude them (inventory owner). |
| x87-consuming mocked callees | 2 | 0x00F83C00 (calls 0x012F4E6A with two st values), 0x013156B0 (calls `_ftol2`): the push-only mock contract overflows; needs a per-call "mock pops N" contract. |
| x87 values across mocked calls | 1 | 0x012F1C9D (`log` helper): its st arguments cross calls to unlifted CRT helpers. |

Of claude-3's 68 CRT-class rejections (L28 census): **55 record** on
native-L22c, 11 lift without a fixture (eight interior entries leave the
image, three load an unmasked x87 control word from their arguments), and
0x012F1C9D and 0x013156B0 stay rejected.

### 4a. Retiring the NaN guards: evidence

*Why they existed.* (1) "NaN/Inf float input" (check_float): Unicorn
(QEMU 5) does not quiet a signalling NaN on an x87 load, while the CPU
does (harness.py's own header note). (2) "two NaN operands" (two_nans): for
SSE arithmetic with two NaNs the CPU returns the first source operand's
NaN; QEMU applies the x87 rule (larger significand). In both cases the
recorder's emulator, not the hardware, would have defined the expected
result, so such cases were rejected.

*Why they are exact now.* Every instruction whose float semantics matter
takes the host's result: all supported x87 forms (loads, stores,
arithmetic, compares, transcendentals) through the x87 oracle, every SSE
arithmetic, min/max, sqrt, conversion, compare (COMISS/UCOMISS/CMPxx) and
approximation through the SSE oracle, run natively under the guest MXCSR.
The Rust replay executes the same host instructions (x87 over FSAVE images,
SSE in non-pure asm, hardware compares). Instructions outside the oracles
are bit moves (MOVSS/MOVAPS/...), logic (ANDPS/XORPS/...), shuffles and
integer SSE, which are exact in Unicorn for any bit pattern; an x87 form
outside the oracle's encoding set rejects the case
(`x87 instruction outside the host oracle`). So no recording path still uses
Unicorn's float semantics, and no guard is kept.

*Tests* (`test_x87_oracle.py`, native run vs oracle-backed Unicorn, plus
Unicorn alone): signalling-NaN x87 load is quieted (0x7F800001 ->
0x7FC00001) and Unicorn alone keeps it signalling; QNaN payload propagates
through FADD and ADDSS (0x7FC12345); two-NaN order: x87 FADD returns the
larger significand, ADDSS the first operand, both operand orders, and
Unicorn alone gets the SSE order wrong; two-NaN ADDPS across four lanes.

*Effect.* The recorder still evaluates both old conditions and tags a case
it would have rejected (`nan_guard_retired`). On the final set, 258 of the 1,368 new cases carry the tag (230 NaN/Inf inputs, 95 two-NaN operations; some carry both), and 86 of the 468 new functions have only tagged cases, i.e. they are admitted solely because of the retirement. No existing
record or case changed (section 7).

### 5. Missing IStream methods (uMovie)

AddRef 0x00F7B650, Release 0x00F7B660 and Seek 0x00F7B7D0 were not
inventory entries: they are slots of the COM IStream table 0x01436E78
(QueryInterface 0x00F7B690 ... Clone), which has no RTTI (slot -1 is 0) and
is anchored only by its constructor's store at 0x00F7C4AB. Discovery
admitted MT-DTI and MSVC-RTTI tables only. New rule
(`discover_functions.vtables`): a read-only table stored at offset 0 of an
object by lifted code is a vtable when its run of code entries includes an
already known function; the run is cut like the RTTI tables. On the current
lifted manifest plus the liftable candidates' decoded bodies (the XAPO
anchor 0x00DD619B sits in the not-yet-recorded sound-manager constructor
0x00DD5ED0) the final table adds **210 entries, none removed**: 150 from
the RTTI-less vtable rule, 57 address-taken, 3 others (59,290 -> 59,500
non-thunks). **198 of the 210 record**, 5 are non-code rejections, 7 have no
fixture yet.

claude-4's RTTI-less XAPO entries: 0x00DCCD00, 0x00DCF3F0 and 0x00DCF3C0
are admitted by the rule as first written. 0x00DD5030 is slot 17 of the
table 0x0142AFE8, which 0x00DD50EC stores at `[esi+4]` (a secondary vptr),
and 0x00DCCD30 slot 17 of 0x0142AFA0, stored at `[edi+0x4DD78]` by
0x00DD619B (the IXAPO vptr of a member object embedded in the sound
manager; claude-4's finding). The anchor therefore takes vptr stores at any
4-aligned non-negative offset (`vptr_stores`; the MT-DTI rule keeps its
offset-0 anchor); the known-slot corroboration is unchanged. All five are recorded in native-L22c.

### 6. Counts

| | Start | After L22 (recorded; replay pending) |
|---|---:|---:|
| Non-thunk functions | 59,290 | 59,500 (+210 discovered) |
| Lift rejections | 157 | 20 (17 not code, 2 x87-consuming mocked callees, 1 x87 values across mocked calls) |
| Quarantines | 10 | 3 |
| Verified non-thunks | 58,628 (98.9%) | 59,212 expected (99.5%, pending train replay) |
| Selected records / cases (shared after L17: 58,989 / 175,600) | 58,913 / 175,425 | 59,497 / 177,087 (L28b1 59,029 / 175,719 + 468 / 1,368) |

### 7. Existing records unchanged

Round 1 on the work copy: the 58,913 baseline records and 175,425 cases
are an unchanged prefix of the adopted pair (canonical per-record SHA-256,
`record_hashes.py`; `unchanged-proof.txt`). The hand-off set: native-L28b1's 59,029 records and 175,719 cases are an unchanged prefix of native-L22c (canonical per-record SHA-256, `native-L22c/unchanged-proof.txt`); 468 records / 1,368 cases are added.

### 8. Audit: Rust between lifted instructions under the guest MXCSR

Lifted bodies are integer code plus helper calls. The float operations
that run there are guest instructions by construction: sse2.rs helpers and
x86.rs scalar helpers (non-pure asm), hardware compares, and the x87 glue
(FNSAVE/FRSTOR and x87 opcodes, which do not touch MXCSR). `Flags`,
`Mem`, `call_lifted`, the platform models (memset/memcpy/allocators) and the
integer helpers do no float work. Services run under 0x1F80. The test
replay's mock handler does integer JSON work only. Residual: nothing found.

### 9. Verification status and hand-off

Verified on this branch (cheap checks): 110 native Python tests (synthetic
emitter/recorder/oracle cases including native-vs-oracle comparisons for
x87, SSE and NaNs); `ddnative-base` instruction tests (18, against the host
CPU: RCR/RCL, SCAS, PMAXSW, byte shifts, new x87 forms, PUSHFD/POPFD,
Machine, live MXCSR rounding and flags, hardware compares, approximations;
also after merging main);
`scripts/test_discover_functions.py` (8). Recording on the rebased data:
on native-L22c (L28b1 + my round-1 additions): `prepare` found 484
missing candidates and 20 rejections; five strategies (three seeds, 20 s
per function) adopted 207 (image-v1) + 12 (zero) functions, plus the 249
round-1 additions carried over (none stale against L28's closed set, none
duplicated): **468 functions / 1,368 cases**. Among them all seven former
x87 quarantines, all 16 MXCSR bodies (incl. `__control87` 0x01309347,
`___set_fpsr_sse2` 0x012FC1DC), `__fpmath`, `__clrfp`, the six 64-bit
divide helpers, memcpy 0x012EFEA0, 0x0061FDA0, the CPUID/PUSHFD/RDTSC and
CONTEXT-capture functions, the eight IStream and five XAPO methods, and
seven constant-loop bodies with a budget (0x00CFC150, 0x0137E490,
0x00446700, 0x0048B540, 0x004A2A40, 0x004EE180, 0x005A5180). New-case
fields: `x87_oracle` 492, `mxcsr` 299, `nan_guard_retired` 258, `st_in` 80,
`cpu_dependent` 64, `segments` 31, `machine` 6. 265 liftable candidates
still have no fixture (recorder failures, per-seed reasons in the trial
logs). Expected inventory after replay (gen + `inventory.py` on L22c):
**59,212 of 59,500 non-thunks verified (99.5%, 98.2% of code bytes)**;
the 288 others are 265 without a fixture, 20 rejections, 3 quarantines.

Not verified here (the train's, per the batched-verification rule): the
Rust replay of every addition, the `ddruntime` build (machine.rs, MXCSR
boundaries; `cargo check` was cancelled at the head's request), the
`ddruntime::machine` unit tests, and the generated code with claude-9's
`fast_memory` lowering composed.

**Adoption for the head:**
1. Merge `task/L22-lift-rejections` (it already contains main 59f55a9
   and task/L28-crt up to 3bf9559). Conflicts expected only with
   claude-9's prototype lowering in `cpu_arm`/`rust_fn` (merges cleanly
   per `git merge-tree`; my `exact_compares` runs first in `cpu_arm`,
   their `fast_memory` last) and, on x86.rs's scalar `mod sse`, take
   L22's asm bodies (they keep `#[inline]`).
2. Data: adopt `E:/ddda-re/notes/native-L22c` after native-L28b1: its
   `patterns/lift.json` and `vectors/lift.json` are L28b1 plus 468 / 1,368
   (prefix proof above), `patterns/lift_novector*.json` as in L22c,
   `structured_quarantine.json` = the three own-return bodies (the seven x87
   quarantines and 0x0061FDA0 are released). Supplementary inventory:
   `native-L22c/extra_functions.tsv` (+210 entries vs the shared table, 0
   removed; built from the lifted manifest plus liftable candidates, see
   section 5).
3. Then the train: `lift.py gen`, `DDNATIVE_REQUIRE_VECTORS=1 cargo test -p
   ddnative` (with the new cases: MXCSR comparison, machine events, `st_in`,
   segments, `cpu_dependent` cases compared only on the recording CPU,
   "AMD Ryzen 7 5700X3D"), `cargo test -p ddnative-base`, `cargo test -p
   ddruntime --lib` (machine.rs tests), the runtime tests (MXCSR boundaries
   in `Registry::call`, `call_guest`, `Session::call`), and `inventory.py
   build` / `summary` with the L22c table.
