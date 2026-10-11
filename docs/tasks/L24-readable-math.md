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

(open)
