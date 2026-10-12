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

(claude-6, 2026-10-12; branch `task/L45-function-tails`, which also merges
Codex's `task/L28b-crt` e1304ff so the recorder changes build on it.)

### 1. Census (train-2 data, `research/native/tail_census.py`)

- **Code past Ghidra's extent:** 2,264 lifted functions have instructions
  outside their inventory extent. 2,251 are their own tails (no other
  record owns the code), 13 are shared tails. All were already decoded
  (the recursive decoder follows direct branches); the hole was only in
  table jumps and in jumps to entries that are not functions.
- **Real tail calls:** 2,694 functions jump directly to another entry.
  Three of those entries are not functions (frame tails, below).
- **Table jumps that left the body:** in train 2's data, 105 functions
  had a table jump the lifter could not prove complete under the first
  rule. With the final rules, 7 bodies grow (both memcpy variants,
  0x0061FDA0, 0x00727370 and three that only grew under an early
  over-wide mask; see below) and 44 remain unproven.
- **Frame tails:** 3 entries pop their jumper's frame: `__VEC_memcpy`
  0x012F9D82 (from both memcpy variants), 0x013006FE (from 0x012F0708,
  0x013296A8, 0x0132992D), 0x00E0FB9C (from 0x00E0FB80).
- **Direct transfers to unbound addresses** (not a function, import or
  platform model) in the lifted arms: **0**. The recorded-mock count (546
  functions whose cases mocked a transfer to a non-entry address in the
  image) is almost all random pointers through random memory, which are
  legitimate mocks, so the gate uses the static form.

### 2. General rules (no address special cases)

- `lift.edge_slots` / `edge_bounded_tables`: a table jump's slots are
  proven on every path into it. Bounds come from cmp/jcc and sub/jb
  edges, `and` masks (and pass-through), `neg`, a register holding one
  constant (`mov`, `xor`, `push imm ... pop`), and table-jump edges.
  `decode_body` iterates until no table adds code. Both memcpy variants:
  16 of 16 tables proven; 0x012EE130 is in the body.
- `lift.frame_tail`: a direct jump to an entry whose derived exit pops
  more than its `ret` is decoded inline, not as a tail call. Both memcpy
  variants decode 336 instructions, including `__VEC_memcpy`.
- Recorder: terminal cases at calls whose return path runs into int3
  padding, and at a root tail jump into a noreturn entry. Discovery: the
  function-boundary rule for address-taken and vtable admissions. See
  NATIVE_PIPELINE "Function tails and verification gaps (L45)".

### 3. Verification hole closed

`lift.gen` now declares a function verified only if
`lift.verification_gaps` finds no unbound direct target, no unproven
table and no stale body. The list and reasons are written to
`patterns/verification_gaps.json`. The motivating record (0x012EDE40 as
lifted before L45) is flagged "stale body: 17 instructions from
0x012EE100 not lifted". Tests: `test_lift_tails.py` (edge rules,
push/pop, frame tails, gate), `test_frame_contract.py` (both terminal
forms), `test_discover_functions.py` (boundaries).

**memcpy vectors:** `structured_inputs.py refresh` keeps cases until
every proven table target is reached. 0x012EDE40: 14 cases, 30 of 30
targets. 0x012EFEA0 (`original_entry`): 15 cases, 30 of 30. All 15
reproduce exactly when rerun. **Not covered:** the inlined
`__VEC_memcpy` path. It needs n >= 0x80, equal 16-byte alignment and the
CRT flag at 0x019AA078. No input strategy produces a finite size in
that range (small-v1 sizes stop at 31; the others give huge copies).
Codex's range inputs (L28c) may fit.

### 4. Re-record on my copy (native-L45 = train 2, the same files as native/backup-pre-train3)

Base: lift.json `64c4d4cd…4778` (59,502), vectors `60bd4258…328b`
(177,102). `refresh` re-lifted the 33 stale ext records in place:
- the 8 from table tails and frame tails: 0x012EDE40, 0x012EFEA0,
  0x012F0708, 0x013296A8, 0x0132992D, 0x0061FDA0 (8 of 15 targets
  reached), 0x00727370 (3 of 5) and 0x00E0FB80;
- the 25 EH4 registrants (section 7). Two of them, 0x012EF8A7 and
  0x012F9244, are terminal-v1 records that `refresh` re-recorded in that
  mode.

No closed record was stale. **Proof** (`inplace_proof.py`, canonical
per-record hashes): all 59,469 other records and 177,010 other cases are
identical and in the same order. The 33 refreshed functions go from 92
to 129 cases. Train 3 changed none of the 33 records or their cases
relative to train 2, so the per-address merge takes them cleanly. Result:
lift.json `9c02ff56…4912`, vectors `b5f773df…c9a6` (59,502 / 177,139).
Gate afterwards: 44 gaps, no stale body.

### 5. Counts

- Gate on train-2 data with the final rules: 77 functions flagged before
  the refresh (44 with unproven tables, 33 stale bodies), **44 after**.
  All 44 are unproven tables: input- or memory-indexed switches such as
  0x0044D1F0, 0x009C4230 and 0x012E7CE0. All of them counted as verified
  in the train-2 inventory.
- **Expected verified, train-2 base:** 59,217 − 44 = **59,173 of
  59,500**, provided the 33 refreshed functions replay (all were verified
  before). The drop of 44 is the hole closing: those functions' vectors
  cannot have covered the slots the lifter cannot bound.
- **On train-3 base after the merge:** train 3's count − 44, plus the 12
  triage additions (section 6, if they replay), minus the 6 lifted bogus
  admissions once the inventory is rebuilt with the boundary rule (they
  also leave the denominator).
- The discovery boundary rule removes 58 supplementary admissions (45
  address-taken constants, 13 table-text slots) when the inventory is
  rebuilt. Six of them are lifted today (0x004E0003, 0x00746963,
  0x00776C70, 0x00786D69, 0x00BFFDFF, 0x01330611); they leave both the
  numerator and the denominator. List: `native-L45b/boundary_dropped.json`.

Not run here (the train's job): `lift.py gen` on the merged data, the Rust
replay of the 8 refreshed records and of the gate's declaration, and
`tests/options.rs`.

### 6. Boot triage (head's list, on native-L45b = a copy of native-train3)

Base: lift.json `a7c3d9ad…cea8` (59,533), vectors `12f7e0dc…b402`
(177,188). Every function on the list lifts with the current lifter; none
was a lift-rule gap. All failed in recording. Batch
(`structured_inputs.py prepare`, then `record`/`adopt --terminal` with
image-v1, zero, small-v1, zero-call-v1, objects-v1 and zero-objects-v1,
3 seeds, 20 s): **12 functions / 36 cases added**. Codex's
`audit_records.py`: ok, all 59,533 previous functions and their cases
unchanged, nothing promoted or missing.

| Function | Route | Cause | Result |
|---|---|---|---|
| 0x00DFECF0 (23,652 bytes) | sBbsRpgMain ctor | the frame needs `__alloca_probe` (Codex promoted it in train 3); its error path ends in `call; int3` | recorded (terminal evidence) |
| `___crtExitProcess` 0x012ED9DD, 0x012E1A60, 0x012E1A13, 0x004080A0, 0x00408100 | WinMain, aTitle, sBbsRpgMain, sRender | they end in a call to `ExitProcess`, `_CxxThrowException` or `__invoke_watson`, followed by int3 padding | recorded (terminal evidence) |
| `_purecall` 0x012EE1A1 | sBbsRpgMain vtable | `jmp _abort` | recorded (terminal tail jump) |
| 0x0040DDC0 | aTitle state 1 | a terminal path, plus memcpy-size/stray-return seeds | recorded (4 of 12 trials) |
| 0x0064E2B0 (uGUIGameover vf05) | two hops | its noreturn path | recorded |
| 0x004AB440, 0x00E0E470, 0x00F9FBA0, 0x00448500, 0x004403A0, 0x004709D0, 0x004734E0, 0x0047DC60, 0x004A2D30, 0x00D0C590, 0x00D8BCE0, 0x00FEE130 | ctor, frame, render | `__alloca_probe`, mock ABI | already in train 3 (Codex L28b) |
| 0x007DF790, 0x00DBC3B0, 0x00E0D940, 0x0109DAC0, 0x010A3C00 | two hops | none | already in train 3 |
| 0x012F9D82 `__VEC_memcpy` | memcpy | not a function: it pops memcpy's saved registers | now inlined in both memcpy variants (frame tail); no vector reaches it (see 3) |
| 0x00520003/4, 0x004E0004, 0x004E0017, 0x00740000/3/4, 0x007D0003, 0x00534658 | pointer hops | constants stored by 0x00444C20/0x004861E0, not code | dropped by the boundary rule |
| 0x0066006E, 0x00670069, 0x0069002E, 0x0069006E, 0x006F0063 | vtable 0x01407180 slots 34-38 | UTF-16 text after the table | dropped by the boundary rule |
| 0x00410000, 0x00E0D750 (`CompressStream::vf10`) | two hops | loops until a mocked callee (0x00CF16F0, the compression step) returns 1; no input strategy returns 1 | open: input distribution (Codex's strategy lane) |
| 0x00476D60 | two hops | constant 900-iteration loop with a byte-remap switch; the budget rule does not cover switch bodies | open |
| 0x012F1C9D | sBbsRpgMain ctor (5 deep, `_CIpow` 0x012F1C40) | entered with x in st0; `__fload_withFB` pushes y before `fyl2x`. Its error path uses `fnsave`/`frstor` (full x87 state save and restore), which neither the emitter nor `ddnative-base` models | open: needs fnsave/frstor in ddnative-base. The x87 contract half is done (below) |
| 0x01304753 | one hop | `_trandisp`-style interior piece: `fst` of the caller's st0 into the caller's EBP frame | open: needs caller-frame inputs |

For 0x012F1C9D the x87 contract part is general and committed (dec31786).
An ext body's entry deficit now walks through calls to closed callees
whose x87 effect is known (`X87_NET`, new `X87_NEED`), and
`x87_depth_check` checks a closed callee's need at the call. Lifting the
three call-free helpers (0x01300675 fldcw, `__fload_withFB` +1,
0x01300660 2^x) closed and walking through them gives 0x012F1C9D its true
contract: one value from `_CIpow`. Only `fnsave`/`frstor` then remain.
Making the helpers closed is a promotion (Codex's `promote`).

0x00DAFED0 (novector) is Codex's (L28c).

Also adopted in the batch: 0x012E1AD4, 0x012F9290, 0x012FE702.
0x012F9290 registers an EH4 table and was lifted before the EH4 rule,
so its body was refreshed alone in L45b (3 terminal cases). Result:
lift.json `39950712…cf56` (59,545), vectors `f4c62c12…b61` (177,224).
The 33 records L45b still shows as stale are exactly native-L45's
refreshed ones (section 4). Bogus admissions are kept out of adoption by
`structured_quarantine.json` (61 entries: the 3 previous plus 58).

### 7. L27b integration (branch `task/L27b-integrate`, worktree `E:\ddda-rs-wt\L27b-int`)

Merged Codex's `task/L27b-seh-faults` (1db9ae9) on top of this branch.
Conflicts were resolved keeping both behaviours: `lift()` keeps L22's x87
entry contract and constant-loop budget outside step mode, `gen` keeps
the verification gaps plus the `instructions` flag, and RUNTIME.md keeps
L32's network row plus L27b's rows. One fix was needed: step functions
emitted the train-2 page-table fast path (`m.fast_u32(pt, …)`) without
`pt` and did not compile. They now emit plain `Mem` calls
(`cpu_arm(fast=False)`), which keeps every access inside the instruction
transaction. Checked: 134 Python tests and `test_steps.py` (12 emitted
step fixtures, including the original EH4 transfer, built against
`ddnative-base` and `ddruntime` without default features).

**The 39 missing scope/filter targets:** the EH4 scope rule (section 2,
NATIVE_PIPELINE "EH4 scope code") decodes them into their registering
functions. That is all 39, and nothing outside the census. Those
registrants are then stale bodies: 25 in train 2's set (refreshed on
native-L45) and 26 in train 3's, where the 25 merge per address and
0x012F9290 (a train-3 addition) is refreshed on L45b. No vector raises an
exception, so the handler code is lifted and steppable but not covered by
vectors.
