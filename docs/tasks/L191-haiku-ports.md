# L191: per-model draw-prep and probe ports (haiku trial branch)

Ports in `crates/mtcore/src/semantic_l191.rs`, registered in `crates/ddports/src/lib.rs`.
Vectors (Unicorn recordings, not committed) are in `E:/ddda-re/notes/native-L191/`,
recorders in `research/native/record_l191_*.py`, tests in
`crates/mtcore/tests/semantic_l191.rs`, paired benchmarks in
`crates/mtcore/tests/l191_bench.rs` (`cargo test --release -p mtcore --test l191_bench -- --ignored --nocapture`).

Register rules checked per recorded call: EBX, EBP, ESI, EDI and ESP at every call,
the registers a callee declares it reads (EAX/ECX/EDX where declared), and at return
EBX/EBP/ESI/EDI/ESP (plus EAX for functions that return a value). EAX/ECX/EDX after a
call are caller-saved and are not compared unless declared.

| Address | Function | Role | What it does | Speedup | Cases / mutations | Commit |
|---|---|---|---|---|---|---|
| 0x00886880 | `uObjModel::vf11` | per-model draw prep | Copies the model level to +0x158 and sets bit 30 of +0x108 when the level is above a constant; may run the state handler 0x007A6640, then tail-calls 0x00FFE5C0. | 3.3x (6.5x before the register fix) | 3000 / 3 of 3 | 1f92321b, register fix below |
| 0x00885DC0 | `uObjModel::vf08` | per-model draw prep | Clears the draw counter; the first time calls vtable 0x80; when the pass word is 1 calls vtable 0x168, 0x16C, 0x170 and tail-calls 0x174. | 1.11x | 3000 / 3 of 3 | 6e1bbb3d, register fix below |
| 0x0084C720 | `uCharacterBase::vf09` | per-character draw prep | Sets bit 0x200000 from a mode byte, calls 0x008586E0 and the model prep 0x00886530; with a block it calls a vtable slot 0x94 and the stdcall 0x00CA5930. | 1.41x | 3000 / 3 of 3 | c5525508, register fix below |
| 0x00886530 | `uObjModel::vf09` | per-model draw prep | Flag-driven sub-object and child method calls; bounds test on the global reference (may call 0x00821750); angle block with the native CRT atan2; releases the table at +0x23B0. | 1.33x | 3000 (a third with exact bound ties) / 3 of 3 | 2126666c, register fix below |
| 0x01165B40 | probe search step | probe tree step (`Global::FUN_01165b40`) | Picks an 80-byte record of the table, may ask a vtable method, a callback and table lookups; returns the last callee's EAX. | 1.68x | 3000 / 3 of 3 | aaa9a018, register fix below |

## Register audit (after the crash report)

The first versions checked only the declared inputs of each callee. The lifted caller
had set EBP (frame base), EDI, ESI, EBX or EAX before the call, and the ports did not, so
callees read different implicit inputs. Fixed per port:

- vf11: EAX = the argument and ESI = the state before 0x007A6640.
- vf08: EAX = the vtable and EDX = the method target before each call; EAX = 0 and ESI = this on entry.
- char_vf09: EBP = the frame base and ESI = this on entry.
- vf09: EBP = the frame base, EDI = this on entry; EBX = the pass word (always loaded); ESI = the sub-object, the loop counter and the reference object at the points the lifted body sets them; EAX/EDX for vtable calls.
- probe: EBP = the owner, EBX = this (the slot base inside the table branch), EDI = the index (the callback's result after it), ESI = the state.

The caller-saved registers are not modelled after calls; a port cannot reproduce the
recorder's scribbles there, and a callee that depended on them would be reading
undefined values.

Not verified: EFLAGS at return (the recorder does not record flags). A caller that
branches on flags left by a ported callee would not be caught by these tests.
