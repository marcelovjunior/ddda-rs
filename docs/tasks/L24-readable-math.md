# L24: readable ports of the MT Framework math library

- **Owner:** claude-8
- **Branch:** `task/L24-readable-math` (worktree `E:\ddda-rs-wt\L24`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The lifted program is correct but unreadable: register-level Rust. The
player wants to mod, optimise and unlock the game (frame rate and more),
and that needs readable code, starting where it matters most. The engine's
math library is a good start:

- It is small and self-contained.
- Everything calls it: vectors, matrices, quaternions, angles,
  interpolation, random numbers.
- It is hot, so a readable port that the compiler can optimise also helps
  performance.

## Task

1. **Census.** Find the math library in the exe: MtVector3/4, MtMatrix,
   MtQuaternion, MtFloat helpers, the angle/trig helpers and the random
   generator. Use Ghidra names, DTI classes (`docs/DTI.md`), call counts
   (`scripts/inventory.py`) and byte families
   (`research/native/families.py`). List the functions in Results with
   their callers' counts.
2. **Port** each function as readable Rust over named types: `Vec3`,
   `Mat44` and so on, with the original's memory layout and field offsets
   (`#[repr(C)]`). This follows `docs/NATIVE_PIPELINE.md` ("how to port a
   DDDA.exe function"):
   - same results bit for bit, including x87/SSE precision, rounding and
     NaN behaviour;
   - `ddnative::x87_copy` where the original copies floats through x87;
   - verified by the same recorded vectors as the lifted function
     (differential test);
   - a port counts only when its vectors pass.
   Where the original's float semantics cannot be expressed readably
   without changing results, keep the exact form and explain it in a
   comment.
3. **Wire** the verified ports in, so the runtime calls them instead of the
   lifted bodies. Use the pipeline's existing mechanism for verified ports
   (the `Translated from` convention and the resolver); read how
   `ddnative::action::construct` was done. Measure the difference with a
   micro-benchmark (lifted against port).
4. **Document** the library in `docs/` (types, layouts, conventions) for
   modders.

## Results

claude-8, branch `task/L24-readable-math`. Details for modders and porters:
[docs/MATH.md](../MATH.md).

### Census

MT Framework's vector and quaternion arithmetic is inline in DDDA.exe; what
is out of line was found by shape (`research/native/math_census.py`: small
functions mostly made of SSE/x87 instructions, ranked by direct callers),
the xorshift constants (MtRandom) and Ghidra. Callers are direct call
sites' functions from the inventory.

| address | callers | function | state |
|---|---:|---|---|
| 0x00D00690 | 386 | MtRandom::next (xorshift128) | ported, wired |
| 0x00D006C0 | 4 | MtRandom::seed | ported, wired |
| 0x007AD9D0 | 82 | yaw from one point to another | ported, wired |
| 0x007AD8E0 | 66 | point rotated about y (CRT sinf/cosf) | ported, wired |
| 0x007AD7A0 | 65 | yaw of a normalized direction | ported, wired |
| 0x007AEED0 | 50 | unit normal of b x a with a fallback pair | ported, wired |
| 0x007AD870 | 46 | difference of two angles, short way round | ported, wired |
| 0x00CFCF80, 0x00CFC9E0, 0x00CFCE60, 0x00CFCB00, 0x00CFCC20, 0x00CFCD40 | 45, 12, 6, 5, 5, 5 | MtMatrix to Euler angles, six orders (CRT atan2/asinf) | ported, wired |
| 0x00CFD5E0 | 35 | MtMatrix lookAtRH | ported, wired |
| 0x007AF5B0 | 31 | frame from a forward vector | ported, wired |
| 0x007ADCA0 | 17 | target inside a view cone (CRT sinf/cosf) | ported, wired |
| 0x007AE860 | 15 | quaternion turned toward a target (CRT acosf/cosf/sinf) | ported, wired |
| 0x00CFC1D0 | 14 | MtMatrix from Euler angles (CRT sinf/cosf) | ported, wired |
| 0x007AF450 | 13 | frame from an up vector | ported, wired |
| 0x00CFC350, 0x00CFC4B0, 0x00CFC620 | 12, 3, 2 | perspective from field of view RH, RH scaled, LH (CRT tanf) | ported, wired |
| 0x00CFC8B0, 0x00CFC940 | 12, 3 | orthographic RH, LH | ported, wired |
| 0x00CFD890 | 5 | MtMatrix lookAtLH | ported, wired |
| 0x007ADC00 | 5 | angle between vectors (CRT acosf) | ported, wired |
| 0x00CFDB10 | 2 | reflection in a plane | ported, wired |
| 0x00CFC770, 0x00CFC810 | 1, 1 | perspective from size RH, LH | ported, wired |
| 0x00CFDC80 | 10 | matrix blend | lifted: ~130 spills to an aligned frame |
| 0x00CFD0A0 | 2 | multiply by an Euler rotation | lifted: 22 spills, low value |
| 0x007AEE90 | 12 | random float in a range (x87) | lifted: extended precision |
| 0x00D4EBF0, 0x00D4EC50 | 25, 49 | cubic curves (x87) | lifted: extended precision |
| CRT 0x012F0906/0A55/0BAC/0D4C/161F/1109/1E8A/2042 | | sinf, cosf, tanf, asinf, acosf, atan2, sin, cos (identified by running them) | platform, called by the ports |

29 functions ported, 1,497 direct call sites in 786 calling functions.

### What landed

- `crates/mtmath`: `Vec3`, `Plane`, `Mat44`, `Quat`, `Random` with the
  original layouts (`#[repr(C)]`, offsets documented, a layout test);
  readable ports in `random`, `matrix`, `euler`, `angle`, `vector`, `quat`,
  each with its `Translated from` line and constants cited by address.
  `scalar::F32` makes `+ - * /` one SSE instruction with the written
  operand order, so even the payload chosen when two NaNs meet is the
  original's (pinned on the host CPU by `scalar::tests`).
- `crates/mtmath/src/abi.rs`: an adapter per port with the lifted ABI
  (`fn(&mut dyn Mem, &mut Cpu)`): arguments where the original reads them,
  every general register, xmm0, the x87 state and every byte the original
  writes (pushes, aligned-frame spills, overwritten argument slots); the
  `PORTS` table also records which other xmm registers are reproduced
  (all eight for 24 of the 29). Ports that call the CRT reach it through
  `x86::call_ext` exactly where and how the lifted body does.
- Wiring: `research/native/lift.py gen` reads `PORTS` and writes, in the
  shard holding each address, `pub use mtmath::abi::f_X;` (an explicit
  import shadows the lifted glob, so intra-shard calls, `FUNCTIONS`,
  `lookup()`, `call_lifted` and the runtime's `lifted::all()` all reach
  the port) plus `pub mod replaced` keeping the lifted body. Regenerating
  changes exactly three `mod.rs` files; shard manifests gain an `mtmath`
  dependency. Agreed with claude-6 (holder of lift.py, #9165/#9200).
- Verification tooling: `research/native/vectors_math.py` (lifted vectors
  for the ported addresses; edge vectors from the original in Unicorn
  with full xmm state, garbage-filled objects and stack, every ESP
  alignment, recorded CRT callouts, poked globals, and Unicorn's two-NaN
  SSE results corrected to the hardware rule), `math_liveness.py` (lane-
  precise check that no caller reads an xmm lane an adapter does not
  reproduce), `test_math_wiring.py`, the tracker crediting `crates/mtmath`
  (`scripts/inventory.py`, test added) and `native_gate.sh` running the
  mtmath tests.
- `research/native/stack_audit.py`: the dead-stack audit (below).
- `ddnative::x87_copy` moved to `mtmath::scalar` (ddnative re-exports it;
  its CPU test is unchanged).
- `crates/mtmath-bench`: the micro-benchmark.
- `docs/MATH.md`.

### Verified, and how

- `cargo test -p mtmath` with `DDNATIVE_REQUIRE_VECTORS=1`: the lifted
  program's own vectors for every port (87, the lifted test's contract:
  registers, xmm0, exactly the written bytes, no unrecorded read,
  recorded ext calls checked register by register) and 8,700 edge cases
  recorded from the original (300 per function; 815 with two-NaN SSE
  results, 4,800 with CRT callouts), all general registers, the
  reproduced xmm registers lane by lane, object and stack windows byte by
  byte. Passes in debug and release. Branches counted where they matter
  (Euler poles 44..194 cases each; unit normal 184/41/75; rotate-toward
  44/25/231). Mutations caught: pi off by one ulp, one operand swap in
  lookAtLH, one sign in the Euler table.
- `math_liveness.py`: 0 problems over 1,497 call sites; Ghidra lists only
  direct calls to the ported functions.
- PENDING THE TRAIN (head's batched verification, AGENT-RULES.md): the
  full required-vector replay of the lifted program with the ports wired
  in (`gen` + `cargo test -p ddnative`: all of `vectors/lift.json` through
  `lifted::all()`, which now holds the ports, and every caller of them),
  the native gate, and the benchmark (`cargo run --release -p
  mtmath-bench`: lifted body, adapter and readable function per call, for
  nine of the ports that call no CRT routine). Not run on this branch: one
  attempt lost rustc to the memory guard while the player's game held the
  RAM, then the pause and the new rule came. `cargo check` of the shards
  and mtmath-bench has not run either.
- Private data for the train: `vectors/math_lift.json` and
  `vectors/math_edge.json` (written by `research/native/vectors_math.py
  lift|edge`, already in E:/ddda-re/notes/native/vectors). The patterns
  cache and `vectors/lift.json` are unchanged (17:29/17:30).

### Not done, and why

- 0x00CFDC80 and 0x00CFD0A0: correct to port, but the adapter would have
  to replay a long schedule of dead spills (about 130 and 22 slots).
- Relaxing the lifted contract to ignore memory below the caller's ESP
  (head's proposal, conditional on an audit): the audit says no.
  `research/native/stack_audit.py` follows every path of all 59,290
  inventory functions (ESP/EBP tracked from entry; escaped frame addresses
  count as written; ESP untracked after indirect calls and alloca). 332
  functions read bytes of their own frame they never wrote on that path
  (1,883 reads, 512 partial), so they see what earlier callees left below
  their caller's ESP: 0x0046AC80 loads an unwritten local into EDX,
  0x00470680 writes 2 bytes, reads 4 and pushes them as an argument,
  0x004ECE60 copies 16-byte slots of which 8 or 12 bytes were written.
  Reads below ESP itself: 7 functions, all alloca frames the model does
  not track (3) or CRT routines (4). The strict contract stays; no lifter
  or recorder change (posted to head, claude-6, claude-3: #9262).
- The x87 functions: a readable `f64` form is exact only under 53-bit
  precision control, and the guest's control word is not established
  (docs/NATIVE_PIPELINE.md); spelled with x87 operations they would be
  the lifted body again.
- docs/RUNTIME.md: one sentence on mtmath added to the hand-ports
  paragraph (agreed by codex, #9268). docs/NATIVE_PIPELINE.md: proposed
  subsection sent to its owner claude-3 (#9263), no answer yet, not
  edited.
