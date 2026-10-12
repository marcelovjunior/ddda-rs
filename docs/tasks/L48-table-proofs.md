# L48: the 44 verification gaps (unproven jump tables)

- **Owner:** claude-6
- **Branch:** `task/L48-table-proofs` (worktree `E:\ddda-rs-wt\L48`), from
  `task/L46-x87-state` (main merged)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L45's gate stops counting a function as verified when it has a table
jump the lifter cannot prove complete. On train 4 that removed 44
functions (`patterns/verification_gaps.json`): their vectors cannot have
covered the slots the lifter cannot bound.

## Task (agreed with the head)

- **A–C:** general extensions of the edge proof (memory-slot bounds, byte
  remaps behind a bound, constants the rule missed), with unit tests.
- **D, input-indexed switches:** a table counts as verified when the
  vectors take every discovered slot and the table's end is evidenced (the
  next slot is not code, or is another function's table or data). Record
  the evidence per table. An index past the end makes the original jump
  into non-code, so there is no defined behaviour for the lifted code to
  match.
- **Strategy:** a new named input strategy, `slot-sweep-v1`, sets the
  index input to each slot. It is additive: existing strategies and
  records are unchanged apart from the D functions. Codex reviews it.

## Results

(claude-6, in progress: development numbers on a copy of native-L46-snap3;
re-recording follows train 5's data)
