# L08: lifter: remaining integer and SSE instructions

Owner: Codex (same session). Branch `task/L08-integer-sse` from the head's
`main` (the head names the base commit), worktree `E:\ddda-rs-wt\L08`.
After L07.

## Why

With x87 (L05) and function extents (L07) handled, the remaining rejections
are mostly single instructions the lifter does not model yet. From the scan
after L05: sbb (414), packed SSE forms (362), div (263), cvtps2pd (207),
rep movs/stos (193), mul (181), movd (146), movsd (131), plus a long tail
(adc, shld/shrd, bt*, bsf/bsr, cmovcc, idiv, neg forms, xchg, cwde/cbw,
movsx/movzx widths, setcc forms, pshufd, unpcklps, cvt* families,
andnps, maxps/minps, rcpss/rsqrtss ...).

## Task

1. Re-run the scan on your data and rank the first unsupported instruction
   per rejected function (the scan's reason counts do this); work in that
   order of yield.
2. For each instruction: exact semantics including every flag it defines
   (and the documented undefined flags as the CPU leaves them, if a later
   instruction can observe them; otherwise reject reads of undefined flags
   as today), the original operand order for SSE (NaN propagation), and
   x86 edge cases (div by zero and overflow raise #DE: reject such vector
   cases in the recorder rather than modelling the trap; `rep` with
   direction flag set; `mul`/`imul` widths).
3. Prefer small exact helpers in the base crate (like the ALU helpers) with
   exhaustive or boundary unit tests against the host CPU via `asm!` where
   the instruction exists in 64-bit mode.
4. Report rejection counts before/after, record vectors for the newly
   lifted functions, pass the lifted test on your data.

## Rules

As in L07 (`docs/tasks/L07-lifter-function-extents.md`, "Rules", i.e. L06's).
With the shard split, `CARGO_BUILD_JOBS=4` and `LIFT_WORKERS=8` are fine;
still one build or one vector run at a time.

## Results

Implemented carry/borrow at 8/16/32 bits; one-operand MUL/IMUL and
DIV/IDIV; 16-bit two/three-operand IMUL; rotates, SHLD/SHRD, bit scans and
bit-string operations; CMOV, XCHG and sign extension; MOVS/STOS with REP,
CLD/STD and either direction. New integer helpers retain every defined
arithmetic flag and mark undefined flags unavailable. The recorder rejects
observed undefined flags, undefined bit-scan destinations, oversized
double-shift counts, divide-by-zero and quotient overflow. Guest DF lives
separately from arithmetic flags.

SSE additions include packed memory operands, scalar/packed doubles,
conversions, comparisons, shuffles, unpacks, packed integer operations,
MOVD/MOVSD and partial-register moves, and sign-mask extraction. Hardware
helpers fix operand order and restore host MXCSR after executing in the
recorder's masked round-to-nearest mode. Source widths and unaffected XMM
lanes are preserved. The model remains serialized guest memory execution;
it does not add guest-thread atomicity or cache ordering.

Same **37,728 scan candidates**, with no previously liftable function lost:

| Measure | Before | After |
|---|---:|---:|
| Liftable functions | 33,669 | 36,793 |
| Rejected functions | 4,059 | 935 |
| SBB | 461 | 0 |
| Packed SSE memory form | 394 | 0 |
| DIV | 297 | 0 |
| REP | 257 | 0 |
| CVTPS2PD | 243 | 0 |
| MOVD | 212 | 0 |
| MUL | 208 | 0 |
| MOVSD | 167 | 0 |
| IDIV | 109 | 0 |
| ROL | 97 | 0 |
| IMUL | 97 | 0 |

The final rejection list is indirect jumps 717, x87 input requirements 147,
INT3 26, RSQRTPS 21, RCPPS 9, segment overrides 7, SFENCE 3, CPUID 2,
PUSHFD 1, x87 overflow 1 and RDTSC 1. Approximate reciprocals remain
unsupported because their CPU-dependent result bits cannot be equated with
QEMU's results. Guest MXCSR changes and unsupported segment/address modes
also remain outside this implementation. Complete counts and scan logs are
private under `native-L08`; the shared inventory and shared native data
were not written.

Private vector selection: **28,665 -> 31,580 functions** and
**85,496 -> 94,088 cases**, retaining every prepared function and adding
**2,915**. Of the 3,124 raw scan additions, 206 have no usable random/zero
fixture within the recorder limits, two require an unavailable x87 callee
contract under the retained boundaries, and one is quarantined for an
isolated emulator discrepancy. The 209 entries and reasons are in private
`native-L08/unverified-L08.json`; patterns, vectors and audit logs remain
there for the head to review and adopt.

The original prepared direct-call boundaries were retained. Newly closed
targets invalidated 406 old callers' cached mocked-call fixtures. They
were re-recorded, or restored only after the original mock boundary was
restored. Five additions remain external as callees while still being
tested as independent roots: `0x00491450`, `0x005E47E0`, `0x0075C990`,
`0x00D181E0`, `0x012C4490`. This retains six prepared callers that otherwise
had no usable new fixtures. Final selection has 14,186 closed functions;
576 of those are additions. Audits are in `stale-audit.json` and
`stabilize.json`, including explicit fixture-key migrations where only the
root's external classification changed.

Subset replay exposed Capstone's incorrect m128 operand detail for
COMISD/UCOMISD. The emitter now reads m64, with synthetic regressions;
18 cached bodies were refreshed without changing their recorded call
boundary. Another failure, `0x00F276C0`, is an existing x87 limitation:
isolated FSIN of f32 bits `0x59d78928`, followed by multiplication by
f32 bits `0x18730fb5` and an f32 store, produces `0x181c7560` in Unicorn and
`0x181c74e9` on this CPU. That reproduces its output difference exactly.
The function and its three cases are quarantined, and hardware semantics
are unchanged. This is not an integer/SSE emitter discrepancy.

Validation: **32 Python regressions**, containing **79 synthetic
instruction programs**, **seven new CPU-comparison tests** in
`ddnative-base/tests/instructions.rs`, and **seven existing Rust units**
pass. Corrected subset replay passes **50,460 cases / 16,931 functions**,
including every closed callee and all additions/affected prepared callers.
Final full replay passes **94,088 cases / 31,580 functions** with
`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative --test lifted --test units`
and `LIFT_DATA=E:/ddda-re/notes/native-L08`. Compilation takes **3m25s**,
replay **44.21s** (monitored command total **252.05s**), minimum free memory
**17.97 GiB**, with four Cargo jobs and `CARGO_INCREMENTAL=0`.
This is sampled instruction comparison, with documented external mocks;
it is not gameplay verification. No shared native gate was run.

Memory: four Cargo jobs, incremental compilation disabled, never a build
and vector run together. Recording started at six workers; one reached
3.66 GiB resident despite the existing recycling/TCG limits. I stopped my
own worker tree proactively and resumed from progress with three workers,
then completed random, zero-input and boundary-repair recording. The
lowest sampled free memory was **9.50 GiB**, above the user's 6 GiB stop
threshold. Long runs had a two-second free-memory guard that would stop
their own process tree below that threshold. Head was notified of the
remaining worker growth; no recorder memory-policy changes are included.

Handoff: source/tests/docs only, no generated code or private research
data committed, no push. Next action is for the head to review/adopt the
private L08 patterns and vectors and regenerate on main. Remaining work:
structured fixtures for the 209 unverified additions, x87-consuming callee
models, the FSIN emulator discrepancy and residual vector-worker growth.
