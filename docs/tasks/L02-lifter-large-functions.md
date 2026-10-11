# L02: lifter: functions over 2,000 bytes

Owner: Codex (one agent, no subagents). Head: Claude. Continue in the L01
session; branch `task/L02-large-functions` from the head's current `main`
(the head tells you the base commit), worktree `E:\ddda-rs-wt\L02`.

## Why

`lift.py scan` only looks at functions of 6 to 2,000 bytes. The 1,214
non-platform functions above that hold 39% of the game code (5.5 MB of
14.3 MB): AI, combat, physics, script interpreters. Verified functions are
24.6% by count but only 8.4% by bytes.

## Task

1. Run the scan over the large functions (raise or remove the cap for a
   separate list; keep the main scan's behaviour unless the change is
   verified to be harmless). Report how many lift (closed and ext) and
   the rejection reasons for the rest.
2. Fix what breaks in the lifter for large functions (generated code
   size, compile time, deep match arms, anything Unicorn-side such as the
   20,000-instruction runaway limit for long-running but legitimate
   functions: raise it per function only with a reason).
3. Record vectors for the newly lifted large functions (LIFT_WORKERS=2
   unless the head says the CPU is free), generate, and pass
   `DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative --test lifted`.
4. Check the generated code's compile time and size: if one huge function
   makes `cargo test` much slower, propose a fix (e.g. a module per large
   function) and measure it.

## Rules

Same as L01 (docs/tasks/L01-lifter-switch-tables.md, "Rules"): private
`LIFT_DATA=E:/ddda-re/notes/native-L02`, never write
`E:/ddda-re/notes/native`, no `native_gate.sh`, commit source and docs
only (no generated code), git identity janne / marcelovjunior55@gmail.com,
no AI trailer, never push. Document findings in docs/NATIVE_PIPELINE.md.

## Results

(Codex fills this in.)
