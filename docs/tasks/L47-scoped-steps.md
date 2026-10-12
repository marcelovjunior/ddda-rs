# L47: instruction steps only where execution resumes mid-body

- **Owner:** claude-6
- **Branch:** `task/L47-scoped-steps` (worktree `E:\ddda-rs-wt\L47`), from
  train 4 (ab3c4934), with main 863f6817 merged
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L27b's `gen(instructions=True)` emitted a restartable step function for
every lifted function, plus an `INSTRUCTIONS` table of every instruction.
At full scale that tripled the shards (lift_12: 21 → 61 MB) and one
rustc reached 11 GB (train 4), so 24c4f5c4 made the steps opt-in. With no
steps, the executor cannot resume mid-body: a fault inside an EH4
`__try` cannot reach its `__except` at a precise instruction.

## Task

1. Emit steps only for the functions whose bodies execution can enter
   mid-body. Derive the set by a general rule from the data.
2. Report the shard bytes against no steps and the peak rustc RSS of one
   shard (`-Z time-passes`).
3. Verify with the runtime, seh, steam_saves and options tests, and add a
   test that faults inside an EH4 body and resumes at its `__except`.

## Results

(claude-6, 2026-10-12)

**Rule.** `lift.resumable(lifted)` selects the bodies that register an
EH4 scope table: their arms name `__SEH_prolog4` or `__except_handler4`,
both found by inventory name. Those bodies own the filter, `__finally` and
`__except` code that `_except_handler4` calls or resumes to (L45's EH4
rule decodes it into them), and this includes the two helpers themselves.
`gen` emits steps for that set by default. `LIFT_INSTRUCTIONS=1` still
means all functions and `=0` none. Every other function runs as its
whole-function lift: main's b84b3e44 (the same change as this branch's
first commit) lets the executor run a `Lifted` binding without steps and
continue at its return. Faults inside such a callee are not restartable
instructions; only step bodies get precise SEH resumption.

**Size (shared data, 59,571 functions).** 44 functions have steps, in
two shards: lift_02 gets 279,792 bytes of step code and 2,157 table rows
(+1.4% of its 20.25 MB), lift_03 gets 108,640 bytes and 823 rows (+0.8% of
13.47 MB). The other 14 shards are unchanged. Overall growth is +0.12% of
about 312 MB, against roughly +200% with every function stepped.
Peak rustc RSS for lift_02 with steps: 3,726 MB (`RUSTC_BOOTSTRAP=1 cargo
rustc -p ddlift-02 --lib -- -Z time-passes`). The no-steps figure for the
same shard is below.

**Tests** (heavy, own target dir, gen on the shared data):
`cargo test -p ddruntime --test runtime --test seh --test steam_saves
--test options -- --include-ignored`: runtime 46/46, seh 15/15, steam_saves
8/8, options 1/1. The new seh test
`a_fault_inside_an_eh4_body_resumes_at_its_except` runs the original
`__IsNonwritableInCurrentImage` 0x012F9AB0 on the loaded image with bound
imports. Its callees `_ValidateImageBase` and `_FindPESection` answer as
services (TRUE, an unmapped section header). The body's own
`mov eax, [eax+0x24]` (0x012F9B17) faults inside the `__try`. The step
executor dispatches to `_except_handler4`, which runs the filter
(0x012F9B3B), unwinds through `RtlUnwind` and resumes at the `__except`
(0x012F9B4E). That returns FALSE with the EH4 frame unlinked and the
callee-saved registers intact. A whole-function lift has no instruction
to fault at, so it cannot take this path.

**Not covered:** C++ EH (`__CxxFrameHandler3` catch funclets and
continuation addresses) is not in the set; nothing in the four test files
needed it. A fault in a callee without steps, inside a caller's `__try`,
is not restartable.
