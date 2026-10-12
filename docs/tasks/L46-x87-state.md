# L46: the x87 save/restore state (FNSAVE/FRSTOR)

- **Owner:** claude-6
- **Branch:** `task/L46-x87-state` (worktree `E:\ddda-rs-wt\L46`), from
  `task/L27b-integrate`
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L45's boot triage left 0x012F1C9D open. It is the CRT `pow` interior
reached from `_CIpow` 0x012F1C40 on the sBbsRpgMain-constructor route (5
calls deep). Its error path saves the whole x87 state with `fnsave`,
calls a C helper (0x01301C61) and restores the state with `frstor`.
Neither the emitter nor `ddnative-base` modelled these instructions. Its
x87 values also come from three small call-free helpers: 0x01300675
(`fldcw`), `__fload_withFB` 0x013006A5 (pushes one value) and 0x01300660
(2^x on st0). As mocked ext callees they cannot supply those values.

## Task

1. Model `fnsave`/`frstor` (and `fnstenv`/`fldenv` if the same paths use
   them) in the emitter and `ddnative-base`, bit-exact with the 108-byte
   (and 28-byte) 32-bit protected-mode layouts. Test against the real FPU
   with host-oracle tests, like L22's SSE oracle.
2. Promote the three call-free helpers closed with Codex's promote path.
3. Re-record 0x012F1C9D and anything else the closure reaches.

Data: a copy of `E:/ddda-re/notes/native-train4b` (train 3 + L45 + L45b;
59,545 / 177,261) as `native-L46`. The frozen base is
`E:/ddda-re/notes/native-train4b-base`.

## Results

(claude-6, 2026-10-12; branch `task/L46-x87-state`, which merges Codex's
`task/L28b-crt` and carries the head's 24c4f5c4 (steps opt-in))

### 1. FNSAVE/FRSTOR, measured and modelled

`research/runtime/x87_state_oracle.c` (32-bit MSVC, run by its `.ps1`)
read the real FPU (AMD Ryzen 7 5700X3D, WOW64) in seven experiments:
- **Pointers:** FIP and FOP come from the last non-control x87
  instruction. FOP is `(opcode & 7) << 8 | ModRM`. FDP/FDS come from the
  last memory operand. `fnstcw`, `fnstsw`, `fnclex` and `fwait` keep all
  of them.
- **Selectors:** FCS 23h, FDS 2Bh.
- **Reserved words:** read FFFFh, whatever FRSTOR loaded.
- **After FNSAVE:** the FNINIT state.
- **FRSTOR:** recomputes the tag of every non-empty register from its
  contents.

`ddnative-base` `X87::fnsave/frstor` write and read that image. The
emitter proves the pointer fields at each FNSAVE from the last x87
instruction on the same straight line and rejects otherwise. The depth
analysis restores the saved depth at FRSTOR, and the recorder's oracle
tracks the pointers dynamically and writes the same bytes. Tests:
- Rust `x87_fnsave_frstor_match_cpu`: the image equals the host FPU's own
  FNSAVE of the same state in every compared byte, and the E5 tag
  recomputation matches.
- Python: the oracle's E5 bytes byte for byte; rejection cases.
- An emitted-versus-recorded program with `fnsave`/`frstor`.

`fnstenv`/`fldenv` occur in no function (census of every inventory
extent), so they stay unsupported. `fnsave`/`frstor` occur only in
0x012F1C9D.

### 2. The three helpers promoted, 0x012F1C9D recorded

- **Promotion:** `promote --address` names the entries to consider. A
  helper whose arms do x87 work is promoted even when mocked, because a
  mock leaves the x87 stack and control word alone. A closed body keeps an
  x87 entry contract only on request (`lift(x87_inputs=True)`), with its
  need checked at every caller (`X87_NEED`).
- **Promoted closed:** 0x01300660 (2^x, needs st0), 0x01300675 (fldcw)
  and `__fload_withFB` 0x013006A5 (+1). Their two mocking callers,
  0x012F0708 and 0x0132992D, were re-recorded.
- **0x012F1C9D:** lifts with x87 entry contract 1 and its fnsave path, and
  was recorded with 3 cases (zero inputs). The fnsave path itself (x's
  exponent 7FFh, about 1 in 2,048 random inputs) is not reached by any
  case. It is covered by the synthetic end-to-end and host-FPU tests.

### 3. Frame tails completed, and the L45 defect fixed

- **Tails with calls:** frame tails may contain calls
  (`frame_exit(calls=True)`, signed exit). This adds the CRT x87 tails
  0x01300749 and 0x0130070B, and four "entries" that are code inside their
  jumper (0x0056532A, 0x0057D24D, 0x00786D74, 0x00BB922B).
- **Defect (in train 4's data):** L45 inlined such tails but still
  emitted the jump as a `call_ext` tail call, so memcpy's fast path would
  have stopped at `__VEC_memcpy`. A jump into an inlined tail now stays
  local (`inlined_tail`). The gate reports "tail call into its own inlined
  code", and `refresh` re-lifts those records.
- **Refresh budget:** in `refresh`, a distribution with no case after
  seeds×8 attempts now yields to the next.

### 4. Boot stops folded in

- **Stop 10 (`_qsort` CF):** `div ebx; inc eax; cmp eax, 8`. INC/DEC now
  carry an undefined CF over instead of reading it (`Flags::inc_dec`). gen
  lowers the cached template, so no record changes.
- **Stop 12 (0x00DB8B90):** `static_pop` treats a path ending in int3
  after a call as noreturn, not unknown, so 0x00DB8AC0 pops 4 (its IAT
  tail jump to LeaveCriticalSection). 0x00DB8B90 was recorded.

### 5. Data and counts

native-L46: base `native-train4b-base` (lift `ce7a0a08…`, vectors
`7bf1b1f1…`; 59,545 / 177,261). Final: lift `fc02e98b…551e`, vectors
`74bd9787…8f09` (59,547 / 177,266). Codex's hash audit: 59,533 functions
and their cases unchanged; nothing missing.
- **Promoted:** 0x01300660, 0x01300675, 0x013006A5.
- **Re-recorded:** 0x012F0708, 0x0132992D (callers of the promoted
  helpers); 0x012EDE40, 0x012EFEA0, 0x013296A8, 0x00E0FB80, 0x0057D240,
  0x00786CA0, 0x00BB9210 (jumps into inlined tails now local).
- **Added:** 0x012F1C9D, 0x00DB8B90.

Gate on native-L46: 46 gaps. 44 are unproven tables (unchanged since
L45). Two are stale bodies: 0x00565308 and 0x00786D69 are fragments that
pop registers they never pushed (`pop edi; pop esi; ret 8`), cannot be
recorded standalone, and leave the verified count.

**Expected count** on the train-4 numbers (59,229 of 59,440):
- +2 additions;
- −2 for the two fragments;
- the six records train 4 overcounted are now correct and stay counted.

That is 59,229 before Codex's own train-5 changes and before the
boundary-rule inventory rebuild (L45: −58 admissions, 6 of them lifted).

Checked: Python suite (all but Codex's pre-existing
`test_interleaved_local_allocation_is_not_a_mock_argument`, which was
reported to Codex); `ddnative-base` instruction tests (21). Not run here:
gen and the Rust replay (the train's).
