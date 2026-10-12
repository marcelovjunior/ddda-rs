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

(claude-6, 2026-10-12; branch `task/L48-table-proofs`, main 84cf1194
merged; data native-L48, base `native-train5`)

**A–C, proven by the extended edge proof (14 functions).** 0x00525480,
0x005F5C40, 0x00614DF0, 0x0061C610, 0x00630470, 0x0064F270, 0x00789010,
0x009008C0, 0x0093B470, 0x00A471F0, 0x00A88720, 0x00BA02E0, 0x00BC1A00 and
0x00E7B6F0. The rules:
- **Memory slots:** `cmp dword [esp+d], imm; ja; mov ecx, [esp+d]`.
- **Byte remaps:** `mov ecx, 0x16; cmp eax, ecx; ja; movzx eax, byte [eax+T1]`.
- **Copies:** register copies.
- **Constants:** `lea ecx, [edx+1]` and `and edx, ecx` with constant
  registers.
- **Flag-neutral gap:** `movss` stores between `cmp` and `ja`.

Unit tests: memory slot (and its negative, a store to the slot), byte
remap, lea constant, and-with-constant. 0x00BC1A00's body grows, so it
was refreshed (9 cases).

**D, slot sweep (16 functions recredited).** 0x00441FB0 (47/47 slots),
0x00442180 (47/47), 0x0044CAC0 (15/15), 0x004606A0 (42/42), 0x0047A2A0
(18/18), 0x004B72B0 (5/5), 0x007671E0 (6/6), 0x00797BA0 (13/13),
0x00850390 (10/10), 0x008F7A00 (4/4), 0x009C43A0 (5/5), 0x009E5910 (4/4),
0x00A10080 (4/4), 0x00A48B70 (23/23), 0x00A713F0 (4/4) and 0x00C435D0 (6/6). The evidence per table (`table_sweep`
in the record):
- **End:** usually the 0xCCCCCCCC padding ("not code"); for 0x00A48B70
  and 0x009C4230, a dword pointing outside the function.
- **Index input:** a register untouched since the entry, or an argument
  at `[esp+4]`/`[esp+8]`.

`slot_coverage` lists the slots whose target ran. An index past the end
makes the original jump into non-code, so there is no defined behaviour
for the lifted code to match. 0x009C4230 reached 9 of its 11 slots and
stays a gap.

**Counts on native-L48 (train 5: 46 gaps).** 16 gaps remain:
- **Index from memory or a call result:** 0x012E7CE0, 0x009921E0,
  0x00FA0130.
- **Byte remap of an input:** 0x0044D1F0, 0x004593E0, 0x004594A0.
- **Stack local behind a call:** 0x004298B0, 0x0097A6A0, 0x00CC0F10.
- **Other paths:** 0x00711200 (another jump enters the table jump),
  0x0059C1A0 and 0x00A2F3B0 (index not on the straight line from the
  entry).
- **Closed record, not swept:** 0x005F3C70.
- **Partial sweep:** 0x009C4230.
- **Fragments that are not functions:** 0x00565308 and 0x00786D69.

**Expected verified:** 59,231 + 30 = **59,261 of 59,440**, provided the
new sweep cases and the refreshed record replay.

**Data:** native-L48, base `native-train5` (lift `5f0c87ca…fbfc`,
vectors `edcbcdbe…a54d`; 59,574 / 177,352). Final lift
`1f466682…e987`, vectors `20b70062…d6f1` (59,574 / 177,620). Hash audit:
59,556 functions with cases unchanged, 18 changed (the 16 swept records,
0x009C4230's partial sweep and the refreshed 0x00BC1A00), nothing added
or missing.

**Checked:** 188 Python tests (all of research/native but test_steps).
**Not run:** gen and the Rust replay (the train's).
