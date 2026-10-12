# L45: function tails past Ghidra's end (jump tables into unattributed code)

- **Owner:** claude-6
- **Branch:** `task/L45-function-tails` (worktree `E:\ddda-rs-wt\L45`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The first run of `crates/ddruntime/tests/options.rs` (L34, in train 2)
stops at:

    the menu's writer 0x00EDDB20: stopped: call to untranslated code at
    0x012ee130

0x012EE130 is not a function. It is inside the CRT memcpy variant at
0x012EDE40 (inventory: `FID_conflict:_memcpy`, 708 bytes, status
verified). Ghidra ends that function at 0x012EE104. Its unrolled-copy
jump tables land in code after that end, for example
`jmp dword ptr [edx*4 + 0x012EE14C]` at 0x012EE143 and the 4-byte
moves at 0x012EE110-0x012EE134. The lifted body treats those targets as
external tail calls, and at run time nothing is bound there.

The function is "verified" because its vectors never took that path.
This is a hole in the verification as well as in the lifter.

## Task

1. **Census.** Find every lifted function with a jump-table or direct
   jump target outside its Ghidra bounds that is not another function's
   entry (inventory, `lift.json`, the exe's bytes). Classify them:
   - tails of the same function (the code is reachable only from it);
   - shared tails (reached from several functions);
   - real tail calls.
   List counts and examples in Results.
2. **General rule.** Lift the code a function reaches that lies outside
   its bounds and is not a known function's entry as part of that
   function, with the same evidence rules as the switch and budget rules.
   No special case by address.
3. **Close the verification hole.** A function whose lifted body has an
   ext target that is not a bound entry (not a known function, import or
   platform model) must not count as verified. Either the gate or the
   inventory should catch it. Add vectors that cover memcpy's tail paths:
   sizes and alignments that reach every table entry.
4. **Re-record** on your own LIFT_DATA copy, as in L22. Prove that
   existing records are unchanged apart from the functions the rule
   touches.
5. Results: counts before and after, and the expected verified count.
   Verification follows the merge-train rule; the head runs
   `tests/options.rs` in the train.

## Then: L27b integration

Codex's `task/L27b-seh-faults` (1db9ae9) conflicts with train 2 in
`research/native/lift.py` and `docs/RUNTIME.md`. Merge it, keeping both
behaviours (your L22 rules, its SEH emission and session routing), and
resolve its 39 missing scope/filter targets. Work in the same worktree or
on a second branch, `task/L27b-integrate`.

## Results

(claude-6)
