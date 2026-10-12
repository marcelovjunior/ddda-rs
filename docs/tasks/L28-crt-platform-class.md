# L28: the CRT functions still marked `platform`

Owner: claude-3, branch `task/L28-crt` (cut from `task/L17-seh-workers`,
whose recorder it needs), worktree `E:\ddda-rs-wt\L17BC`, data set
`E:/ddda-re/notes/native-L17` (the one the boot, L21, generates from).

## Why

The boot (L21) passes `__heap_init`, `__mtinit`, `__RTC_Initialize` and
`__ioinit`, then stops at `__setargv` -> `__setmbcp` 0x012FB049, which has
no lifted body. 116 inventory rows still have status `platform`, rule
`CRT/library range`: `___updatetlocinfo`, `__control87`,
`___set_fpsr_sse2`, `__fpmath`, `___tzset`, the 64-bit division helpers,
stdio/lowio (`__fsopen`, `_fread_s`, `_fclose`, `__read`, `__lseek`,
`__close`, `__commit`, `__alloc_osfhnd`, `__getstream`, `flsall`,
`__fcloseall`), `FID_conflict:_memcpy` 0x012EDE40 and about 60 `FUN_` in
0x012E0EA0..0x0132A390. Per RUNTIME.md they are lifted and verified like
everything else, never written as services.

## Task

1. Census the class: which are still platform-only, and why the earlier
   CRT lifting (L14/L15) skipped them.
2. Bring them into the normal flow (scan, vectors, gen, required-vector
   replay) with a general rule, not by address.
3. Lift rejections go to claude-6 (L22, lifter rejections).
4. Tell claude-2 (L21) as each group lands.

## Rules

As L17: heavy jobs only through `E:/ddda-re/tools/heavy.py`,
`CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`, at most six vector workers,
existing records and cases unchanged unless a documented boundary change
requires it, no address special cases, no pushes.

## Results

### Recorder classes: finite condition values (2026-10-12)

Starting from the census, dispatch/return is the largest recorder class
(62 rows), then instruction bounds (29). Neither class had received the
new guarded-values-v1 strategy as a whole; dispatch trials are queued
first. Queues exclude functions now credited by the latest inventory and
the explicitly set-aside 0x00493AA0. Baseline hashes:
`L28c/classes-before.json`, **59,603 functions / 177,403 cases**.

Static inspection identifies SETNE in 0x00416740/0x00618880 and NEG,
SETO/SETB in 0x01146420. The guarded proof now materializes known condition
bytes without changing flags; absent flags produce unknown values.
NEG tracks its width-correct result and defined CF/OF/ZF/SF. No parity
facts are invented. **166 light tests pass**, including a finite loop
controlled through SETNE and original execution at zero, one, signed
minimum and unsigned maximum for 8/16/32-bit negation. No original-game
additions yet in this class batch. No guard or instruction ceiling changed.
**Next:** complete dispatch trials, then wrapper cleanup and bound classes.

### Final recipes and post-train-11 census (2026-10-12)

The two manager chains finished **0/3 each** with their checked producer
observations preserved. For 0x004A0C00, 0x00437BE0 then 0x00489F80 still
stops the proof at 0x004027E9 after **92** instructions: the linked-object
field remains unknown. For 0x00D29AF0, 0x00740590 then 0x00D29820 retains
the **20-byte** input count, but the allocator call at 0x00D29B4C still
has **EAX=0** after **21** instructions. The manager's accepted mocked
boundary does not supply a usable allocator. No checks were relaxed.
Private evidence: `L28c/final-manager-recipes.log` and
`L28c/final-manager-proofs.log`. All workers exited.

This follow-up adds only **0x00C9E0F0, +1/+3**, audited above. Four of the
original 15 remain: 0x00493AA0 (set aside), 0x004A0C00 (linked-object
facts), 0x00815C60 (floating comparison proof), 0x00D29AF0 (allocator
state). **163 Python tests pass**; head Rust replay is still required for
the new serializer. Frozen base unchanged; native-L28c remains open.
Handoff SHA-256: patterns/lift.json
`b5be3edbdb505dae9c977e2e76f148f314ef6eab2e09a49b990c931bccbe56d1`;
vectors/lift.json
`2f638c8adeaf5f0eda39f14788cb2799a3470da49b7399b306246274c7943613`.

[RESIDUALS.md](../RESIDUALS.md) is the requested single-table census:
**137 non-verified / 59,440** non-thunk inventory functions after train 11,
**59,303 verified**. Membership exactly matches scripts/inventory.py's
database query, with no duplicate/missing rows; each has class, blocker and
owner. **104 recorder; 33 claude-6** (15 gap, 10 lifter, eight x87 oracle).
The gaps file has 16 entries, but 0x00786D69 is not admitted to inventory
and is outside this denominator. 0x00C9E0F0 stays listed as replay pending.
Private source hashes/rows: `L28c/residual-census-train11.json`.

The four undefined-flag entries all execute DIV/IDIV followed immediately
by INC/DEC (0x005524B0, 0x00604A80, 0x00BA4160, 0x00C9B350). Their
specific instruction sites are in the table; CF-preservation/liveness
review was sent to claude-6 on the board. No flag guard was changed here.
**Main a6fb8349 merged before handoff. Next:** head replay of the new
serializer, then assign remaining work from the census, boot stops first.

### Exact-register serializer inputs (2026-10-12, pending replay)

Opt-in **guarded-values-v1** supplies the bounded proof with the exact
initial GPR values recorded for that case, excluding ESP (already governed
by the stack contract). It retains guarded-input-v1's page/register input
distribution, alias checks, mock matching and return checks. Other
strategies still leave entry values unknown unless explicitly bound by a
checked setup. The same opt-in mode accepts checked constructor state.
Source **0e2e5260**; **163 light tests pass**, including comparison with
ordinary synthetic original execution and rejection of a changed return.

**0x00C9E0F0 records 3/3:** the explicit source pointer resolves the null
test at 0x00D00D68. All three traces prove **24,681 instructions**, giving
limit **44,681**. Private native-L28c is **59,603 functions / 177,403 cases**,
**+1/+3** since the prior handoff. `L28c/exact-values-audit.json` confirms
all prior **59,602 / 177,400 unchanged**, no stale mocks, promotions or
unexpected edits. Frozen native-train3-orig remains untouched. No Rust build;
head replay required. This addition follows the ten queued for train 11.

Four original recipes remain, including 0x00493AA0 set aside. Two evidenced
manager chains are queued: save manager 0x00437BE0 then record constructor
0x00489F80 for 0x004A0C00; allocator-manager initializer 0x00740590 then
buffer constructor 0x00D29820 for 0x00D29AF0. The initializer publishes the
allocator pointer to 0x0187662C at 0x00740833. Every producer must still
match all accepted observations. 0x00815C60 requires floating comparison
facts beyond the integer/zero-vector proof; no assumptions added.
**Next:** finish those trials, then the post-train-11 residual census.

### String and constructed-copy fixtures (2026-10-12, pending replay)

**+3 functions / +9 cases, all 3/3:** 0x0048F330 and 0x00495520 use
guarded-input-v1 with exact narrow-register/zero-vector facts; their proofs
cover 59,359 and 532,884 instructions. 0x0048AB50 replays its accepted
0x00489F80 constructor exactly, then binds both stack arguments to that
constructed object (a self-copy); its proof covers 38,189 instructions.
Each execution limit is its proved count plus the existing 20,000 margin.

Cumulative **+10 functions / +30 cases** from the 15-recipe queue;
native-L28c **59,602 / 177,400**. `L28c/string-constructed-audit.json`
confirms all **59,592 baseline functions / 177,370 cases unchanged**,
with no promotions, removals, unexpected edits or stale mocks. A Windows
file-sharing error delayed the constructed vector rename: the pending
file was checked to contain the identical prior cases plus precisely the
three new cases before completing the rename and adoption bookkeeping.
The subsequent full audit passed. Frozen base native-train3-orig untouched.

**161 light tests pass** for the source through 1d22d152; no subsequent
source edits. Main merged through 35b516ed before handoff. No Rust build;
head integration replay remains required.

**Five recipes remain:**

- 0x00493AA0 stays aside as requested. Manager-first changes destination
  reads/writes/MXCSR; destination-first changes manager reads/writes,
  ECX/EDX outputs and calls. Neither order matches the accepted setup.
- 0x004A0C00: constructor 0x00489F80, EAX bound to its input EAX,
  records 0/3. The proof stops at conditional branch 0x004027E9 after
  92 instructions; further callee/manager state is unresolved.
- 0x00815C60: constructor 0x00811780, EDI bound to its input EAX,
  records 0/3. The proof reaches COMISS at 0x00816522 after 9,319
  instructions; floating-point comparison facts remain unresolved.
- 0x00C9E0F0: narrow-register/zero-vector retry still records 0/3
  (runaway). A valid serializer source/count recipe remains outstanding.
- 0x00D29AF0: constructor 0x00D29820 supplies its nonempty default buffer;
  ECX bound to its input ECX still records 0/3 (runaway). The proof retains
  the 20-byte buffer count but stops after 21 instructions at allocator
  dispatch 0x00D29B4C (CALL EAX, EAX=0); allocator-manager state remains
  unresolved. The constructor alone does not establish that dispatch.

Private logs: `narrow-vector-recipes.log`, `remaining-constructed-recipes.log`,
`constructed-815c60-proof.log`, `remaining-constructed-proof.log` under L28c.
**Next action:** prioritize any new boot stop; otherwise resolve the first
unknown branch in 0x004A0C00 using checked caller/manager state.

### Division/tail fixtures (2026-10-12, pending replay)

**+2/+6, all 3/3:** 0x0046A2D0 proves **151,141** instructions (limit
171,141); 0x00DBA780 proves **102,484** (limit 122,484), including its
checked final external mock tail. Cumulative session **+7/+21**,
native-L28c **59,599 / 177,391**. `division-tail-audit.json` confirms
all 59,592 baseline records / 177,370 cases unchanged; no stale mocks,
promotions or unexpected changes. **161 tests pass.** Main merged through
35b516ed (boot main-loop milestone); no Rust build. Eight original recipes
remain, including set-aside 0x00493AA0. **Next:** checked 0x00815C60
constructor input, then narrow-register/zero-vector string retries.

### Narrow register and zero-vector facts (2026-10-12)

The proof retains known byte/word register lanes without inventing unknown
high bits. Full and implicit register writes clear narrower facts. Exact
bitwise-zero vectors can supply zero store bytes; other floating/vector
values remain unknown, and calls or overwrites discard vector facts.
**161 light tests pass**, including original synthetic SSE/store/byte-test
execution and explicit/implicit clobbers. Recording remains queued behind
claude-2; accepted data is still the audited **59,597 / 177,385**.

This addresses the string-copy stop at 0x007674D5: PXOR/MOVQ zeroes the
source bytes, then MOV DL/TEST DL needs only the known byte lane. Planned
retries: 0x0048F330, 0x00495520 and serializer 0x00C9E0F0.
For 0x00815C60, constructor 0x00811780 takes its object in EAX, initializes
+4/+8/+0xC/+0x10 at 0x0081191F onward and flags at +0x4654. Caller
0x00812820 passes the same object in EDI to 0x00815C60 at 0x008128D5.
**Next:** checked producer 0x00811780 with EDI bound to its input EAX;
then the string retries, after the pending division/tail trials.

### Checked division proof (2026-10-12)

32-bit DIV/IDIV proofs require known inputs, nonzero divisor and a fitting
quotient. Signed division truncates toward zero with a same-sign remainder;
all arithmetic flags become unknown. **159 light tests pass**, covering
both signs, both quotient-overflow cases, zero divisor and undefined flags.
0x0046A2D0 and 0x00DBA780 trials are queued serially in heavy.py behind
claude-2's boot run. No new data since the audited +5/+15 checkpoint.
**Next:** run those trials; inspect narrow-register/zero-vector facts for
the two remaining string-copy recipes while recording waits.

### Checked mocked tail proofs (2026-10-12)

A bounded root trace can end at a direct external mock tail only with its
return address preserved and a checked static cleanup. The certificate
records the tail site; execution must report that same mock and cleanup.
Closed/model targets, unknown cleanup and outstanding stack words do not
use this rule. **158 tests pass**, including original synthetic tail
execution and missing/mismatched mock events. Original-game trial pending.

The queued sibling diagnostic was canceled (own waiting process only)
after static inspection established its next missing operation: signed
32-bit division at 0x0046A352. **Next:** division proof semantics, then
serial 0x0046A2D0 and 0x00DBA780 trials when the boot slot is free.

### Multiplication and rotates (2026-10-12, pending replay)

General proof support covers one-/two-/three-operand 32-bit multiplication,
including EDX:EAX products, and 32-bit rotates with masked counts. Only
CF/OF are known after multiplication; rotates preserve ZF/SF and leave OF
unknown for multi-bit counts. **157 light tests pass.**

**+2 functions / +6 cases**, all 3/3: 0x00494D80 proves **76,963**
instructions (limit 96,963), and 0x0046A150 proves **83,097** (limit 103,097).
Cumulative session **+5/+15**, native-L28c **59,597 / 177,385**.
`full-multiply-adopt-audit.json` confirms all 59,592 baseline records /
177,370 cases unchanged. Ten original recipes remain, including the
set-aside 0x00493AA0. No Rust build; head replay required.

0x0046A2D0 still records 0/3; its next diagnostic is queued behind
claude-2's boot run. **Next:** that proof stop and the final mocked tail
boundary of 0x00DBA780, with runtime call-contract checks retained.

### Exact-input recording batch (2026-10-12, pending train replay)

**+3 functions / +9 normal-return cases**, all 3/3 under guarded-input-v1:

| Function | Proved instructions | Recorder limit |
|---|---:|---:|
| 0x004A06B0 | 37,740 | 57,740 |
| 0x00495150 | 308,534 | 328,534 |
| 0x0050A110 | 23,595 | 43,595 |

0x00495150 needed 120 seconds of wall time to finish three cases; its
instruction limit remains proof-derived. Adoption selected the complete
three-case trial for each new body without editing earlier trial history.
`L28c/standalone-adopt-audit.json`: **all 59,592 records / 177,370 cases
unchanged**, zero promotions, stale mocks or unexpected edits. Native-L28c
now **59,595 / 177,379**. **155 tests pass**; no Rust build or replay.
Main train 10 merged before handoff. Sources: 6850c602, 3449543b,
542f9d42, c000069d. Twelve original recipes remain, including 0x00493AA0
set aside per head.

Remaining proof stops are recorded in `standalone-proof-stops.log`.
0x00494D80 reaches IMUL at 0x00495010 (69,678 instructions); 0x0046A150
and 0x0046A2D0 stop on ROL. 0x00DBA780 reaches a root tail to 0x00DC5C00
after 102,460 instructions. Other cases still need call/state facts;
0x00D29AF0 still raises its original CPU exception with this input.
**Next:** general multiplication/rotate proof semantics, boot route first.

### Standalone exact-input proof (2026-10-12)

`guarded-input-v1` makes the same bounded initial-page proof available
without a constructor. It uses the recorder's existing input distribution
and exact page generator; initial register values remain abstractly unknown.
Checked state remains required for `guarded-state-v1`. **155 light tests
pass**, including an original synthetic loop whose budget depends on its
recorded generated input byte. No original-game addition is claimed yet.

A serial trial list is prepared for the remaining 14 functions, direct boot
route first; **0x00493AA0 is excluded**, as requested. The first signed-loop
0x00494D80 constructor retry is still waiting behind head-train in heavy.py.
Private accepted data remains **59,592 / 177,370**, unchanged from the
train-10 baseline. **Next:** finish that run, then the standalone trials;
boot-stop messages always take priority.

### Checked stack-argument bindings (2026-10-12)

`--bind stack+4=1.regs_in.edi` binds a method stack argument to a checked
producer register, optionally selecting a setup phase. Only aligned offsets
4..128 are allowed; the return address cannot be rebound. Bindings replace
stale captured stack bytes in both execution and proof inputs. Later setup
phases retain their original entry distributions. **154 light tests pass**;
no recording ran while waiting for head's heavy slot.

Prepared recipe: destination 0x00489F80, source manager 0x0049F990;
0x004A06B0 gets EAX from phase 0 input EAX and stack+4 from phase 1 input
EDI. Both producers already have three accepted cases. Pending actual
sequence replay; this is not a recorded or validated combined recipe yet.
A second evidenced producer is 0x00DBB690 for destructor 0x00DBA780:
factory 0x00488B10 calls it on ECX at 0x00488B42, and wrapper 0x00488B70
calls the destructor on the same ECX object at 0x00488B79.
**Next:** queued 0x00494D80 retry, then these two constructor recipes.

### Integer proof operations (2026-10-12)

The bounded interpreter now models CDQ, 32-bit SHLD/SHRD, and signed
branches with overflow-aware arithmetic flags. Unknown operands do not
produce signed comparison facts; zero-count shifts and CDQ preserve flags.
**153 light tests pass; records unchanged**, `integer-state-audit.json`.
0x00494D80 advances past CDQ to the signed loop at 0x00495032 (69,674
instructions). The retry with signed-branch support is queued behind head's
train in heavy.py; no guessed budget or weakened check. **Next:** finish
that retry, then the evidenced 0x004A06B0 source/destination recipe.

0x004A06B0 evidence: caller 0x004964C0 passes destination `ESI+0xE65A0`
and source singleton 0x018FA504 at 0x00496574-0x00496581. Parent constructor
0x00495FF0 builds that destination using 0x00489F80 at 0x00496022.
0x0049F990 publishes the source singleton from EDI at 0x0049FD4F.
This recipe requires general checked setup-output bindings to stack arguments.

### Exact setup/input pages (2026-10-12)

Opt-in `guarded-state-v1` requires checked setup state. Its proof reads
captured pages and lazily generated pages from the recorder's same immutable
input generator, capped at 256 pages. Known stores replace those values;
unknown writers must pass existing protected-read alias checks. Changed
instruction bytes refuse a proof. Entry registers remain unknown unless
explicitly bound. **151 light tests pass**, including lazy-page equality,
overwrite invalidation, alias checks and changed-code rejection.

0x00494D80 with checked 0x004980F0 remains **0/3**, but proof advances
from the flag test at 0x00494D90 (7 instructions) to **0x00494EA5, CDQ,
69,197 instructions**. The flag's page was untouched by the constructor;
its method input comes from the normal deterministic page generator.
`L28c/manager-state-audit.json` confirms **59,592 / 177,370 unchanged**.
**Next:** general CDQ/double-shift proof semantics, then repeat this recipe.

**0x00493AA0 set aside per head:** manager-first changes destination
producer 0x004915A0 reads, writes and MXCSR. Destination-first reproduces
0x004915A0, then changes save-manager 0x00437BE0 reads, writes, ECX/EDX
outputs and recorded calls. Neither sequence passed exact replay; no
accepted record was changed. Evidence: `bound-output-compare.log` and
`output-first-compare.log`. No general composition rule was established.

### Earlier-phase method bindings (2026-10-12)

`constructed_inputs.py --bind edi=0.regs_out.eax` selects an earlier
checked producer output by zero-based phase index. Two-part bindings still
select the last producer. Bindings are saved for the method only; subsequent
producers retain their accepted entry distributions. Invalid phase indexes
reject. **148 light tests pass**, including binding provenance and sequence
replay. `L28c/phase-binding-audit.json` remains clean.

Destination-first 0x004915A0 -> 0x00437BE0 -> 0x0047DF20 also records
**0/3** for 0x00493AA0. A diagnostic confirms the first producer matches
its accepted case, then the save-manager replay changes reads, writes,
volatile outputs and calls. The opposite order changes destination reads,
writes and MXCSR. Both sequences reject; no comparison was relaxed and no
existing record was replaced. **Next:** identify shared input dependencies
and an independently accepted setup distribution, or advance the next
boot-priority manager recipe. Fifteen original recipes remain unfinished.

**Batch handoff:** boot stop 16 is the sole addition (+1/+3), native-L28c
**59,592 / 177,370**. All 59,591 baseline records / 177,367 cases are
unchanged. Main merged before handoff; full Rust replay remains with head.

### Bounded setup replay (2026-10-12)

Guarded input strategies now re-prove a producer on accumulated setup
state, using exact prior writes and checked aliases. They keep the original
input distribution and all accepted-case comparisons; image-only proofs
remain disabled on constructed state. Tests cover long bounded producers
and legacy random producers in sequences. **147 light tests pass.**

0x004915A0 now returns in the three-producer recipe, with the same proved
208,066-instruction budget, but prior state changes its reads, writes and
MXCSR observation. The exact comparison rejects this recipe; no fixture
is adopted. `L28c/reproof-audit.json` confirms all baseline records unchanged
and only the already-reported boot stop 16 addition. **Next:** investigate
the conflicting setup inputs or an independent producer ordering.

### Boot stop 16: 0x00DC8270 (2026-10-12, pending replay)

**3/3 normal returns, adopted: +1 function / +3 cases.** The existing
checked-constructor strategy replays accepted 0x00DD5ED0, then binds
ECX to its input ECX. With guarded-stores-v1, exact constructor writes
resolve the reset branches and virtual calls. Its proof counts **49,933
instructions**, giving a **69,933** recorder limit; no guessed budget.
All three cases retain eight recorded calls. Ordinary inputs still exceed
the default limit; the constructor supplies the required object state.

**145 light tests pass.** `L28c/boot16-audit.json` proves every prior
**59,591 record / 177,367 case unchanged**, no stale mocks or promotions.
Private native-L28c now holds **59,592 / 177,370**. Source dd2b67bd;
main merged before handoff. No Rust build; head must replay the addition.
**Next:** resume the 0x00493AA0 output-constructor recipe, unless another
boot stop arrives.

### Explicit checked bindings (2026-10-12)

Checked producer return values can be explicit method-register proof inputs;
other entry registers remain unknown. Mismatched bindings and ESP bindings
reject. Stack allocation helper tests use plain compiler terminology and
check that the return address is preserved when the callee adjusts ESP.
**145 light tests pass; 59,591 records / 177,367 cases unchanged**,
`L28c/bindings-audit.json`. The three-producer 0x00493AA0 recipe still
records 0/3. **Next:** priority boot stop 16, 0x00DC8270, with checked
0x00DD5ED0 constructor state and a proof-derived instruction budget.

### Affine input relationships (2026-10-12)

The guarded-store proof tracks linear expressions modulo 2^32 through full
register copies, LEA, add/sub, saved stack words and alias-checked stores.
Only complete cancellation produces a known value. Independent entry
registers remain independent even if their concrete pointer witnesses match.
Partial writes and overlapping stores discard expressions; unknown writers
must pass the existing runtime alias guard. Unknown arithmetic operands do
not produce carry facts from a known result.

**143 light tests pass; accepted data unchanged**, `affine-audit.json`.
The checked manager recipe now passes 0x00493C29, then stops after 7,634
instructions at 0x00484B8E, which tests its output pointer for null. That
pointer derives from the method's still-unbound EDI. No recorded case yet.
The original caller 0x004964C0 passes `ESI+0xC570` to 0x00493AA0;
constructor 0x00495FF0 builds that same member with 0x004915A0 at
0x0049600B. The latter returns its object in EAX at 0x00491890.
**Next:** bind the destination to that checked producer's return and admit
only explicit setup-register bindings as proof input facts.

### Checked setup sequences (2026-10-12)

Main 4d7f291d merged as **afbc5c96**, preserving both recorder policies and
main's slot-sweep strategy. `constructed_inputs.py --also-setup ADDRESS`
now runs additional accepted original producers on accumulated state. Every
phase must match all observations in its accepted case; changed read/call
dependencies reject. Untouched earlier pages/writes survive, later writes
take precedence, and register distributions and heap reservations stay intact.
No arbitrary merging of independently generated object snapshots is used.

**140 light tests pass.** Three new tests cover disjoint state, overwrites,
changed-read refusal, register-distribution protection and retained heap
reservations. The original 0x00437BE0 -> 0x0047DF20 sequence validates for
all three accepted seeds. 0x00493AA0 still records **0/3**; accepted data
stays **59,591 / 177,367**, with a clean `L28c/chain-audit.json`.

The proof now passes 0x00493B86 and stops at 0x00493C29 after 7,052
instructions. At 0x00493BF9/FE, ECX is `-0xC0E0 - EDI`; EBX is
`EDI + 0xC0E0` at 0x00493C04, so their addition at 0x00493C24 cancels
the input pointer. The abstract proof currently loses this relationship.
Evidence: `chain-493aa0.log`, `chain-proof-diagnose.log`. **Next:** a
general bounded affine-cancellation proof, with unknown flags preserved;
no branch-value assumptions from concrete random register addresses.

### Fixed-frame inputs and final checkpoint (2026-10-12)

**0x0081C330 records 3/3 and is adopted.** Its prefix at 0x0081C333 reserves
0x204 local bytes, released at 0x0081C64F. The old heuristic included those
516 bytes in the mock's argument count: call 0x0081C41B popped 524 instead
of 8, and RET read zero. The opt-in wrapper strategy now excludes one fixed
entry allocation only when every decoded return has the matching immediate
frame cleanup. Final frame teardown is also excluded from caller argument
cleanup. All ordinary return/page/flag checks remain; this is still a mock
input strategy, not an assertion of an unavailable callee's ABI.

**137 light tests pass. All 24 accepted wrapper-strategy cases re-record
identically**, including the earlier seven functions. The first private
recheck driver lacked Windows multiprocessing's main guard; it was stopped
and rerun as a sequential driver. It changed no accepted data. Successful
recheck: `L28c/recipes-frame-recheck-fixed.log` and
`recipes-wrapper-recheck.json`.

Final checkpoint: **59,591 functions / 177,367 cases**, cumulative **+10/+30**.
`L28c/recipes-final-audit.json` proves all **59,581 baseline records and
177,337 cases unchanged**, no promotions/removals/stale mocks. Recording ran
serially through heavy.py as codex. No Rust build/replay, shared-data write,
base-data write or push. Data remains open; base is native-train3-orig.

**Original 37-entry queue accounting:** 11 now have fixtures in this set
(two before this session, nine added here). The separate new boot stop
0x00DC7D00 makes ten additions this session. Remaining 26:

- 0x00DB8B90 and 0x00C9B350 are owned/recorded by the lifter lane; they
  were not imported into this private baseline or duplicated here.
- Nine unconfirmed entry addresses remain reserved for entry validation:
  0x00620000, 0x00620003, 0x00620004, 0x00670002, 0x00670003,
  0x007A0003, 0x007A0004, 0x007D0005, 0x00C40000.
- Fourteen finite-work residuals still reject: 0x00493AA0, 0x00494D80,
  0x004A06B0, 0x0046A150, 0x0046A2D0, 0x0048AB50, 0x0048F330,
  0x00495150, 0x00495520, 0x004A0C00, 0x0050A110, 0x00815C60,
  0x00C9E0F0, 0x00DBA780. All received the new finite-work policy;
  four also received evidenced manager setup, without an accepted result.
  Construction alone has not established a sound bound for these paths.
- 0x00D29AF0 still requires a nonempty buffer and two distinct allocation
  results. Original instructions at 0x00D29B42/45 load the buffer/count;
  calls at 0x00D29B4C/66 allocate 0x48 and 0x1000 bytes, and DIV at
  0x00D29CF3 requires nonzero count. No verified setup recipe supplying
  those inputs was established in this batch; no guessed game state added.

**Follow-up diagnosis:** the save-manager-backed 0x00493AA0 proof stops
after 7,038 instructions at 0x00493B86: it tests the value loaded through
player manager 0x018FA4EC + 0x99C (loads at 0x00493B75/7B). This is a second
manager, not state initialized by the supplied save-manager setup. Original
producer 0x0047DF20 writes that singleton at 0x0047E0DE. Substituting this
producer alone also yields three instruction-limit rejections, with no new
accepted cases. Logs: `recipes-proof-diagnose.log`, `recipes-player-manager.log`.
No broader bound was granted for the unknown branch.

**Next action:** establish a checked composition of the required manager
producers for 0x00493AA0, with explicit overlap and setup-read dependencies.
The remaining fifteen recorder entries are explicitly unfinished; this
checkpoint does not claim that all 37 requested recipes are implemented.

### Frameless virtual wrappers (2026-10-12)

**Seven functions / 21 normal-return cases adopted:** 0x005EC920,
0x00606090, 0x00606130, 0x006066E0, 0x006335A0, 0x00633EF0,
0x011590E0. `wrapper-call-v1` recognizes a bounded sequence of 32-bit
loads into volatile registers ending in a jump through a loaded register.
Stores, stack/frame operations, calls, branches and segmented loads refuse.
Such a wrapper leaves the caller arguments untouched, so its mock uses the
existing indirect-call argument heuristic, including delayed caller cleanup.
This is an explicit **mock input contract**, not recovered callee behavior.
Actual closed/platform dispatch is unchanged; older strategies are unchanged.
Sources: wrappers 0x010FEE50, 0x0106B1C0 and 0x01156B00. Tests use synthetic
wrappers, rejection forms and caller-versus-callee cleanup cases.

Private native-L28c is **59,590 functions / 177,364 cases**; cumulative
**+9/+27** this session. `L28c/recipes-wrapper-audit.json` compares against
the same baseline. **135 light tests pass**, no Rust replay. The eight-entry
trial also retried 0x0081C330: all three cases still return outside the image.
Its failure stays rejected and is being traced separately.

L49 owns C9B350 (now recorded there after its flag-liveness correction) and
the x87 caller-state follow-up; no duplicate records or x87 hook changes here.
Next: diagnose 0x0081C330's stack frame and remaining finite-work inputs.

### Boot stop 15: lazy bulk fills (2026-10-12)

**0x00DC7D00 records 3/3 normal returns** with `bulk-call-v1`. Its original
literal-size memset writes 0x100000 bytes: eagerly mapping those 256 pages
leaves no space for the caller/object/code under the ordinary page guard.
The new general strategy retains a constant-size platform memset as a lazy
fill range. Subsequent accesses materialize original pages overlaid with
the fill and still count against the unchanged page cap. Already mapped
pages update immediately. Overlapping fills and later ordinary writes retain
execution order; reads after a fill are not misreported as initial inputs.
Output uses the **existing exact byte-range schema**, including every byte
of the 1 MiB fill. No compressed-value assumption or Rust replay change.
Existing constant-size and dynamic model-byte guards remain; dynamic sizes
stay eager. Setup capture with lazy fills refuses explicitly.

Head/claude-2 #9685/#9686 identify the real boot object as constructed by
0x00DD1B70, with vtable 0x0142B0D8 and slot +0x2C = 0x005F35C0. The new
cases use ordinary zero-filled object inputs and the existing mock boundary;
they do **not** claim to replay that boot object graph. Original method byte
effects and normal return are recorded; integrated Rust replay remains head's.

Private native-L28c is **59,583 functions / 177,343 cases**, cumulative
**+2/+6** from `recipes-next-before.json`. `recipes-bulk-audit.json` proves
all baseline records/case multisets unchanged. New tests cover large fills,
post-fill reads/writes, eager equality, overlap, address wrapping, byte/page
guards and unsupported setup capture. Reproduce through heavy.py with
`structured_inputs.py record --strategy bulk-call-v1 --address 0x00DC7D00`.
Next: head boot preview, then frameless wrapper and manager input recipes.

### Constructed finite-work input: 0x00491340 (2026-10-12)

**Recorded and adopted 3/3 normal returns.** The verified original constructor
0x00490E80 returns the object in EAX; binding that value to method ECX supplies
the four arrays initialized by calls at 0x00491002. The method reads their
counts at 0x00491369. Setup replay must still match every accepted observation.
The opt-in bound now admits only bytes actually written by that setup, with
the existing alias guards; arbitrary captured page contents stay unknown.
Known method writes replace those facts. Stack-allocation helpers may move
their return address only when RET still reads the exact saved call PC.
Register XCHG preserves values and pointer identities without changing flags.

Native-L28c grows **59,581 -> 59,582 functions / 177,337 -> 177,340 cases**.
`L28c/recipes-first-audit.json` proves all previous records/case multisets
unchanged. Base remains read-only native-train3-orig. Reproduce via heavy.py:
`constructed_inputs.py 0x00491340 0x00490E80 --bind ecx=regs_out.eax
--strategy guarded-stores-v1 --seconds 45` (single command).

Fifteen finite-work residuals also received three plain guarded-store trials
each; all 45 hit the instruction limit. Manager producers were identified
from original singleton stores: 0x00437BE0 writes 0x018FA4BC at 0x004384CF;
0x004980F0 writes 0x018FA4F8 at 0x00498169; 0x004821A0 writes 0x018FA4F0
at 0x00482210. Setup trials for 0x00493AA0, 0x00494D80, 0x00495150 and
0x0050A110 still hit the limit. The two string helpers 0x00606090/0x00606130
still leave the image. None of these 18 attempts was adopted. Logs:
`L28c/recipes-bounds.log`, `recipes-managers.log`, and the structured logs.
Next: caller argument inference for frameless virtual-tail wrappers, then
continue bounded manager/record recipes. Full Rust replay belongs to head.

### Boot stop 14: 0x00DD5ED0 (2026-10-12, pending train replay)

**Records 3/3 normal-return cases and is adopted.** `guarded-stores-v1`
proves **83,137 instructions**, with the ordinary 20,000-instruction
allowance giving a 103,137 limit. The first aggregate 20-second trial
recorded two seeds; the third ran separately with its own 20-second cap.
Native-L28c grows **59,580 -> 59,581 functions / 177,334 -> 177,337 cases**.
`L28c/stored-audit.json` compares against `stored-before-hashes.json`;
the earlier movie batch remains unchanged too.

Evidence from head/claude-2 #9660-9663, confirmed against the original
instructions: 0x00DD5F21 writes vtable 0x0142B068 to the current object;
0x00DC6030 initializes members after it without overwriting that word.
The slot load at 0x00DD5F49 selects +0x2C, hence 0x00DD5F99 calls
**0x00DC67E0**. This closed callee reads +0x6CC/+0x6D0, also initialized
by the constructor; resolving the call alone without preserving those
stores would still leave its branches unproven.

The general rule is opt-in and has no game-address cases. It carries entry
pointer identities through copies, LEA, saved registers and constant offsets.
These identities are used only for exact overlap checks and loads of bytes
already written by the traced program. Entry register values, initial object
contents and unknown branches remain unknown. Closed callees and platform
memsets participate in the same write tracking. Unknown writers must pass
the existing runtime guard against every protected read, saved pointer,
instruction byte and return address. A known overlapping store replaces the
fact. The proof follows the real closed target rather than proposing a mock.

**119 light tests pass.** Five new tests cover saved-register/closed-call
survival, real recorder dispatch, overlapping and aliased writes, unknown
writer guards, and refusal to treat initial registers/objects as constants.
No full Rust replay was run. Reproduce with the normal structured recorder,
strategy `guarded-stores-v1`, address 0x00DD5ED0; private logs/scripts are
`L28c/stored-dd5ed0*`. No snapshot or shared-data write was made.
**Next:** head preview/integration replay, then the remaining constructor
and manager input recipes. Direct-route residuals are now six (including
the reserved 0x00DB8B90); one-hop residuals are 29 after the movie batch.

### Constructor/setup inputs: current batch (2026-10-12)

Main merged as **5a2d5f41**. The baseline is **59,579 functions /
177,331 cases** in native-L28c; native-train3-orig remains read-only.
Completed instruction traces are private `L28c/state-traces.jsonl`, with
disassembly/callers in `state-disassembly.txt`. The table below describes
the observed input requirement, not a claim that a guessed constructor
will fix it. `Possible` means the producer/recipe still needs confirmation.

| Residual | Input/state needed; observed blocker | Constructor/setup coverage |
|---|---|---|
| 0x00491340 | Object with four arrays starting at +0x4548; makes 256 temporary records through 0x00786DD0. Trace reaches the instruction limit in that constructor. | Not the immediate blocker: needs a sound bound for the fixed work. |
| 0x00493AA0 | Manager at 0x018FA4BC and writable output in EDI; reads manager +0xB8760 onward. Long copies exhaust the limit. | Possible manager setup; still needs bounded-copy proof. |
| 0x00494D80 | Manager at 0x018FA4F8, including flags +0x20E719/+0x21AFDA. Trace exhausts the limit in the copy at 0x00475224. | Possible manager setup; not yet a demonstrated fix. |
| 0x004A06B0 | EAX object and stack source record; calls 0x0048AC80 and copies records. Limit reached in 0x00489785. | Possible record constructor; copy bound remains. |
| 0x0081C330 | ESI object containing a pointer/count at +0xC/+0x10 and 0xD0-byte elements. Empty input gets to the epilogue but returns to zero. | Possible object graph; first resolve call/stack cleanup. |
| 0x00DD5ED0 | Constructor itself builds 128 elements, calling 0x00DC6030. The guarded proof predicts a mock at 0x00DD5F99 but execution enters closed code instead. | No: constructor already executes; keep the failed contract guard. |
| 0x00DB8B90 | Reserved boot-stop work of claude-6. | Not retried here. |
| 0x0046A150 | Iterates image table 0x01523978 and bit sets; trace at 0x0046A245 has advancing counters, not a stationary wait. | No object producer established; finite-work proof needed. |
| 0x0046A2D0 | Same image-table family, nested index loops; limit at 0x0046A3D9. | No object producer established; finite-work proof needed. |
| 0x0048AB50 | Two stack record pointers; copies 0x1BD pairs, then larger records through 0x004899EE. | A record constructor can supply contents, but the measured blocker is the copy budget. |
| 0x0048F330 | Two stack record pointers, including nested strings; trace in 0x0077AC19/0x007674B0. | Possible source-record setup; fixed nested work also needs a bound. |
| 0x00495150 | Stack record and manager at 0x018FA4F0; uses +0x36058/+0x3687C. Limit in byte-copy loop 0x00474C20. | Possible manager/record setup; bound not established. |
| 0x00495520 | Two stack record pointers, including repeated float records. Limit in 0x004FDA6A onward. | Possible record setup; copy bound remains. |
| 0x004A0C00 | EAX object with a vtable and manager at 0x018FA4BC. Limit in nested copy 0x004896D0. | Possible object/manager setup; copy bound remains. |
| 0x0050A110 | Manager at 0x018FA4BC, fields +0x1C/+0xB8760/+0xB8764 and associated records. Limit at 0x004A7D22. | Possible manager constructor; recipe unconfirmed. |
| 0x005EC920 | EAX object's +0x104 entry array and ECX index, plus 0x018FA4BC manager. Four arguments to virtual wrapper 0x010FEE50 remain on the stack; ret reads 0x015583E0. | Object graph alone does not fix mocked cleanup. |
| 0x00606090 | NUL-terminated string in ESI, stack destination object/vtable, manager at 0x018FA4BC. Four arguments to 0x010FEE50 at 0x006060CB are not popped. | Object graph alone does not fix mocked cleanup. |
| 0x00606130 | Same manager/string family; four wrapper arguments at 0x0060617B are not popped. | Same cleanup blocker. |
| 0x006066E0 | EAX object's +0x104 entries, ECX and stack indexes, nested +0xE8 array. Two arguments to wrapper 0x0106B1C0 remain before ret4. | Possible graph, but cleanup first. |
| 0x006335A0 | ESI object's +0x104 table, entries +0x18/+0x20/+0x30 and their +0x60 flags; four-argument 0x010FEE50 wrappers. | Possible graph, but cleanup first. |
| 0x00633EF0 | ECX object with +0x104 children, EAX argument and stack argument; path reaches ret4 at 0x00634083 with bad stack. | Possible graph; exact cleanup culprit still unconfirmed. |
| 0x00620000, 0x00620003, 0x00620004 | Old supplementary entries; first two exit to zero, third exhausts the limit. Their validity as function entries is unconfirmed. | Do not invent objects to rescue an unconfirmed entry. |
| 0x00670002, 0x00670003 | Same supplementary-entry issue; both leave the image at zero. | Entry validation belongs to lifter lane. |
| 0x007A0003, 0x007A0004 | Same supplementary-entry issue; both leave the image at zero. | Entry validation belongs to lifter lane. |
| 0x007D0005, 0x00C40000 | Same supplementary-entry issue; both leave the image at zero. | Entry validation belongs to lifter lane. |
| 0x00815C60 | EDI object with fields +0x1224/+0x1254/+0x1244 and record arrays. Limit while searching 0x74 elements at 0x0081A510. | Possible initialized array; search/call bound still required. |
| 0x00C9B350 | ECX data and ESI destination; `idiv` at 0x00C9B3D9 followed by `inc edx` at 0x00C9B3DB triggers the undefined-flags guard. | Not an object-input fix on this path; flag-liveness question for lifter lane. |
| 0x00C9E0F0 | EAX source object plus stack output buffer, serialization helpers 0x00D009F0/0x00D01020. Limit in repeated ten-word groups at 0x00C9E167. | Possible valid source counts/buffer; producer unconfirmed. |
| 0x00D29AF0 | ECX object: byte buffer at +4 and nonzero byte count at +8 (loaded at 0x00D29B42/45). The count divides at 0x00D29CF3. Also needs two distinct allocation results for +0x31C/+0x320. | Possible constructor with nonempty buffer; zero-call mocks also alias the two allocations, so construction alone is not sufficient. |
| 0x00DBA780 | Destructor scans 16,384 slots at object +0x40D8, then manager 0x018D0AA0 and other arrays. | Constructor may supply ownership state, but all-zero slots still require a large finite scan. |
| 0x00F7DE40 | Movie state at this+0x2C must reach 9 (checks at 0x00F7DE49/59). Constructor 0x00F7BA50 writes 0 at 0x00F7BAA5; reset 0x00F7B170 writes 9 at 0x00F7B244. | Reset-first is the evidenced recipe; constructor alone cannot release this wait. |
| 0x011590E0 | Object +4 vtable and manager 0x018D0CC4, with +0x21C/+0x214 children. Two arguments to wrapper 0x01156B00 remain at the epilogue. | Possible graph, but mocked cleanup first. |

The general `constructed_inputs.py` strategy re-executes an accepted original
setup case and requires its entire recorded result to match before transferring
pages/register bindings to a new method run. No game fields are assigned by the
strategy. Constructor/reset external calls retain their accepted mock boundary;
this is fixture state, not proof of a fully initialized game runtime.
The method records ordinary reads/writes, so it needs no generated Rust change.
Setup identity, seed, case hash and register bindings are retained as provenance.
Generic refresh refuses these cases rather than silently losing their setup.
Image-byte budget proofs are disabled for transferred state, and allocation
after an allocating setup is rejected until the replay supports an initial
heap cursor. Existing return, page, flag and instruction guards remain enabled.

**Recorded:** 0x00F7DE40 returns normally in **3/3** cases after the verified
0x00F7B170 reset. Each setup reproduces every field of its accepted case;
legacy random seeds are identified from the first GPR draw and checked by
that full recorded-result comparison. No accepted setup case is rewritten.
Native-L28c is now **59,580 functions / 177,334 cases** (+1/+3).
`L28c/constructed-audit.json` proves **all 59,579 old functions and their
177,331 cases unchanged**, with no promotions, removals or stale mocks.
Six synthetic tests cover state transfer, unchanged capture output, legacy
seed identification, mismatches, guards, executable writes and heap reuse.
The combined light suite has **114 tests**; full Rust replay remains for the
head's integration train. No snapshot was made; data stays open.

Reproduction (set LIFT_DATA/INVENTORY_EXTRA to native-L28c; use the research
Python and heavy.py as `codex`):
`research/native/constructed_inputs.py 0x00F7DE40 0x00F7B170 --bind ecx=regs_in.ecx`.
The command logs to `structured-constructed-v1.jsonl` without adoption;
`structured_inputs.adopt(Path(LIFT_DATA), 'constructed-v1')` performs the
normal append-only adoption. It refuses to record an already selected method.
The hash baseline is `L28c/state-before-hashes.json`; compare with
`research/native/audit_records.py compare --root ... --manifest ... --report ...`.

**Next:** head priority #9661/#9663 is boot stop 14, 0x00DD5ED0. Its own
vtable store at 0x00DD5F21 survives 0x00DC6030 and resolves the indirect
call at 0x00DD5F99 to 0x00DC67E0. Implement a general alias-checked rule,
record this constructor first, then return to the remaining input recipes.

(draft; numbers after the pause lifts)

**Census (native-L17, after L17's recording).** The 116 rows are not
excluded from any scan: `structured_inputs.py prepare` takes every row whose
status is not `verified`. The status is only stale because the inventory
was not rebuilt. They split into:

| Group | Rows | Why no fixture |
|---|---:|---|
| Lift rejections | 68 | lifter: x87-return/unsupported, STMXCSR/LDMXCSR/FNCLEX, RCR (all six 64-bit division helpers), the memcpy 0x012EDE40 table, PMAXSW/PSLLDQ/RSQRTPS/PAUSE/REPNE. Handed to claude-6; most are fixed on `task/L22-lift-rejections` (STMXCSR/LDMXCSR remain: guest MXCSR needs sticky SSE flags) |
| Recorder failures calling the SEH helpers | 28 | the recorder mocked `__SEH_prolog4`/`__SEH_epilog4`, so the frame never moved and the function's `ret` popped its scope table (e.g. `__setmbcp`: "returned to 0x0161EBE0" under every strategy) |
| Other recorder failures | 16 | noreturn and unwind paths (`_abort`, `__exit`, `___crtExitProcess`, `__math_exit`, `__global_unwind2`, `_EH4_GlobalUnwind2`, `__CallSettingFrame@12`), eight `FUN_` and `vf14` |
| Fixtures from L17's passes | 4 | `vf20` 0x0130A800, `vf31` 0x0130AAC0, 0x013201C0, 0x01320700 |

Across all 418 remaining candidates, 33 call an SEH helper.

The 16 other recorder failures, with every reason seen across the L17
passes (all strategies):

| Address | Function | Reason |
|---|---|---|
| 0x012ED9DD | `___crtExitProcess` | noreturn call (ExitProcess path) |
| 0x012EDC4B | `__exit` | noreturn target |
| 0x012EE1A1 | `vf14` | noreturn target |
| 0x012F9E95 | `_abort` | noreturn target |
| 0x013006FE | FUN_013006FE | unmasked x87 exceptions (FLDCW input) |
| 0x0130070B | `__math_exit` | unmasked x87 exceptions (FLDCW input) |
| 0x012FF0F0 | `__CallSettingFrame@12` | execution outside the image (calls a funclet address from its argument) |
| 0x01303CD2 | `_EH4_GlobalUnwind2` | execution outside the image |
| 0x01306FD0 | `__global_unwind2` | execution outside the image; overwritten return |
| 0x012E1DA1 | FUN_012E1DA1 | execution outside the image |
| 0x012F9D82 | FUN_012F9D82 | execution outside the image; runaway |
| 0x012F0D4C, 0x012F37AD, 0x012F45C4, 0x0132A1EC | FUN_ | NaN/Inf float input |
| 0x01314A90 | FUN_01314A90 | NaN/Inf float input; runaway |

The four noreturn bodies cannot have a returning fixture under the current
contract (the recorder rejects noreturn paths by design); the unwind group
needs exception-dispatch state (a runtime task, L15/L17).

**Rule: promotion of verified call-free callees.**
`structured_inputs.py promote` re-lifts a selected ext entry as a closed
callee when its arms make no call at all and some unverified candidate
calls it directly. Its own cases are re-recorded in closed mode with its
previous input distribution. The recorder (`run_sparse`'s `lifted` set) and
the replay (dispatch table) both run closed targets for real, so a case that
mocked a promoted entry would change meaning. Hence:

- An ordinary call-free entry that some recorded case mocks stays ext: a
  mock with arbitrary returns is a faithful stand-in for it.
- A caller-frame helper (derived `frame_exit`, L17) is promoted even when
  mocked, because no mock can move the caller's frame. The callers whose
  cases mock it are re-lifted against the new closed set and re-recorded
  with their previous distribution. The helper is promoted only if every
  one of them records again; otherwise it stays ext (iterate to a fixpoint),
  so no verified function is lost.

Promoted helpers store `frame_exit`, and `run_sparse(frame_contracts=)` ends
their nested frame at the return PC with exactly that ESP. The prolog
returns below the call's ESP, and the ordinary `ESP >= call ESP` test would
leave the caller marked nested, which breaks mocking and the own-return
check. A synthetic regression test shows the caller rejected without the
contract and recorded with it.

Two corrections during the run. (1) The first draft also required "no
recorded case mocks it". That excluded the SEH helpers themselves: 21 cases
of 8 verified functions (0x012EDAF5, 0x012F914F, 0x012F933B, 0x012F95B6,
0x012F96FD, 0x012FAD40, 0x01302F75, 0x01304FEE) mock both helpers. That run
promoted 11 unmocked call-free entries, 0x01306F7F among them. (2) Dropping
that gate entirely made 378 widely mocked targets (102,442 recorded calls)
eligible and started re-recording most of the program. I stopped it before
it wrote anything and restricted the mocked case to caller-frame helpers.

**Verification process.** Under the batched-verification rule
(AGENT-RULES.md, 2026-10-11) this branch records on its own data set and
runs the Python tests. The full gen + required-vector replay is the head's
integration train.

### Batch 1 (recorded on native-L17; replay is the head's train)

**Promotion.** 13 entries were promoted to closed callees. 11 were unmocked
call-free entries: 0x004021A0, 0x00D93610, 0x0129F000, 0x0129FB20,
0x012F51DC, 0x012F9EC8, 0x012FE663, 0x01306F7F (`__EH_prolog3` body),
0x01306FB5, 0x013070DC, 0x01314A30. The other 2 are the caller-frame
helpers `__SEH_prolog4` 0x012F71E0 and `__SEH_epilog4` 0x012F7225. All 8
verified callers whose 21 cases mocked the helpers re-recorded under the new
boundary, so no helper stayed ext: 0x012EDAF5 (`_doexit`), 0x012F914F,
0x012F933B, 0x012F95B6, 0x012F96FD, 0x012FAD40 (`___updatetmbcinfo`),
0x01302F75, 0x01304FEE.

**Recording** (fresh prepare after promotion: 418 candidates; L17's pass
protocol; then a `--retry 'memset of|memcpy of'` pass for the
constant-size rule):

| Pass | Added functions |
|---|---:|
| random (3 seeds) | 30 |
| objects-mockzero-v1 (3 seeds) | 1 |
| other 3-seed passes, the 12-seed passes | 0 |
| memset/memcpy retry, all strategies | 9 |
| **Total** | **40** |

| Measure | L17 final | Batch 1 |
|---|---:|---:|
| Selected lifted entries | 58,989 | 59,029 |
| Cases | 175,600 | 175,719 |
| Liftable candidates without a fixture | 418 | 378 |

Audit (`L28/audit_l28.py`, per function against L17's final pair):
exactly 21 records changed, the 13 promoted and 8 re-recorded; all other
58,968 functions' records and cases are byte-identical; 40 are new.

What records now. 26 of the 116 `platform` rows: `__setmbcp`,
`___updatetlocinfo`, `___tzset`, `__isindst`, `__fsopen`, `_fread_s`,
`_fclose`, `__fseeki64`, `_fseek`, `_fprintf`, `__read`, `__lseek`,
`__close`, `__commit`, `__alloc_osfhnd`, `__getstream`, `flsall`,
`__fcloseall`, `__sopen_helper`, `__exit`, `_Type_info_dtor`,
`___DestructExceptionObject`, `CallCatchBlock`, `___BuildCatchObject`,
`___BuildCatchObjectHelper`, `___FrameUnwindToState`. Of claude-7's
title-path list: `sSave::ctor` 0x004980F0, 0x00499830, 0x00499880,
0x00DCE2E0, `sShader2::ctor` 0x00E121D0 (constant-size memsets), `_exit`,
`__lseeki64` and `__write`.

**Unverified.** The batch has not been replayed in Rust on this branch
(batched-verification rule). The head's train runs gen + the gate on it.

**Remaining 378 candidates**, primary reason across all strategies:
execution outside the image 190, NaN/Inf float input 76, runaway 73,
noreturn call 15, two NaN operands 5, undefined flags 4, overwritten return
5, CPU exception 2, memset (non-constant size) 2, unmasked x87 2, misaligned
stack 1, noreturn target 1, page limit 1, x87 stack fault 1. Owners:
NaN/two-NaN go to claude-6's oracle work (L22 retires both guards, so they
need a retry after that merges). Runaway: claude-6's constant-loop budget
covers 8; stack-slot counters and value-tracked starts are open.
Outside-image is the recorder's mock-ABI class (L11-L13), next for this
branch. Noreturn call needs a terminal-fixture contract (below).

**Terminal fixtures (proposal, not implemented).** 15 candidates always
reach a noreturn call. Examples: 0x00D0CB00, a fatal-error reporter
(format, callback, `_exit(1)`; callers include D3D init 0x00DB1480), and
`___crtExitProcess`/`_abort`. A case could end at the noreturn call with the
state recorded there; replay would stop at that call_ext with a sentinel
and compare state. This needs the generated test template (lift.py) as well
as the recorder.

**Adoption (head, for a train).** Branch `task/L28-crt` (on top of
`task/L17-seh-workers`, merged as a097dfb): 522d784, 2018ea3, ede0c63,
de8880d, 0421471, plus this Results commit. Private data: native-L17's
`patterns/lift.json`, `vectors/lift.json`, `patterns/lift_novector.json`,
`patterns/lift_novector_keys.json`, `structured_quarantine.json`,
`quarantine_evidence.json`; evidence in `structured-*.jsonl`,
`structured-*-adopted.json`, `structured-promoted.json`, `L28/`. Merge
note: `research/native/lift.py` has a four-line `frame_contracts` plumbing
in `vectors()`, next to claude-6's L22 lines (claude-6 informed).

### Batch 2 (recorded on native-L17; replay is the head's train)

Recorder changes (general rules, each with synthetic tests):

1. **Mock cleanup.** `static_pop` searches up to the lifter's body limit
   (20,000 instructions) instead of 512. 0x0040AB20 ends in `ret 4` after
   640 instructions, and the old zero fallback left its argument on the
   stack. `IMPORT_CLEANUP` gives 45 imports their documented x86 cleanup
   (Microsoft Learn signatures; steam_api is cdecl), replacing L15's
   three-entry list. Traces behind this: 0x004083A0 returned 4 bytes low
   after `push eax; call 0x0040AB20`; 0x0130A5C0's QueryPerformanceCounter
   mock popped its local frame along with its argument; 0x01106C80's IMM32
   thunks fell back to zero cleanup.
2. **Constant model sizes.** A memset/memcpy size pushed as an immediate
   and not overwritten counts against a separate 4 MB per-run budget.
   Example: `sSave::ctor` clears 0x80000 bytes.
3. **Noreturn roots.** The noreturn-target guard exempts the root entry
   (`__exit` returns syntactically).
4. **Terminal fixtures (terminal-v1).** Head-approved. They touch lift.py's
   generated test template; see NATIVE_PIPELINE.md. claude-6 merged them
   into L22 and adds MXCSR to the Terminal payload there.
5. `record --retry REGEX` and `record/adopt --terminal`.

| Pass | Added functions |
|---|---:|
| retry outside-image/overwritten-return/misaligned: random | 46 |
| same, objects-mockzero-v1 / mockzero-v1 / zero | 2 / 1 / 1 |
| terminal: random (zero, mockzero-v1: 0) | 5 |
| **Total** | **55** |

| Measure | Batch 1 | Batch 2 |
|---|---:|---:|
| Selected lifted entries | 59,029 | 59,084 |
| Cases | 175,719 | 175,883 |
| Liftable candidates without a fixture | 378 | 323 |

Audit (`L28/audit_b2.py`, against the head's snapshot native-L28b1): no
record changed, 55 are new.

By claude-7's L38 queue: 9 title path (the IME setup 0x01106C80, 0x01106BB0,
the clock helper 0x0130A5C0, the Steam callbacks 0x004083A0,
`sSoundExt::vf06` 0x004AD440, 0x00DFA0C0, 0x00E04A90, and the terminal cases
0x00D0CB00 and `__amsg_exit`); 18 gameplay (`uPlayer::ctor` 0x00B55900,
`uNpc::ctor` 0x00BAB3A0, `uCmc::ctor` 0x00B53220, `uPlayerBase::vf23`,
`uCharacterBase::vf80`, `uModel::vf21`, `uGUICompound::vf05`,
`sPrimitive::vf16`, `_abort` (terminal) and 9 FUN_); 28 elsewhere.

**Unverified:** all 55, plus the template change. Terminal cases replay
only with this branch's lift.py template (or L22's merge of it). Data note:
claude-6's native-L22c also starts from L28b1. The two pairs add disjoint
functions as far as I know (L22 adds lift-rejection fixes, this branch adds
recorder-failure fixes), so adopting both means appending each one's new
records and cases to L28b1.

**Remaining 323 candidates.** NaN/Inf and two-NaN inputs wait for L22's
oracle merge, then a `--retry`. Runaway: the constant-loop work is
claude-6's; stack-slot counters and value-tracked starts are open. The rest
of the outside-image class needs per-trace diagnosis (indirect calls
through random data, internal entries).

**Adoption (head):** branch `task/L28-crt` commits after 0c9e87b
(afe22df, 3bf9559, the train-1 merge, this Results commit); private data:
native-L17's `patterns/lift.json`, `vectors/lift.json`,
`patterns/lift_novector.json`, `patterns/lift_novector_keys.json`,
`structured_quarantine.json`, `quarantine_evidence.json`.

### State at hand-off (2026-10-12, wrap-up requested by the head)

**Batch 3, recording in progress (run 4).** These commits came after
batch 2: f43e7ff/7000f4b/307287b/fcaadc4 plus the alloca-probe work:

- `moves_frame` classifies ESP moved by a value (`__alloca_probe`) as an
  unmockable frame helper, closed with `frame_exit: 'return-pc'` (nested
  frame ends at the first return-PC arrival).
- `local_alloc_sites` recognizes entry allocations through branches.
- promote's re-recording falls back across the standard input distributions.
- An indirect call's merged deferred caller cleanup is subtracted from its
  mock pop.

Run 4 (`L17BC/l28c.sh`, log `L28/run4.log`) archived the batch 2 trial logs
to `L28/trials-b2/`. It then promoted 0x00DB9C30 (call-free, unmocked). It
kept `__alloca_probe` ext: under that run's code only
`__alloca_probe_16`/`_8` failed to re-record, and the distribution fallback
committed afterwards fixes that (checked by a diagnostic: the other 21
mocking callers re-record). It is recording the full protocol; at the time
of writing native-L17 holds 59,089 entries / 175,898 cases. The local-alloc
and merged-cleanup changes landed while run 4's workers were running, so its
later passes mix recorder versions. That is harmless for validity: a mock
pop only ever leads to rejection, never a false pass. It does make
per-rule attribution for run 4 unreliable. The head adopts native-L17 after
run 4 ends; audit it with `L28/audit_b2.py`, pointed at native-L28b2 as the
base.

**Not yet recorded (run 5, ready).** It promotes `__alloca_probe` (29
remaining candidates call an alloca probe) with the fallback. It also
re-records everything under the merged-cleanup rule, which by trace makes
the sShader2 loader 0x00E0E470 record (3/3 seeds, 8 mocked calls).
Commands:

```
# after run 4 has finished (no other job of mine writing native-L17):
E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py <who> -- \
  "C:/Program Files/Git/bin/bash.exe" E:/ddda-re/notes/native-L17/L17BC/l28d.sh \
  > E:/ddda-re/notes/native-L17/L28/run5.log 2>&1
```

`l28d.sh` archives the trial logs to `L28/trials-run4/`, then runs
`structured_inputs.py promote`, `prepare`, and record+adopt for: the 9
strategies at 3 seeds / 20 s; the 3 zero-return strategies at 12 seeds /
60 s; random/zero/mockzero-v1 with `--terminal`. It sets `LIFT_DATA`,
`INVENTORY_DB` and `INVENTORY_EXTRA` to native-L17's copies and 6 workers.
Then hand the pair to the train; no local Rust replay (batched rule).

**Remaining candidate classes** (323 before run 4; about 318 now):
- Outside-image (about 160): mock-ABI residue that needs per-trace work.
  Seen in samples: mid-function supplementary entries (0x004E4145),
  indirect targets through random data, callee cleanups static_pop can't
  prove.
- NaN/Inf and two-NaN inputs (about 80): retry with
  `record --retry 'NaN'` once L22's hardware oracle is on main.
- Runaway (about 73): constant-trip loops with calls or stack-slot
  counters. claude-6 covers 8; stack-slot counters and value-tracked starts
  are open.
- Noreturn calls on non-root paths, and a residue of terminal-ineligible
  noreturn paths.
- Small classes: undefined flags, unmasked x87, page limit, CPU exceptions.

The lift rejections (STMXCSR/LDMXCSR, x87 forms, RCR and others) are
claude-6's (L22).

### L28b takeover: run 4 audit and run 5 (2026-10-12)

**Batch ready (codex, task/L28b-crt): +25 functions / +70 cases in run 6,
59,115 entries total; alloca promotion and its 23 caller refreshes audited.
33 light tests pass; Rust replay belongs to the integration train.**

Run 4 printed `run4 done` before this work touched native-L17. Its final
pair contains **59,090 selected entries / 175,899 cases**, leaving **317**
prepared candidates without fixtures. The inherited `L28/audit_b2.py`,
pointed at the head's native-L28b2 snapshot, reports **6 additions / 16 new
cases**, no removals and no unexpected edits; the only existing entry
changed is the promoted 0x00DB9C30.

The six additions are 0x007F2120 (random), 0x007DF790
(objects-mockzero-v1), and 0x004AB440, 0x00E0E470, 0x00E0E5A0,
0x00E111A0 (small-mockzero-v1). In particular **the shader loader
0x00E0E470 already recorded in run 4**. Run 5 preserves accepted entries;
promotion refreshes the helper and its previously mocking callers, and
the strategy passes retry the remaining candidates. It does not refresh
every accepted record.

Run 5 uses the supplied `L17BC/l28d.sh` with only its working-directory
line changed to this L28b worktree. It runs through `heavy.py codex`, with
native-L17's LIFT_DATA, INVENTORY_DB and INVENTORY_EXTRA and six workers.
Recorder source stays fixed throughout the run. No private data snapshot
was made. `research/native/audit_records.py` records hashes and addresses
only, then checks the complete selected pair: existing record hashes and
case multisets must match, except newly closed helpers and callers whose
old cases mocked them. Those callers must receive new cases, and no case
may still mock a newly closed helper. Missing entries also fail the audit.

Run 5 finished at 02:32:03: **59,090 entries / 175,899 cases**, zero
additions and zero promotions. `L28/run5-audit.json` reports **all 59,090
existing entries unchanged**, including their case multisets. No removals,
unexpected edits or stale mock cases. All prescribed strategy passes ran.

**Why the fallback still failed.** The existing recorder always initializes
ESP to a multiple of 16. The alignment wrappers 0x012F4E90 and 0x012F4EA6
compute alignment from `ESP + 4 - EAX`. Even zero EAX therefore allocates
four bytes, and their exit ESP equals entry ESP; the ordinary stack guard
rejects it. Merely switching among the existing input distributions cannot
exercise their zero-allocation path.

The new opt-in `zero-call-v1` uses the zero-input distribution with entry
ESP four bytes below an aligned caller stack (ESP modulo 16 = 12).
It adds no address special case or stack-contract exemption, preserves all
existing distributions and checks, and is included in promotion fallback.
Synthetic 8-/16-byte alignment tests reproduce the old rejection and the
new valid return; the promotion test proves this distribution is tried for
an affected caller. The existing rejection test still forbids promotion
when a caller cannot re-record. **33 light tests pass** (16 structured-input,
13 frame-contract, 4 audit).

Run 6 (`L28/run6.sh`, `L28/run6.log`) starts with an assertion against each
original alignment wrapper, then promotes, prepares, and records/adopts
random, zero, zero-call-v1 and small-mockzero-v1 (three seeds, 20 seconds).
It moves old trial logs aside; no selected-data snapshots are taken. Since
run 5 preserved the entire pair, the run-5 hash manifest is also the exact
run-6 baseline. Both original wrappers rejected under the old zero strategy
and returned with ESP increased by four under zero-call-v1, **3/3 seeds
each**. Promotion then closed **0x012F2280**, re-recorded all **23** existing
mocking callers and kept every selected entry (59,090 / 175,901 cases at
that point). Run 6 finished at 02:49:42:

| Measure | Before run 5 | After run 6 |
|---|---:|---:|
| Selected entries | 59,090 | 59,115 |
| Recorded cases | 175,899 | 175,969 |
| Liftable candidates without a fixture | 317 | 292 |
| Lift rejections (this branch, before L22 integration) | 157 | 157 |

Random added 17 functions / 44 cases; zero added 8 / 24. Promotion added
two cases while refreshing the existing helper/callers. The new aligned
strategy enabled promotion; it added no functions directly in its own
pass. Seven new entries occur in TITLE_PATH.md: **0x004403A0, 0x00448500,
0x0044E6F0, 0x004709D0, 0x004A3260, 0x00F9FBA0, 0x00FEE130**.

**Preservation proved:** `L28/run6-audit.json` reports **59,066 unchanged
old records and case multisets**. Exactly 24 existing entries changed:
the promoted 0x012F2280 and its 23 previously mocking callers. No entries
were removed, no unrelated entries changed, and no cases still mock the
promoted helper. The report lists every added and refreshed address.

**Adoption / verification:** native-L17 is frozen after run 6 for the head
to snapshot and adopt. Use its `patterns/lift.json`, `vectors/lift.json`,
`patterns/lift_novector.json`, `patterns/lift_novector_keys.json`, plus the
unchanged quarantine metadata. This is a boundary-changing pair, not just
25 append-only additions: when combining another lane's data, re-record
any imported case still mocking 0x012F2280. No Rust build or replay was
run here, per batched verification; all 25 additions and the refreshed
24 entries await the integration train. The original-wrapper assertions
and the 33 light tests passed. No shared native/inventory data was written,
and no snapshots were taken by this agent.

The audit can be repeated without modifying the pair:

```
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/audit_records.py compare --root E:/ddda-re/notes/native-L17 --manifest E:/ddda-re/notes/native-L17/L28/run5-before-hashes.json --report E:/ddda-re/notes/native-L17/L28/run6-audit.json
```

**One next action:** integration replay of this pair; then resolve the
image-table/stack-slot bounded loops below with the lifter lane. NaN and
instruction rejections still wait for L22 integration. The outside-image
terminal-call cleanup finding remains a diagnosis, not a shipped fix.

**Title-path follow-up evidence (not fixes).** Of the 317 run-4 residuals,
62 addresses occur in TITLE_PATH.md (this is a document intersection, not
a newly proven reachability closure). Private evidence is in
`L28/run4-title-residue.json` and `L28/title-loop-evidence.json`.

- Archive registration 0x00419100 reads ten descriptors via 0x0153B6BC.
  Their image-backed counts total 13,061 records. Its six-instruction
  inner loop at 0x00419113 already requires 78,366 instructions, above
  the default 20,000 budget. Random inputs cannot remove this loop.
- `sObjCollision::ctor` initializes a stack-slot counter to 799 at
  0x0047862B and decrements it at 0x004786B5 (800 iterations).
  `sNpcManager::ctor` also has a stack-slot counter initialized at
  0x0047549A and decremented at 0x00475540. These require bounded-loop
  analysis beyond the register-counter forms in the L22 lane; sent to
  head and claude-6 on the board.
- 0x00DB8B90 pushes one argument for 0x00DB8AC0. The latter's normal
  exit 0x00DB8B65 tail-jumps to LeaveCriticalSection through 0x0139D164
  (four-byte cleanup), but its other path calls 0x004080A0 at 0x00DB8B7F
  then reaches INT3 at 0x00DB8B84. The recorder's static cleanup analysis
  returns zero because of that trap path. This is a candidate diagnosis,
  not a proved recording fix: terminal-path classification needs evidence
  before changing the conservative fallback. The next link is concrete:
  0x004080A0 calls inventory symbol `__invoke_watson` (0x012EDDD5) at
  0x004080C5, then reaches INT3 at 0x004080CB. Neither name is covered by
  the current four-symbol `noreturn_entries` set. Sent to the lifter lane.

### L28c: boot partition and imported alloca boundary (2026-10-12)

**Batch ready: boot blocker 0x00DAFED0 has eight accepted fixtures;
0x00F6A380 is refreshed. 59,534 entries / 177,196 cases, both changes
audited separately. 39 light tests pass; Rust replay awaits the train.**

Merged main into task/L28b-crt at **3d8ac913**, retaining both sides of the
milestone-note conflict. Harness and structured-input changes merged
automatically; all 33 preceding light tests passed. This batch uses only
**native-L28c**, initially **59,533 selected records / 177,188 cases**.
native-L17 stays frozen; native-train3 is untouched. INVENTORY_EXTRA points
to native-L28c/extra_functions.tsv; the shared inventory database is opened
read-only (no private database was supplied with this data set).

**Boot blocker 0x00DAFED0:** random, zero, small-v1 and objects-v1 each
rejected as `runaway` in the original recorder, with its ordinary 20,000
instruction limit. Read-only disassembly/decompilation establishes:

- First and last are entry-stack arguments +8/+12 (loads at 0x00DAFEE2
  and 0x00DAFEDD); +4 is the output iterator-pair pointer.
- The distance is converted to a midpoint and multiplied by **0x5C** at
  0x00DAFF06. Median selection calls 0x00DAFD10 at 0x00DAFF12.
- Comparisons use two unsigned words at record offsets **0x50/0x54**
  and a float at **0x58** (e.g. 0x00DAFF30..0x00DAFF4C).

Independent random pointers/counts do not describe this bounded range;
zeroing both pointers still does not supply the nonempty median-selection
precondition. The general `record-range-v1` strategy takes a private JSON
layout describing stride, first/last/result stack or register bindings,
and finite float/unsigned-key fields. No function-address dispatch is
used. It fixes all initial bytes before execution, keeps image reads and
guest writes unchanged, and records the layout for exact re-recording.
Its seeded cases include ascending, descending, equal and mixed keys,
3/5/8/17/33 records, and page-crossing records. No budget or rejection
check is relaxed.

The baseline audit also finds exactly one imported stale caller:
**0x00F6A380** still mocks already-closed **0x012F2280**. The generic
`refresh-closed` operation re-lifts/re-records only such callers, preserving
the current lifter's x87-entry and instruction-budget metadata. A failed
caller refresh writes neither selected file. This repairs an imported
boundary mismatch without re-promoting or demoting unrelated entries.

**Original-code checks:** all four old distributions rejected. The bounded
range produced **8/8 accepted trials** without increasing the instruction
budget. Every trial also passed independent checks on its recorded writes:
returned bounds are inside the input range and record-aligned; the output
records are a permutation of the inputs; and the less/equal/greater
partitions satisfy the lexicographic two-uint/float comparator. Counts
3/5/8/17/33 and equal-key cases are exercised. These are original-code
recording checks, not a claim that Rust replay has passed.

The harness owner agreed to the five input-only hooks in board #9531;
the source patch was then applied. **39 light tests pass** (19 structured
inputs, 13 frame contracts, 2 range-generator tests, 5 audit tests). The
refresh tests also check failure before pair writes and removal of stale
optional x87/budget metadata when the current lifter no longer emits it.

The first refresh job was terminated before reporting a result. Its hash
audit confirmed all 59,533 entries and 177,188 cases unchanged. The retry
repaired only 0x00F6A380; `L28c/refresh-audit.json` confirms **59,532 old
records/case multisets unchanged**, no removals, no unrelated changes,
and no remaining mocks of 0x012F2280. Counts stayed 59,533 / 177,188.

**Final pair:** **59,534 records / 177,196 cases** (+1 / +8); missing
prepared candidates **229 → 228**, with **20 lift rejections / 3
quarantines** unchanged. `L28c/partition-audit.json` compares against the
post-refresh hash manifest and proves **all 59,533 pre-existing records
and case multisets unchanged** during adoption. Its only addition is
0x00DAFED0. Together with the preceding refresh audit, exactly one original
entry changed (0x00F6A380) and exactly one was added. No snapshots were
taken; the manifests contain hashes/addresses only.

**Private evidence:** `L28c/diagnose.log`, `partition-trial.json`,
`partition-check.log`, `refresh-audit.json`, `partition-audit.json` and
`partition-layout.json` under native-L28c. The eight trial cases were
recorded through heavy.py using the exact proposed hook in an isolated
process while waiting for the shared-file owner. After agreement the
same hook was applied to source and light-tested normally. Adoption and
hashing execute no guest code; that light step ran directly with 16 GB
free after cancelling its unnecessary queued heavy wrapper.

**Reproduction for a missing candidate:** set LIFT_DATA to
`E:/ddda-re/notes/native-L28c` and INVENTORY_EXTRA to its
`extra_functions.tsv`, then run through heavy.py as codex:

```
python research/native/structured_inputs.py record --strategy record-range-v1 --layout E:/ddda-re/notes/native-L28c/L28c/partition-layout.json --address 0x00DAFED0 --seeds 8 --seconds 20
python research/native/structured_inputs.py adopt --strategy record-range-v1
```

Use the research Python interpreter specified above. The private layout is
`stride=92`, `first={stack:8}`, `last={stack:12}`, `result={stack:4}`,
`uint_offsets=[80,84]`, `float_offsets=[88]`. `--address` is an optional
candidate filter, not an address-specific implementation rule. Successful
existing functions are not re-recorded by `record`. `refresh-closed`
repairs imported stale closed-call boundaries; its audit requires explicit
`--refresh-closed 0x012F2280` authorization of that exact target.

**Adoption:** native-L28c is frozen for head's snapshot/merge. Take its
selected pair and updated `patterns/lift_novector.json` /
`patterns/lift_novector_keys.json`; quarantine metadata is unchanged. No
full Rust replay was run here. The next action is integration replay and
resuming the real boot, then prioritizing the next executed title-path
failure. Image-table/stack-slot loop budgets and the terminal-call cleanup
diagnosis above remain open; coordinate with L45 before changing extents
or terminal evidence. Both L45's `refresh`/`table_goals` and this branch's
separate `refresh-closed` command must be retained when merging.

### L28c render cleanup batch (2026-10-12, boot preview ready)

**59,534 → 59,542 selected records; 177,196 → 177,220 cases.** Eight
additions, three original-code fixtures each: **0x00DAA4B0, 0x00DAE760,
0x01128EE0, 0x01128F40, 0x01129030, 0x01129310, 0x01129370,
0x01129460**. All 59,534 old records and case multisets are hash-identical
(`native-L28c/L28c/title-cleanup-audit.json`); no removals, promotions or
stale mocked targets. Missing prepared candidates: **228 → 220**.

The recorder's indirect-call cleanup overcounted arguments when a later
stdcall/thiscall consumes an argument pushed before the indirect call.
Concrete evidence: 0x01128F4F pushes the diagnostic string, 0x01128F54
pushes the COM receiver, and 0x01128F55 calls Unlock. 0x01128F57 then
pushes HRESULT and 0x01128F5A calls 0x00DA89B0, whose return at
0x00DA8C15 is `ret 8`. Unlock consumes four bytes, leaving the string for
the later call. The analogous Lock sequence is 0x01128F19..0x01128F31.
The general forward cleanup scan now accounts for the later direct
callee's static pop, reserving its pre-pushed prefix. No address-specific
case, changed input distribution, or relaxed execution guard is involved.

**41 light tests pass:** 15 frame-contract (two new regressions), 19
structured-input, two range-generator and five audit tests. The new tests
cover the saved prefix and a subsequent callee consuming only intervening
pushes before explicit caller cleanup. Original recording ran through
heavy.py; seven additions use zero-call-v1, 0x00DAE760 uses zero-objects-v1.
Rust replay remains for the head's integration train.

Before this change all 29 remaining direct-title candidates rejected with
three seeds in each of zero-call-v1, zero-objects-v1, small-mockzero-v1 and
image-v1. Private evidence: `L28c/title-diagnosis.jsonl` and
`L28c/title-cleanup.jsonl`. The broader one-hop trial was stopped on head's
new priority order, preserving its completed rows. **native-L28c is not
frozen**, per head: 0x01129030 and 0x01129460 are ready for preview now;
0x00DA9EC0 still rejects and is being traced first, then 0x01104470.
Constructor constant-loop analysis remains open (stack counters, copied
initializers and closed-callee cost); no speculative budget change landed.

### L28c priority boot-route follow-up (2026-10-12)

**0x00DA9EC0 and 0x01104470 now record 3/3 each.** The preceding batch
already added 0x01129030 and 0x01129460, so all four requested boot-route
functions have accepted fixtures. Private pair: **59,542 → 59,544 records,
177,220 → 177,226 cases**. `L28c/boot-route-audit.json` compares against
the post-render hash manifest and proves every old record/case multiset
unchanged. Missing prepared candidates: **220 → 218**. No promotions,
removals or unrelated changes; native-L17 and native-train3 remain untouched.

For 0x00DA9EC0 the original trace reached its own return at 0x00DAA4A4,
but ESP was four bytes too high. At 0x00DA9F2E the COM call is followed
by `mov ecx,[esp+0x20]` (0x00DA9F30), before the HRESULT push and call
to 0x00DA89B0. The cleanup scan previously stopped at that neutral setup
because it had not yet seen a later push. It now crosses non-branching,
non-ESP-writing instructions before as well as after that push. Its
24-instruction bound and branch/call/stack barriers remain. A synthetic
MOV case passes; replacing it with a jump still rejects, demonstrating the
conservative barrier. **42 light tests pass** across the four test files.

0x00DA9EC0 uses zero-call-v1. 0x01104470 uses image-v1: zero-based
distributions correctly reject at the `div ecx` at 0x01104491, whose
divisor is the byte loaded from its first argument +0x25 at 0x01104489.
With the cleanup correction, all three image-backed trials return normally.
No input bytes or arithmetic were repaired during execution.

Evidence: `L28c/trace-da9ec0.log`, `trace-00da9ec0.json`,
`boot-route.jsonl`, and `boot-route-audit.json`. Recording used heavy.py;
no Rust replay. The data set stays **open, not frozen**, as explicitly
requested by head, so claude-2 can preview these additions immediately.

### L28c pointer-route batch and hand-off (2026-10-12)

**Final selected pair: 59,547 records / 177,235 cases.** The last batch
adds **0x00860A60, 0x00874F90, 0x00B54BE0**, three zero-call-v1 fixtures
each (**59,544 → 59,547 / 177,226 → 177,235**). All 59,544 previous
records/case multisets are unchanged (`L28c/pointer-audit.json`). Across
this entire title-route task, **13 functions / 39 cases** were added;
`L28c/title-cumulative-audit.json` proves **all 59,534 original records
and case multisets unchanged**, with no removals, promotions, affected
callers, unexpected changes, or stale mocked closed targets.

These three virtual wrappers use the same deferred argument shape, with
**no stack arguments belonging to the intervening virtual call**. At
0x00860A6B the pre-pushed value belongs to 0x0112F940, called at
0x00860A74 after pushing the virtual result. Both returns of 0x0112F940
(0x0112F98B / 0x0112F991) pop eight bytes. Thus the indirect call at
0x00860A6F pops zero. A separate synthetic regression covers this limit;
no further harness change was needed. **43 light tests pass**: 17 frame,
19 structured-input, two range-generator, five audit. No full Rust replay.

**Queue coverage:** all 32 one-hop entries after excluding the explicitly
forbidden 0x00520003/0x00520004 were attempted. All 29 eligible two-hop
entries were attempted; 0x00FB9A10 remains quarantined and was skipped.
Each unsuccessful entry was tried with three seeds of zero-call-v1,
zero-objects-v1, image-v1, and random, bounded to eight seconds per strategy.
No additional one-hop entry recorded; the three additions above are from
two hops. Together with the direct-title trials there are **77 tested
residuals**, with each attempt's exact reason in
`L28c/title-residual-evidence.json`. Missing prepared candidates:
**228 → 215** overall; the current legacy lift_novector list contains 175
entries. The 20 prepared lift rejections and three quarantines are unchanged.

**Remaining work / routing questions:**

- Constructor budgets were not guessed or increased. 0x00478600 uses a
  stack-slot countdown; 0x00490E80 has a flag-neutral store between its
  decrement and latch; 0x0049F990 copies a register initializer. The current
  constant-loop analysis refuses these forms. 0x00475400 / 0x004915A0
  also fail cost analysis at closed 0x00475180 (it saves EDI, uses it in
  the loop at 0x004751A0, and restores it at 0x004751EE); 0x00480240
  fails at closed 0x0047F660. 0x00DD5ED0's loop contains an indirect call
  at 0x00DD5F99, so a universal static callee cost is unavailable. Board
  #9540/#9542/#9552 requests coordination; **lift.py was not edited**.
- Some remaining cleanup failures require different evidence. For example,
  0x011590E0 calls 0x01156B00 at 0x01159141 with two pushed arguments;
  the latter's non-switch indirect tail leaves its cleanup unresolved by
  static_pop. Do not substitute an assumed pop or relax the own-return check.
- Head/claude-2 received each boot-route success in board #9548/#9550/#9551.
  The relevant implementation commits are **111bbe4e** and **92f14963**.
  Head should replay these fixtures in the integration train and continue
  the boot. All four priority addresses are now present. **native-L28c
  remains open, not frozen**, as head explicitly requested; no snapshots,
  shared-data writes, full replay, or pushes were made.

Recording jobs have finished and released their heavy slot. Private audit
manifests contain hashes only. The untouched comparison base remains
native-train3, and native-L17 remains frozen. Source/doc/test changes are
committed on task/L28b-crt as janne without trailers.

### L28c archive registration boot stop (2026-10-12)

**0x00419100 records 3/3; ready for immediate boot preview.** Private
native-L28c: **59,547 → 59,548 selected records**, **177,235 → 177,238
cases**. `L28c/archive-audit.json` proves all 59,547 old records/case
multisets unchanged; only 0x00419100 is added. Missing prepared candidates:
**215 → 214**. The data remains open, not frozen, as head requested.

The function loads ten descriptor pointers through 0x0153B6BC at
0x00419104. Each descriptor gives an entry pointer and count; the loop
at 0x00419113 stores that entry pointer at `0x018D9288 + id*4`. The
image contains **13,061 entries** across those tables. Its control flow
does not depend on the caller's inputs, but its **78,453 instructions**
exceed the ordinary 20,000 recorder budget.

The opt-in **image-bounded-v1** policy keeps image-v1's input distribution.
A separate conservative abstract interpreter starts with unknown GPRs
(except the recorder's actual ESP) and unknown flags; reads only original
image bytes and its own tracked stores; and must reach a balanced return
without overwriting the root return address. Unknown conditions/addresses,
calls, indirect flow, unsupported instructions, self-modifying writes,
excessive proof memory or time, and paths beyond the hard instruction
ceiling refuse the proof. Unknown stores invalidate image facts, rather
than reloading the old bytes. No input-dependent loop obtains a bound.
The ordinary limit remains when the proof fails. A successful long proof
adds its exact count to the ordinary allowance, capped at two million;
here the resulting limit is **98,453**. The proof never executes instead
of the original: Unicorn still runs and checks every access/instruction,
including all previous return, flag, page, and time guards. Each accepted
case records its strategy and the derived bound for repeatability.

**Verification:** six new synthetic tests check exact bounds, modified
image counts, unknown stores and inputs, unchanged default runaway
rejection, stack/code/ceiling/resource refusals, and an actual original-code
recording using the opt-in policy. **49 light tests pass** across image
budget (6), frame (17), structured-input (19), range (2) and audit (5).
The three real archive cases also pass an independent output-map check:
every one of the 13,061 registration slots contains its expected entry
pointer read from the exe. Evidence is private in `L28c/archive-check.log`,
`archive-record.log`, `archive-audit.json`, and the normal
`structured-image-bounded-v1.jsonl` trial log. Original recording used
heavy.py; no full Rust replay or data snapshots.

The unused identity-instruction helper draft was reverted after its tests
passed: it did not by itself establish the constructor bounds. The lifter
owner granted the budget area in board #9567, and requested merging
`task/L27b-integrate` (acdd978b) before changing lift.py. That merge and the
remaining requested constructor route follow this boot-stop commit.

### L28c constructor and title bounds (2026-10-12)

**Eleven more functions record 3/3 each:** private native-L28c moves
**59,548 → 59,559 records / 177,238 → 177,271 cases** after the archive
commit ab1c2dbe. All **59,548 previous records and case multisets are
unchanged**, with no removals, promotions, stale mock boundaries, or
unrelated changes (`L28c/guarded-cumulative-audit.json`). The first six
also have their own audit (`L28c/constructors-audit.json`). Including
the archive function, this continuation adds **12 functions / 36 cases**.
The data set remains **open, not frozen**, for the head's boot preview.

| Address | Guarded instruction bound | Route |
|---|---:|---|
| 0x00475400 | 91,980 | requested constructor |
| 0x0049F990 | 25,242 | requested constructor |
| 0x00480240 | 45,500 | requested constructor |
| 0x00490E80 | 32,140 | requested constructor |
| 0x004915A0 | 208,066 | requested constructor |
| 0x00480950 | 45,329 | requested constructor |
| 0x004755E0 | 51,233 | remaining title queue |
| 0x00478050 | 29,125 | remaining pointer queue |
| 0x0048AC80 | 31,691 | remaining title queue |
| 0x0048CC00 | 42,582 | remaining pointer queue |
| 0x004918D0 | 174,153 | remaining title queue |

The opt-in **guarded-bounded-v1** distribution uses zero-call-v1 inputs
and zero-return mocks. Its abstract trace starts with unknown GPRs except
the actual entry ESP. It follows direct closed calls, including exact
stack saves/restores, and the recorder's existing direct mock cleanup and
platform models. Known memory writes update abstract memory; unknown
values never become constants. An unknown-address output store may be
ignored only with a **runtime non-aliasing obligation**: it must not touch
any concrete byte read by the proof, any executed code byte, or the root
return address. Both Unicorn memory-write hooks and platform-model
`mwrite` enforce that guard; a violation rejects the original run. Bounds
include conservative mock/trampoline overhead and the ordinary 20,000
allowance, within the existing two-million ceiling. Unknown branches,
unknown indirect targets, unsupported effects, unbalanced returns, and
proof resource limits still refuse an enlarged budget. Original execution,
not the abstract interpreter, produces every accepted fixture.

The four requested constructors still without cases are evidenced:

- **0x00478600:** proof stops at unknown `call edx`, **0x00478854**.
- **0x00DD5ED0:** proof stops at unknown `call eax`, **0x00DD5F99**.
  The vtable store/reload through an unknown object pointer is not yet
  tracked symbolically; no callee contract is assumed.
- **0x0075BAD0:** after tracing its known table branch and bounded REP
  store, the condition at **0x0075BCA8** remains unknown.
- **0x00DB8B90:** its return at **0x00DB8BD8** is four bytes off because
  0x00DB8AC0's cleanup is unresolved. Its normal tail at **0x00DB8B65**
  reaches LeaveCriticalSection through IAT **0x0139D164** (pop four);
  its failure path at **0x00DB8B7F** calls 0x004080A0 before INT3.
  Noreturn evidence for that failure path belongs to claude-6's lane;
  board #9586 reports the dependency. No pop was guessed.

After the first six adoptions, a static census of all **70** remaining
eligible title/pointer candidates found precisely the five additional
bounds above. They all recorded; **65** remain without a fixture in that
queue. This is not a new successful strategy for unknown control flow.
Private evidence: `L28c/constructor-proof-evidence.json`,
`remaining_proofs.py`, `queue-proof-evidence.json`, the two recording logs,
and `structured-guarded-bounded-v1.jsonl`. Missing prepared candidates
fall **214 → 203**; rejected/quarantined entries were not admitted.

**Light checks:** 79 tests pass: image bounds 15, frame contracts 20,
function tails 13, structured inputs 19, record inputs 2, record audit 5,
existing lifter budgets 5. The budget tests cover actual Unicorn alias
rejection (including platform writes), closed callee save/restore, unknown
conditions, REP/count/DF constraints, known interior jumps, and unchanged
default runaway rejection. No lift.py change was needed.

One pre-existing expectation exposed by the owner merge is still awaiting
its owner's update: `test_lift_remaining.py` passes 12/13, but its old
negative `sub esp,8; indirect call; xor eax,eax; add esp,4; ret` now has
the valid split cleanup (callee four plus caller four). The earlier
cleanup scanner stopped at XOR and incorrectly popped eight. A new
owned frame regression proves pop zero/caller eight, pop four/caller four,
and rejection of the genuinely imbalanced caller twelve. Board #9583
requests permission to update the other owner's test; that file is
unchanged. This outstanding test is explicitly **not** reported green.

Merged the lifter owner's acdd978b and 9c412273 through **4104d570** and
**e76ab8bb**, preserving both refresh commands, input_layout/visited, and
the owner's terminal/x87 changes. Recording ran serially through heavy.py
as codex, at most six workers; free memory stayed above 16 GB during this
batch. No Rust replay (head's integration train), snapshots, shared-data
writes, native-L17/native-train3 changes, generated-code commits or pushes.

### L28c checked indirect contracts (source checked; recording queued)

The head's isolated cleanup-test correction **7034548d** is cherry-picked
as **5016233c**. The exact test is
`RemainingTests.test_interleaved_local_allocation_is_not_a_mock_argument`
in `test_lift_remaining.py` (originally introduced by L12, currently in
claude-6's file scope). It now asserts the balanced callee-four/caller-four
case and still rejects caller cleanup of twelve. All 13 tests in that
module pass; the previously documented failing expectation is resolved.

The next guarded-bound extension treats a root indirect call as a
**candidate** zero-return mock contract, only after a completed call has
established an argument boundary. The original run must report a matching
mock event and cleanup for **every visit** to that site. A real closed,
platform, or internal dispatch produces no matching mock event and rejects
the certificate. Unknown nested indirect calls, unknown argument space,
or differing cleanup at repeated visits still refuse the proof. This
establishes a candidate **100,122-instruction** bound for 0x00478600,
whose indirect call is at **0x00478854**; it is not yet an accepted fixture.

For **0x00DD5ED0**, writes at **0x00DD603E/0x00DD6041** put zero into
the same fields read at **0x00DD6063** and the next test. Tracking bytes
at offsets from an unchanged unknown base register proves these reads
without assigning a concrete object pointer. Different unknown bases may
alias and invalidate previous facts; concrete writes also invalidate them.
Changing the base register invalidates its identity; overlapping or opaque
writes replace the affected bytes with unknowns. This gives a candidate
**82,369-instruction** bound. Unknown output stores retain their existing
runtime non-aliasing guard against concrete proof reads/code/return bytes.

Negative tests exposed a Capstone metadata hazard: a MOVUPS memory
destination can be labelled read-only. The opaque SIMD handler now treats
every memory-destination MOVQ/MOVSS/MOVSD/MOVAPS/MOVUPS/MOVDQA/MOVDQU as
a write, independently of that metadata. An actual Unicorn test rejects
a SIMD store aliasing proof-protected code. **All 33 previously accepted
guarded cases are queued for re-recording and comparison in a separate
trial log; accepted records will not be replaced.** This strengthens a
proof assumption check; it does not change the executed instructions.

The analysis also supports known-carry ADC/SBB and bounded, sequential
REP MOVS (including overlap), using the original recorder's initial DF.
Unknown counts/direction/carry remain unknown. These advance diagnosis
at **0x012F2287** (0x00491340's stack helper) and **0x0075A262**
(0x00493AA0's copy), but do not yet produce fixtures for those callers.

The queued recording job includes 0x00478600, 0x00DD5ED0 and the short
0x00DC7D00 candidate, then the old guarded-case recheck. Follow-up trials
will re-lift only missing direct-route and one-hop candidates against the
merged owner source; accepted records and the three L46 x87 helpers are
excluded. **No new accepted data yet:** 59,559 records / 177,271 cases.
The hash-only baseline is `L28c/after-guarded-hashes.json`. Recording waits
in heavy.py behind the train/boot jobs; no slot bypass or full Rust replay.

**99 light tests pass**: image bounds 22, frame contracts 20, structured
inputs 19, function tails 13, remaining forms 13, record inputs 2, record
audit 5, existing lifter budgets 5. New negatives cover closed dispatch
instead of a mock, incorrect mock cleanup, missing argument boundaries,
alias/base/overlap invalidation and the SIMD-store hook. No known failing
test remains in these suites.

### L28c register-object batch (2026-10-12)

**0x00478600 now records 3/3.** Private native-L28c advances **59,559 →
59,560 selected records / 177,271 → 177,274 cases**. The append audit
`L28c/register-objects-audit.json` proves all **59,559 previous records
and case multisets unchanged**: no promotions, caller changes, removals,
stale mock boundaries or unrelated changes. Missing prepared candidates
fall **203 → 202**. The data remains open for the head's preview/adoption.

The constructor takes its object in **ESI**, copied to ECX at
**0x00478606**, and writes the vtable through ESI at **0x0047860D**.
Zero-call inputs put that object at address zero. The mock at
**0x00478837** also returns EAX zero; the vtable loads at
**0x00478842/0x00478844** therefore read the constructor's own object,
and the indirect call at **0x00478854** takes the wrong input-induced
route. This is a fixture alias, not evidence that the game takes that path.

The new opt-in **guarded-objects-v1** policy uses the existing
zero-objects-v1 register-pointer palette for all entry GPRs except ESP.
It keeps zero-call-v1's zero-filled non-image pages, aligned caller stack,
and zero GPR/SSE mock returns. No object layout or game address is special
cased, and no memory is repaired during execution. The abstract bound
still starts with unknown entry GPRs; its alias/call obligations must pass
in the original run. Each accepted case uses the **100,122-instruction**
bound / **120,122** total limit, returns its input ESI in EAX, and records
the indirect mock's **12-byte** cleanup. The earlier guarded-bounded-v1
distribution is unchanged. Evidence: `structured-guarded-objects-v1.jsonl`
and `L28c/this-record.log` (the private filename predates the policy rename).

Two conservative proof refinements accompany it. An indirect target that
is already known from registers or tracked object bytes and belongs to
the closed/model boundary follows that real callee; it is not treated as
a proposed mock. A concrete load returning a wholly unknown value creates
no inferred constant, so it does not add a constant-read alias obligation.
Known loads, code, and return bytes remain protected. Tests cover known
register/memory indirect targets and an actually closed dispatch where
the proposed target was unknown (which still rejects).

**Previous guarded fixtures were rechecked after the SIMD-store fix:**
all **33/33** original cases succeeded, and every case is identical to its
accepted version, including proof metadata (`L28c/guard-recheck-comparison.json`).
No accepted record was replaced. All recording runs were serial heavy.py
jobs with at most six workers; memory stayed above the six-GB stop threshold.

**Queue coverage:** re-lifted only missing candidates against the merged
lifter, then retried the **nine direct-route** and **30 remaining one-hop**
entries under guarded-bounded-v1, zero-call-v1 and image-v1. None added a
fixture. Guarded-objects-v1 then tried 0x00478600 first, the other eight
direct-route entries, and those 30 one-hop entries; only 0x00478600 recorded.
Each strategy requested three seeds, normally with 12 seconds per function;
the focused constructor trials had 90 seconds. Exact attempts remain in
the strategy logs and `L28c/remaining-record.log`. No quarantines were
overridden. There are now eight direct-route and 30 one-hop residuals.

**Remaining constructors / ownership:** 0x00DD5ED0's proposed mock at
**0x00DD5F99** is not the actual dispatch: 128 visits produce zero mock
events, so the certificate correctly rejects. Tracking its vtable across
the intervening closed call needs further evidence; a bound alone is not
a fixture. 0x0075BAD0 retains the unknown condition at **0x0075BCA8**.
Per head board **#9617**, **0x00DB8B90 / 0x004080A0** belong to claude-6,
as do **0x01300660 / 0x01300675 / 0x013006A5**; no further trials or edits
will target them. The earlier failed DB8B90 trials were already complete
when that message was read and changed no accepted data. The short 0x00DC7D00 trial still
exceeds the 256-page guard; that guard was not widened.

**102 light tests pass**: image bounds 25, frame contracts 20, structured
inputs 19, tails 13, remaining forms 13, record inputs 2, audit 5, lifter
budgets 5. No full Rust replay; that remains with the integration train.
Source checkpoint **925ecca8** and the head-test cherry-pick **5016233c**
precede this result; the register-object implementation/result is
**d810fb19**. After the recording workers exited, merged main **b7c4ba86**
(including requested **863f6817**) as **d386e757**, without conflicts.
The owner's terminal/padding rules and main's opt-in instruction-step
generation remain intact. No generated code was authored by this batch;
the merge retains main's already committed generated files.
The same **102 light tests pass after the merge**. The worktree is ready
for review; recording jobs have exited and accepted data remains open.

### L28c train-4 residual census and fixtures (2026-10-12)

The read-only inventory has **211 non-verified non-thunks**: 91 documented,
53 untouched, 57 translated and ten platform. Cross-checking the private
accepted set, prepared candidates, quarantine and `verification_gaps.json`
gives the following disjoint classes (before this batch):

| Class | Count | Treatment |
|---|---:|---|
| L48 verification gaps | 44 | Reserved for claude-6 |
| Boot stop 12, 0x00DB8B90 | 1 | Reserved for claude-6 |
| Already recorded privately, 0x00478600 | 1 | Keep unchanged |
| Prior invalid-return quarantine | 3 | Keep excluded: 0x00C1D790, 0x00D9EF20, 0x00FB9A10 |
| Cached lift rejections | 4 | Lifter lane: 0x00736370 (`bound`), 0x00F83C00 / 0x013156B0 (x87 overflow), 0x012F1C9D (x87 return) |
| Not in prepared candidates | 5 | 0x0047B950, 0x0047B9C0, 0x00736730, 0x012EDC70, 0x012FE5C9; sent to the lifter owner for classification |
| Recorder failures | 153 | Seven eligible direct-route entries, 21 inventoried one-hop entries, 125 other entries |

The recorder group comprises **82 documented, 48 untouched, 13 translated
and ten platform** entries. Its address-level evidence is private
`L28c/train4-inventory-census.json`; this is a census against train 4's
inventory, not a claim that every supplementary one-hop address is a real
function. The prior one-hop queue has additional entries absent from this
non-thunk inventory census.

**Direct route:** re-lifted 0x00491340, 0x00493AA0, 0x00494D80,
0x004A06B0, 0x0075BAD0, 0x0081C330 and 0x00DD5ED0 with the merged
train-4 lifter. All lift; none has a static terminal site. Retried each
with terminal acceptance enabled under guarded-objects-v1,
guarded-bounded-v1, zero-call-v1, image-v1 and small-v1 (three seeds,
24 seconds per function). None recorded. Ordinary instruction, page,
return and indirect-mock guards remain unchanged. Exact failure events
are in the five `structured-*-terminal.jsonl` logs and
`L28c/train4-retry.log`. The eighth residual, 0x00DB8B90, was not retried.

Baseline hash manifest: `L28c/train4-retry-before-hashes.json`, **59,560
records / 177,274 cases**. Comparison base for head adoption is now
**native-train3-orig**, read-only; native-L17 remains frozen.
The unchanged-source light suite passes **102 tests**. No full Rust replay.

**First inventory batch ready:** the 125 other recorder candidates were
re-lifted without a refusal, then tried with zero-call-v1, image-v1,
small-v1 and guarded-objects-v1 (three seeds, 12 seconds per function,
terminal acceptance enabled). **Thirteen functions recorded three cases
each under zero-call-v1; every case returns normally**, rather than using
a terminal shortcut:
0x0085B520, 0x0085E220, 0x0086D7E0, 0x008757B0, 0x00878250,
0x00AFD8D0, 0x00CE8C50, 0x00CE8CC0, 0x00EAFD90, 0x00F20620,
0x00F77CC0, 0x011D2CE0 and 0x012E1DA1.
They use existing input/recorder rules; this result does not attribute
normal returns to terminal acceptance. Private accepted data is now
**59,573 records / 177,313 cases**, +13/+39. The hash audit
`L28c/train4-census-audit.json` proves **all 59,560 old records and their
case multisets unchanged**, with no promotions, removals or stale mocks.

**Bounded caller arguments:** source **22549c3b** adds opt-in
`guarded-stack-v1`. It has guarded-objects-v1's distribution but allows
the proof to read the actual initial **[ESP+4, ESP+132)** bytes. Entry
GPRs, flags, the saved return and callee locals remain unknown. Known
stores override these facts; unknown writes must satisfy the same runtime
non-aliasing guard. This admits the finite branch at **0x0075BCA8** in
0x0075BAD0 without assuming its caller's flags or registers. Original
recording now passes **3/3**, pending adoption with the rest of this batch.
Tests cover the window boundaries, exact counts, preceding writes, old
policy refusal and a runtime alias write. **105 light tests pass**.
The owner's unchanged `static_pop` padding fix **a1f09162** was integrated
as **13446abb** before these new-policy trials. No edits to his slot-sweep
layout handling, decoder, frame-tail, EH4 or step-emission code.

**Caller-route checkpoint:** 0x0075BAD0 is now adopted. All three original
cases return normally and carry a **46,591-instruction** certificate with
**66,591** total allowance. `L28c/train4-stack-audit.json` proves the
accepted pair grew to **59,574 / 177,316** (+14/+42 from this batch's
baseline), with every old record and case unchanged. The five initially
unprepared inventory entries were subsequently checked against the
current lifter: all lift unchanged, and all record three normal-return
cases under zero-call-v1. They are queued for the final append, rather
than being left incorrectly classified as lift rejections.

**Residual inventory addresses by observed guard.** These are representative
blocking classes across the attempted distributions, not claims of a unique
root cause or proof that a function cannot record. A function can trip several
guards; `L28c/train4-recorder-evidence.json` retains every observed reason.
The 44 verification gaps, owner boot stop, four cached lift rejections and
three quarantined entries above are excluded.

| Observed class | Count | Addresses |
|---|---:|---|
| Instruction limit | 45 | 0x00410000, 0x00416740, 0x0046A150, 0x0046A2D0, 0x00475DD0, 0x00476D60, 0x00480BA0, 0x0048F330, 0x00491340, 0x00493AA0, 0x00494D80, 0x00495150, 0x00495520, 0x004A06B0, 0x004A0C00, 0x004A0E20, 0x004EE3E0, 0x004EE720, 0x004F5200, 0x004F53B0, 0x0050A110, 0x00524A10, 0x00524BE0, 0x00528550, 0x005293E0, 0x006076F0, 0x00608AE0, 0x00618880, 0x00689960, 0x007F2020, 0x00811A90, 0x00815C60, 0x008282E0, 0x00B7FF30, 0x00C9BAA0, 0x00C9E0F0, 0x00DA46C0, 0x00DBA780, 0x00DC8270, 0x00E0D750, 0x00F7DE40, 0x01146420, 0x012A5B30, 0x012F9D82, 0x01314A90 |
| Outside-image execution | 67 | 0x004644D0, 0x00561239, 0x005652E0, 0x0056532A, 0x0057D24D, 0x005F3450, 0x00606090, 0x00606130, 0x00606670, 0x006066E0, 0x006197E0, 0x0061BC10, 0x0062E3A0, 0x006335A0, 0x00633A70, 0x00633B70, 0x00646A30, 0x0064DD20, 0x00663040, 0x00677930, 0x00685030, 0x0068A9F0, 0x00691AE0, 0x00691E50, 0x00692A40, 0x006A1770, 0x006EDFE0, 0x006F1590, 0x006F2350, 0x006F3242, 0x006F8730, 0x006F8C00, 0x00705E00, 0x0070B210, 0x007126C0, 0x00712AC0, 0x00786D74, 0x007BEA60, 0x0083FFF0, 0x00947493, 0x009D0E70, 0x00A195D0, 0x00B5A6BF, 0x00B75EC0, 0x00B952C0, 0x00B955C0, 0x00B97220, 0x00BA31D0, 0x00BA5540, 0x00BD45C0, 0x00CED240, 0x00CED280, 0x00CEEBC0, 0x00D97031, 0x00E0FB9C, 0x011251F0, 0x01137DF3, 0x011590E0, 0x011591C0, 0x012EFA1E, 0x012F693F, 0x012F6948, 0x012F6A6F, 0x012F6A78, 0x012FF0F0, 0x01300DFF, 0x01329888 |
| POPF(D) state guard | 4 | 0x0048AB50, 0x00685B40, 0x0081C330, 0x00BB922B |
| Undefined integer flags | 4 | 0x005524B0, 0x00604A80, 0x00BA4160, 0x00C9B350 |
| Overwritten return address | 4 | 0x005EC920, 0x006692F0, 0x00669F40, 0x0071C0F0 |
| Page limit | 5 | 0x00633EF0, 0x0066B9E0, 0x006E0EB0, 0x0070F3C0, 0x00DC7D00 |
| x87 control/stack state | 8 | 0x009897C0, 0x01300600, 0x01300617, 0x013006FE, 0x0130070B, 0x01300749, 0x0130474C, 0x01304753 |
| CPU exception | 1 | 0x00D29AF0 |
| Unmatched proposed indirect mock | 1 | 0x00DD5ED0 |


**Final batch ready:** all recording workers have exited. Adopted the five
prepared-entry successes (0x0047B950, 0x0047B9C0, 0x00736730, 0x012EDC70,
0x012FE5C9). Final private totals are **59,579 records / 177,331 cases**:
**+19 functions / +57 cases**, all normal returns. The audit
`L28c/train4-final-audit.json` is green: **all 59,560 pre-batch records and
177,274 cases unchanged**, zero promotions, removals, unexpected edits or
stale mocks. No accepted function was re-recorded or replaced.

**Coverage and remaining work:** the 153 recorder candidates plus five
newly prepared inventory entries yielded 19 functions; **139** still have
no fixture, grouped above. The direct-route queue goes **8 -> 7**:
0x00491340, 0x00493AA0, 0x00494D80, 0x004A06B0, 0x0081C330,
0x00DD5ED0, plus owner-reserved 0x00DB8B90. All **30 one-hop candidates**
re-lift and were tried under zero-call-v1, image-v1, small-v1,
guarded-objects-v1 and guarded-stack-v1 with terminal acceptance; none
recorded. 0x00520003/0x00520004 were never tried. The nine one-hop entries
outside this non-thunk census remain candidate entries, not newly asserted
valid function boundaries. The guarded-stack pass also tried all other
remaining recorder candidates; 0x0075BAD0 was its only addition. After
integrating the owner cleanup rule, the other six eligible direct-route
functions were retried again with zero-call-v1 and guarded-stack-v1.

**Open evidence:** 0x00DD5ED0 still reaches a real dispatch where the
certificate proposed a mock (0x00DD5F99); it needs exact callee/object
facts, not a relaxed mock guard. The other direct failures still require
caller-state or bounded-flow evidence. A guard firing is recorded as such,
not treated as proof of a bug in the executable. Claude-6's boot functions,
three x87 helpers, 44 verification gaps and the four cached lift rejections
remain outside this batch. The prior cleanup-test expectation is resolved;
there is no outstanding failing Python test.

**Verification and handoff:** **105 light tests pass** (27 image-bound,
21 frame-contract, 19 structured-input, 13 tail, 13 remaining-form, two
record-input, five audit and five budget tests). Full Rust replay was not
run; the head's integration train must verify these fixtures. Recording
used heavy.py as codex, serially, with at most six workers. Free memory
never fell below six GB; the monitors watched only this agent's
own recording trees and never needed to stop them. No snapshots, shared-data writes, generated-code
commits or pushes. Comparison base remains **native-train3-orig**; the
append-only **native-L28c** set is ready for the head to snapshot and
remains open until instructed otherwise. Source commits: **13446abb**
(owner a1f09162) and **22549c3b**; first audited checkpoint: **b7d3bbc3**.
