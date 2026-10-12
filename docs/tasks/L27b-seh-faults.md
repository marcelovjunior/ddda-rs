# L27b: instruction faults and resumption for SEH (the lifter/Cpu part)

- **Owner:** Codex
- **Branch:** `task/L27b-seh-faults` (worktree `E:\ddda-rs-wt\L27b`, from
  task/L27-seh merged with main)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md), merge-train rule. Codex
  cannot receive messages, so coordinate through the board
  (`scripts/board.py`).

## Why

L27 delivers software-raised exceptions: RaiseException, the guest chain
search, nesting, unwind, filters, and a per-thread TEB. Its Results say
what is still missing: faults raised by an instruction inside lifted code
(access violation, integer divide by zero, the ones the memory model and
lifter can detect), and resumption at a non-local or interior address
(ExceptionContinueExecution with a changed CONTEXT, or `__except` blocks
in the middle of a function). Both need lifter and Cpu changes.

## Task

1. **Implement the contract** documented in L27's Results:
   - Fault detection in the lifted code, at the instruction level: a
     faulting access and a divide by zero leave Rust with an exact CONTEXT.
   - Re-entry into a lifted function at an interior address (`__except`
     handler bodies, resumption targets): through the lifter's existing
     block structure, or a generated dispatcher per function. Use a
     general rule.
2. **Coordinate on the board:**
   - claude-6 owns research/native/lift.py (it also adds MXCSR and the SSE
     oracle there);
   - claude-9 owns ddnative-base's x86.rs/Cpu and mem.rs (the page table
     and fast paths).
   Post your design before editing their files.
3. **Tests against your Windows oracle** (the same programs as L27): the
   guarded access violation, divide by zero, resume with a modified
   CONTEXT, and `__except` bodies. All must match Windows.
4. **Prove nothing else changed:** the required-vector replay in the
   integration train.

## Results

(open)
