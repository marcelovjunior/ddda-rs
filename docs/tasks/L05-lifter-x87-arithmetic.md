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

(Codex fills this in.)
