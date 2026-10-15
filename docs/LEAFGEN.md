# leafgen: generated ports of straight-line leaf functions

`research/native/leafgen.py` turns a function of the exe that has no branches, calls or x87 and uses a small instruction subset
(moves, `lea`, integer ALU, scalar and 128-bit SSE moves, `mulss`/`addss`/`subss`) into a readable Rust port. It runs the code
symbolically and emits the loads and stores in program order, the final value of each register the function changes, and `ESP` after the
return. It does not decide anything alone: every port is replayed against the original's behaviour recorded in Unicorn
(`research/native/record_generated.py`, test `crates/mtcore/tests/semantic_generated.rs`), and a function the executor cannot model is
skipped. The result is `crates/mtcore/src/semantic_generated.rs` (805 functions, generated, do not edit) with a `PORTS` table that
`ddports` registers when **`DDRUNTIME_SEMANTIC_GENERATED=1`** is set (a hand-written port at the same address wins; `DDRUNTIME_SEMANTIC_ONLY/SKIP`
select single addresses as for the other groups).

## Regenerating

```sh
export DDDA_EXE=reference/DDDA.exe                      # the Steam exe's code is not encrypted
python3 research/native/leafgen.py --discover 12 > list.txt            # candidates of >= 12 instructions, not yet ported
python3 research/native/leafgen.py list.txt OUT.rs specs.json OUT_test.rs
python3 research/native/record_generated.py specs.json VECTOR_DIR 400   # prints the names that recorded; ~4 s a function
# keep the recorded names, regenerate from exactly those addresses, copy OUT.rs / OUT_test.rs into crates/mtcore, then
GEN_VECTORS_DIR=VECTOR_DIR DDNATIVE_REQUIRE_VECTORS=1 cargo test --release -p mtcore --test semantic_generated
```
Vectors are private (not committed): the test skips without them unless `DDNATIVE_REQUIRE_VECTORS` is set.

## What the checks do and do not cover

- Compared per case: all eight registers, every memory byte except the stack scratch below the returned ESP, and no extra write. Arguments
  the symbolic execution found used as addresses point into a random region (one dword in three is a pointer back into it); absolute
  addresses the function touches get their own random pages. Cases where the original faults are dropped.
- **Flags are not reproduced** by the generated ports. The lifted corpus was scanned: no direct caller of any of them reads flags after the
  call. Virtual callers cannot be scanned, so a port that misbehaves in game is first suspected there (`DDRUNTIME_SEMANTIC_SKIP=<address>`).
- **NaN propagation is not covered by the vectors.** Unicorn returns the second operand for a signalling NaN with a quiet NaN; Intel's rule
  (SDM table 4-7) and the hardware return the first, quieted. The generated float arithmetic uses `x86::mulss/addss/subss` (hardware exact), and
  the vectors for the 58 functions that do float arithmetic contain no NaN or infinity patterns.
- Mutation check (all functions at once): one injected bug per function (a store's offset moved, or a register result flipped); 806 of 807
  mutants were caught, the 807th was a mutation the script failed to apply. An earlier run showed 186 survivors, all stores to image globals:
  the harness then compared only memory at or above the final ESP, which skips the image. It now compares everything except
  `[0x0F000000, ESP)`. The hand-written tests (`semantic_leaves.rs`, `semantic_sound.rs`) use the same corrected filter.

## Not covered (the next steps)

Functions with branches, loops, calls, x87, `div`/`mul`, string instructions, or `cvt*`; 21 candidates whose original faulted too often
under random inputs; functions that use `ESP` as a value (`push ebp; mov ebp, esp; and esp, -8`). About 2,300 of the 3,199 branch-free leaves
are one-instruction accessors that are not worth a port.
