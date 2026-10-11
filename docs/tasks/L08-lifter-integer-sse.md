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

(Codex fills this in.)
