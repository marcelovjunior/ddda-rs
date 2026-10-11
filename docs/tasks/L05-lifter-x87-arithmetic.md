# L05: lifter: exact x87 arithmetic

Owner: Codex (one agent, no subagents). Branch `task/L05-x87-arithmetic`
from the head's `main` (the head names the base commit), worktree
`E:\ddda-rs-wt\L05`.

## Why

After L04 (floats returned in st0), the next rejections are x87: `fild`
(about 470 functions), `lahf` (about 490: mostly the `fcom*; fnstsw ax;
test ah, N` / `sahf` compare idiom), plus fadd/fmul/fdiv/fsub/fcomp/fxch
and friends behind them. The lifter's x87 model today holds single-
precision bits and supports copies only (`fld`/`fstp` dword, `fld1`,
`fldz`, `fchs`, `fabs`).

## Task

1. **Model the x87 exactly.** The stack holds 80-bit extended values; the
   precision-control and rounding fields of the control word affect every
   arithmetic result; FPU status bits (C0-C3, stack top) are read by
   `fnstsw`. The vectors come from Unicorn, which starts with control word
   0x037F (extended precision, round to nearest) unless the function loads
   another (`fldcw`). Suggested approach (not binding): keep `Cpu.st` as
   80-bit values and run the real x87 instructions on the host CPU through
   `std::arch::asm!` (x87 works in 64-bit mode), loading/storing the
   operands as tbytes and setting the control word first. That gives
   hardware semantics; differences from Unicorn's soft-float show up in
   the vectors (reject those cases in the recorder, as with NaN inputs,
   rather than imitating the emulator).
2. **Note the game's own control word.** DirectX 9 switches the FPU to
   single precision unless the device was created with FPU_PRESERVE. Find
   what DDDA does (D3D create flags in the exe, or `fnstcw` live with the
   Ghidra debugger tools) and document it: the runtime must reproduce it,
   even though the vectors run at Unicorn's default.
3. Support, in this order of yield: `fild`/`fistp`, `fadd`/`fsub`/`fmul`/
   `fdiv` (+ `p` and `r` forms, memory and register operands), `fxch`,
   `fcom`/`fcomp`/`fcompp`/`fucom*` with `fnstsw ax` and `sahf`/`lahf`,
   `fld`/`fstp` of qword and st(i), `fsqrt`, `fsin`/`fcos` only if cheap.
4. Count the newly lifted functions, record their vectors, and pass the
   lifted test on your data.

## Rules

As in L01 ("Rules" in `docs/tasks/L01-lifter-switch-tables.md`), plus:
**memory**. A full build of the lifted crate needs about 11 GB per rustc
process; on this 32 GB machine use `CARGO_BUILD_JOBS=2` for full builds and
`LIFT_WORKERS=4`, and iterate on a subset of the function list. The crate
has `[lib] test = false` (unit tests are in `tests/units.rs`) so the library
compiles once.

## Results

Implemented an x86-64 hardware x87 state model: eight 80-bit registers,
tags, TOP, control and status words, with host state preserved around each
instruction. The emitter retains original opcode/register encodings and
routes memory operands through `Mem` at their original widths. Supports
integer/real loads and stores, arithmetic (integer-memory, reverse and pop
forms included), stack copies/exchanges, comparisons, control/status,
SAHF/LAHF, square root, sine and cosine. Unmasked exceptions and environment
load/store instructions exposing instruction/data pointers remain unsupported.

Removed the obsolete immediate-x87-after-external-call guard: L04's
`x87_push` contract already handles those consumers. Stack checks now
account for referenced registers and all supported pop forms.

Same 37,728 inventory candidates, before/after scans:

| Result / rejection | Before | After |
|---|---:|---:|
| Liftable, closed | 13,402 | 13,960 |
| Liftable, external calls | 17,125 | 18,017 |
| Liftable, total | 30,527 | 31,977 |
| Rejected, total | 7,201 | 5,751 |
| fild | 470 | 0 |
| lahf | 486 | 0 |
| x87-return | 717 | 144 |
| disassembly | 1,025 | 1,025 |
| indirect | 860 | 860 |
| falls | 736 | 820 |
| sbb | 389 | 414 |
| packed | 294 | 362 |
| div | 219 | 263 |

There are **1,450 newly liftable functions** (359 closed, 1,091 ext),
plus 199 existing ext functions promoted to closed. No previously liftable
function is lost. Later rejection reasons grow because x87 no longer
stops examination before them.

The private replay batch grows **26,834 -> 28,244 functions** and
**80,186 -> 84,312 cases**. All prepared functions remain. It admits
1,346 newly liftable functions and 64 previously liftable dependencies;
104 new candidates still lack usable fixtures. Their addresses and reasons
are in `E:/ddda-re/notes/native-L05/unverified-L05.json`. Accepted original
code grows 5,547,191 -> 6,822,577 bytes; new admitted functions account for
1,252,620 bytes. Generated lifted Rust grows 103,013,160 -> 122,868,639 bytes,
in 71 modules. These are batch metrics, not an inventory update.

Verification:

- 21 Python regression tests pass, including 19 synthetic instruction
  programs compiled to Rust and compared with Unicorn, stack-fault
  rejection and immediate mocked float returns.
- Seven Rust unit tests pass: full extended precision, precision-control
  cancellation, rounding modes and integer indefinite, comparison/status
  and SAHF/LAHF, qword/stack/sqrt operations, preservation of host x87
  state, existing signalling-NaN and ALU regressions.
- Final iteration subset: 6,776 cases / 2,289 functions passes, including
  all admitted new functions and affected dependencies/call boundaries.
- Full batch **passes**: `DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative
  --test lifted --test units --timings`, with the private LIFT_DATA and two
  Cargo jobs. All 84,312 cases / 28,244 functions and seven units pass.
  Build: 9m19s; replay: 29.83s; command wall time: 590.69s. Observed peak
  library working set: 17.82 GiB, with no second library compilation.

Recorder corrections exposed by these tests:

- This installed Unicorn resets FPCW to **0x0000**, not 0x037F, and FPTAG
  to **0x5555**. Register reads and a synthetic `fnstcw` confirm this.
  The fixture now explicitly initializes 0x037F and empty tags. Affected
  cases were re-recorded, not patched to change their expected values.
- Unicorn does not reliably signal x87 overflow. Six new loop callers
  repeatedly left values on the stack because a generic mock of
  `0x012F05C0` did not consume its x87 argument. New pre-instruction tag
  checks reject overflow/underflow instead of accepting overwritten slots.
- New cases check full 80-bit st0, stack depth and control word as well as
  the existing registers, memory, calls and float return. Older prepared
  cases retain their original comparison coverage.

DDDA's device creation at `0x00DB1960` passes flags 0x46 or 0x26; both
include FPU_PRESERVE. D3D therefore preserves incoming precision. Evidence
and source addresses are in `docs/NATIVE_PIPELINE.md`, "Exact x87
arithmetic (L05)". The incoming control word of every live game thread is
not established by those flags; 0x037F is the explicit fixture default.

Private data and reproducible investigation scripts are under native-L05.
`rescue.py` and `retry-cw-missing.py` retry missing callers against the
retained closed set, with unavailable callees mocked and that boundary
recorded explicitly. Zero-input cases supply boundary coverage; random
retries preserve previously verified callers. This is sampled instruction
comparison, not gameplay comparison or verification of mocked callees.
Remaining work includes structured inputs for the 104 candidates,
x87-consuming external-call models, and unsupported environment/trap forms.

Memory discipline: subset iterations only, two Cargo jobs, four vector
workers, no build/vector overlap, one final full build. No shared native
data writes, inventory gate, generated-code commit, or push.
