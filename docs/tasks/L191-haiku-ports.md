# L191: per-model draw-prep and probe ports (haiku trial branch)

Ports: `crates/mtcore/src/semantic_l191.rs`, registered in `crates/ddports/src/lib.rs`.
Recorders: `research/native/record_l191_*.py` (Unicorn, via `research/native/s10_oracle.py`).
Vectors (not committed): `E:/ddda-re/notes/native-L191/`.
Tests: `crates/mtcore/tests/semantic_l191.rs`. Paired benchmarks:
`crates/mtcore/tests/l191_bench.rs` (`cargo test --release -p mtcore --test l191_bench -- --ignored --nocapture`).
Analysis tools: `research/native/l191_callee_inputs.py` (registers a callee reads before writing),
`l191_flags_scan.py` (flags read after calls, whole lifted corpus), `l191_entry_flags.py` (flags read on entry).

| Address | Function | Role | What it does | Speedup | Cases / mutations | Commits |
|---|---|---|---|---|---|---|
| 0x00886880 | `uObjModel::vf11` | per-model draw prep | Copies the model level to +0x158, sets bit 30 of +0x108 when the level is above a constant; may call the state handler 0x007A6640, then tail-calls 0x00FFE5C0. | 1.96x | 3000 / 3 of 3 | 1f92321b, c6c8df77, 7a5ae8e3 |
| 0x00885DC0 | `uObjModel::vf08` | per-model draw prep | Clears the draw counter; first time calls vtable 0x80; when the pass word is 1 calls vtable 0x168, 0x16C, 0x170 and tail-calls 0x174. | 1.17x | 3000 / 3 of 3 | 6e1bbb3d, c6c8df77, 7a5ae8e3 |
| 0x0084C720 | `uCharacterBase::vf09` | per-character draw prep | Sets bit 0x200000 from a mode byte, calls 0x008586E0 and the model prep 0x00886530; with a block it calls vtable 0x94 and the stdcall 0x00CA5930. | 1.20x | 3000 / 3 of 3 | c5525508, c6c8df77 |
| 0x00886530 | `uObjModel::vf09` | per-model draw prep | Flag-driven sub-object and child-method calls; bounds test on the global reference (may call 0x00821750); angle block with the native CRT atan2; releases the table at +0x23B0. | 1.39x | 3000 (a third with exact bound ties) / 3 of 3 | 2126666c, c6c8df77 |
| 0x01165B40 | probe search step | probe tree step | Picks an 80-byte record of the table; may call a vtable method, a callback and table lookups; returns the last callee's EAX. | 1.82x | 3000 / 3 of 3 | aaa9a018, c6c8df77 |
| 0x0116B670 | owner visit | probe tree (visit of one owner) | Two calls of 0x011787C0 on a local vector, or a scan of records (vector mask test, 0x01107E40/E30, 0x01169A50); ends with the record's indirect method. | 1.14x | 3000 / 3 of 3 | fe8f02ca |

Register rules checked per recorded call: EBX, EBP, ESI, EDI, ESP at every call; the
registers a callee reads before writing (from its lifted code, see below) at every
call; ECX/EDX that still hold a stub's recorded scribble are skipped (undefined for
any port). At return: EBX, EBP, ESI, EDI, ESP, and EAX for value-returning functions.

## Audit results (after the crash report on 0x885DC0)

### Registers (commit c6c8df77)
The lifted caller had set EBP (frame base), EDI, ESI, EBX or EAX before a guest call and
the ports had not. Fixed per port: vf11 (EAX, ESI before 0x7A6640); vf08 (EAX, EDX, ESI);
char_vf09 (EBP, ESI); vf09 (EBP, EDI, EBX, ESI, EAX/EDX for vtable calls); probe (EBP,
EBX, EDI, ESI); owner (EBP, EDI, EBX, ESI, EDX).

### Callee inputs (commit 7a5ae8e3)
Every test callee's register inputs are derived from its lifted code
(`l191_callee_inputs.py`: every path from its entry, registers read before written).
Among them, EAX/ECX/EDX are inputs only where the analysis finds a read; the test
compares those at every call. Path-insensitive analysis can over-report (a read on
a path the caller never takes); the scribble rule covers the rest.

### EFLAGS
- Callers (`l191_flags_scan.py`, all 188,170 call sites of the lifted corpus): the
  first flag operation after the return reads flags in one place only (a `call_lifted`
  to 0x013006A5 whose flags are its own, not a port's). So no caller of a ported
  function reads the flags it leaves.
- Callees on entry (`l191_entry_flags.py`): none of the test callees, and none of the
  vtable methods that vf08 dispatches to, reads flags before writing them (within the
  first blocks; six callees are unresolved beyond that depth).
- Tail calls: a tail-called target sees the flags of the last compare. vf08 and vf11
  now set those (see below); the other ports make only non-tail calls.

### The vf08 crash (0x885DC0)
- vf08 is slot 0 of about 90 class vtables in the image; its dispatched methods are
  the thunks at 0x4C6B90 (tail call through vtable slot 0x190) and 0xBCC920 (+0x2544),
  plus about 20 methods. The 0x80 method is the first call of every model's first
  frame, i.e. startup.
- The lifted body sets the flags `cmp [esi+0x1F9C],0` (ZF=1 on the 0x80 path) before
  that call, and `cmp [esi+0x1F98],1` before the 0x168 call. The port left the flags
  of the caller in place. A method reached through the thunk can read those flags.
  Fixed in 7a5ae8e3 (the flags are set as the lifted compares do; the vf08 benchmark
  stays at 1.17x with the direct return-address write).
- Not verified in-game: the fix is the only flag difference between port and lifted
  on the first call, and the recordings cannot see flags.
- Second cause (found after 7a5ae8e3): the pass path tail-calls slot 0x174 after
  `mov ecx, esi` (0x00885E1D). The port did not set ECX, so the tail-called method
  received the ECX the previous vtable method had left (the recorder's scribble in
  the recording). The crash frames fit this: 0x00886400 (vf10) calls its vtable slot
  0x20, which is vf08; vf08 tail-calls 0x00C51D90 with ECX = 0xFFFF, and 0x00C51D90
  reads `[ecx+0x2627]` = 0x12626. Fixed by setting `c.ecx = this` before the tail call.
  The harness missed it because its replay never changed the port's ECX/EDX after a
  call; it now sets them from the recorded return state, and the test fails without
  the fix (port ECX 404594402 at the tail, recorded 0x20000000). The other five L191
  tests pass with the stronger replay. Not verified in-game: the game runner must
  confirm that the crash is gone with 0x885DC0 registered.

### Gaps that remain
- EFLAGS at non-tail calls in vf09, char, probe and owner are not reproduced (no
  callee in the corpus reads them, see above).
- ECX/EDX are compared only where a callee reads them.
- Six callees' entry flags are unresolved beyond the analysis depth.

## Lessons (implicit registers and flags)

1. A ported function must leave every register the lifted caller will read in the
   state the lifted code would leave it: EBP (frame base after `push ebp; mov ebp, esp`),
   ESI/EDI/EBX when the body sets them, and the register a vtable call takes its
   target from (EAX = vtable, EDX = target). Lifted code often sets a register just
   before a call and the callee reads it; a port that does not set it passes the old
   value.
2. The test must check the callee's inputs, not only the declared ones. Derive them
   from the callee's lifted code (`l191_callee_inputs.py`) rather than from memory.
3. Callee-saved registers (EBX, EBP, ESI, EDI) survive a call in the recording, so they
   must match at every call. ECX and EDX after a stub return hold a recorder scribble
   that is undefined for the port: record it (`scribbled`) and skip it. The replay must
   set ECX and EDX from the recorded return state, or a port that leaves its own stale
   value passes the check by accident (vf08's tail call).
4. Flags: a tail call hands the flags on to the callee. Reproduce the last compare
   before any tail call and before the first call of a path whose callee might read
   flags. `c.fl.alu_sub(a, b, width)` leaves the same flags as the lifted compare.
5. A thunk (`mov eax,[ecx]; mov eax,[eax+K]; jmp eax`) passes ECX and the flags through
   unchanged; the real target is a vtable entry. Analyse the target, not only the thunk.
6. A native port that replaces a call (here the CRT atan2) changes EAX/ECX/EDX/flags
   afterwards; the lifted code's next instructions must not read them before writing.
   Check that, and record the clobber (`after_atan2` hook in `record_l191_vf09.py`).
7. Test on the registers the callee reads, not only on memory: a port can write the
   right memory and still crash the game through a register.

## Not ported: 0x00DDBDC0 (checked, no port)

- 0x00DDBDC0 is a dispatcher: for each entry of the table at `this+0x3B340` (stride 0x40) it
  switches on the entry's type and calls one of five guest handlers (0x00DDB340,
  0x00DD3EF0, 0x00DD4040, 0x00DD4180, or the already-ported 0x00DD42C0), then calls 0x00DCC300
  and 0x00DBE560 (the join/wait machinery) once. Nearly all of its time is in those calls.
- Its own lifted self time is about one of the 19 profile samples attributed to it
  (`prof4`, "nearest lifted function (self + non-lifted callees)"). The rest is the ported
  0x00DD42C0 (`mtcore::semantic::dispatch_event`, 18 self samples under the `call_ext`
  dispatch closure). A port of 0x00DDBDC0 would be mostly guest calls: estimated < 1.1x.
- The real self-time sink is 0x00DD42C0 itself: a linear scan of all listeners of
  `this+0x4CD48` (stride 0x6E0) for each event and each parent level, reading about six fields
  per listener through the page table. Cheaper options: read the first 0xD0 bytes of each
  listener once (one slice instead of six page lookups; done in 88e3c98c, a 1.01x to 1.08x
  gain in the bench, below the gate); keep an owner/channel index maintained by the
  add/remove paths (needs those paths ported too).

## Strict audit of the playtest_fast2 ports (2026-10-14)

Scope: every port the playtest enables (`playtest_fast2.bat`: `DDRUNTIME_SEMANTIC`,
`SEMANTIC_L184`, `GEOMETRY_PORT`, `SHADOW_PASS`, `SHADOW_TRAVERSAL`, `JOB_SHADOW_SAVE`,
`JOB_HASH`, `MATERIAL`, `CULL`, `LIGHTS`) plus the default-on ABI tables (mtcore abi and
hot, mtmath, mtframe, mtdraw abi). Ports that the playtest skips are marked.

Checks at each recorded guest call: the callee's stack and ECX as before, and the
callee-saved registers the callee reads before writing them (EBX, EBP, ESI, EDI), from its
lifted code (`research/native/l191_callee_reads_table.py` writes
`crates/mtcore/tests/support/callee_reads.rs`). The recorded return state is replayed,
including ECX and EDX. The L191 harness checks the four callee-saved registers at every
call. A blanket check of all four at every call fails on values no callee reads (lifted
code uses EBX and ESI as scratch), so the other harnesses check the registers the callee
reads. EAX and the full register set are checked at return.

| Port (address, gate) | Harness | Checked | Result |
|---|---|---|---|
| 0x00885DC0 vf08 (L184; skipped) | semantic_l191 | all four at every call, ECX/EDX replayed, EAX, memory | fixed: tail call ECX (249049f9); pass |
| 0x00886880 vf11, 0x0084C720 char vf09, 0x00886530 vf09, 0x01165B40 probe, 0x0116B670 owner (L184) | semantic_l191 | as above | pass |
| 0x00DD42C0 dispatch_event (SEMANTIC) | semantic | callee reads, ECX/EDX replayed; paged equivalence, 3 mutations | fixed: ESI at owner call (88e3c98c); pass |
| 0x01054700 update_joints (SEMANTIC) | semantic (recorded); semantic_c2 (stubbed callees, not recorded) | callee reads (recorded only) | OPEN: routine 0x0DEAD100 (no lifted body) in case 480, not checked |
| 0x00E0EC50, 0x00E10FC0, 0x00FEDD60, 0x005B31A0, 0x00CD8760 (SEMANTIC) | semantic | callee reads | pass |
| 0x00FF1920, 0x00FF14D0, 0x00FEFB90 (SEMANTIC) | semantic_draw_a | callee reads, ECX/EDX replayed | pass; 0x01200000 BEGIN/END markers read nothing |
| 0x00EA23F0, 0x00FED190, 0x00E0F3C0, 0x01008200, 0x00FEE320, 0x00FFC350, 0x01145BF0 (SEMANTIC) | semantic_draw_c | callee reads | pass |
| 0x00E0E5E0, 0x010655A0, 0x01066C60, 0x00FFC9F0, 0x01140180 (SEMANTIC) | semantic_draw_b | ECX and ESP only; the recording stores no callee-saved registers | not checked: needs re-recording with registers |
| 0x010658B0 apply_pass_state (SEMANTIC) | semantic_c2 | 0x01062F20 is a stub (not recorded): its registers are not checked | pass (ABI level) |
| 0x010A35D0 query_bounds_tree (SEMANTIC) | semantic_jobs | callee reads | pass |
| 0x00C24640 draw_instanced_mesh (SEMANTIC) | no recorded test in this audit | none | not checked |
| 0x00FED480 save_state (JOB_SHADOW_SAVE) | job_shadow | full registers at each call | pass |
| 0x00E0E5E0 dependency_hash (JOB_HASH) | job_hash | final registers | pass |
| 0x00DB63E0 traverse (SHADOW_TRAVERSAL) | shadow_traversal | full registers at each call | pass |
| 0x00EFD240 setup (SHADOW_PASS) | shadow_pass | full registers at each call | pass |
| S1 to S10 groups: 0x005D1AF0 ... 0x00FE4780 (L184) | semantic_s1 to s10 | callee reads, ECX/EDX replayed | pass |
| 0x00860AC0 constraint_edit_apply, 0x00865990 euler_twist (L184; skipped) | semantic_s5 | callee reads | fails at HEAD too; ignored with reason |
| 0x01066B10 bind_parameter, 0x01067790 colour, 0x01062F20 update_material (MATERIAL) | mtdraw material | callee reads, ECX/ESP | pass |
| 0x010655A0 apply (mtdraw copy, unregistered) | mtdraw material | callee reads | fixed: EDI = this before the finalizer call (76f007b6); pass |
| 0x00FF0780, 0x00FF0AD0, 0x00FF0D20, 0x00FF0E60 cull (CULL) | mtdraw material | callee reads | pass |
| 0x0049B1A0 scene_lights (LIGHTS) | mtdraw material | callee reads; 0x0DEA0000 recorder stubs read nothing | pass |
| 0x00FE9580 build_geometry (GEOMETRY_PORT) | ddruntime procedural_geometry | all GPRs, XMM, flags, stack at each call; full callee state replayed | pass (original vectors present) |
| default-on ABI tables (mtcore abi, hot; mtmath; mtframe; mtdraw abi) | differential (each crate) | all eight registers at each call and at return, memory | pass |

Bugs found and fixed by this audit: 0x00885DC0 tail-call ECX (249049f9), 0x00DD42C0 ESI on
an empty listener list (88e3c98c), 0x00D7CF70 registers at the tetrahedron call
(5d626f8a), and the unregistered 0x010655A0 copy's EDI (76f007b6).

Open: 0x01054700 case 480 (routine 0x0DEAD100), 0x02000000 as the recorders' CALLBACK
object (not game code, not checked), and the EFLAGS at calls outside the L191 ports
(only the geometry test compares flags at calls; the L191 flag audit above covers its ports).
