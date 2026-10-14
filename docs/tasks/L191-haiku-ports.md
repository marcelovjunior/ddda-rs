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
   that is undefined for the port: record it (`scribbled`) and skip it.
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
