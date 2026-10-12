# L33: readable ports, wave 2 (the MT Framework's core library)

- **Owner:** claude-8
- **Branch:** `task/L33-readable-core` (worktree `E:\ddda-rs-wt\L33`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L24 ported 29 math functions as readable, verified Rust (`crates/mtmath`,
`docs/MATH.md`). The next most-called layer is the engine's core library.
It is everywhere in game code, so readable versions pay off most for
modding and speed:
- containers (MtArray and friends);
- strings (MtString, string hashing such as the CRC/hash used for
  resource and DTI names);
- MtObject/DTI basics (type lookup, `isKindOf`);
- the allocator front-ends (only where they are not platform models).

## Task

1. **Census.** Rank the core-library functions by call sites (Ghidra
   names, DTI, `scripts/inventory.py`, families). List the top families
   with addresses in Results.
2. **Port them readably** in a new crate (`crates/mtcore`), as in L24:
   - the original layouts (`#[repr(C)]`), bit-exact results;
   - verified by the lifted vectors plus recorded edge cases
     (`research/native/vectors_math.py` is the pattern);
   - wired in through the PORTS table mechanism.
   Leave a function lifted when a port would have to replay dead state;
   L24's dead-stack audit shows why.
3. **Document** the types for modders in `docs/CORE.md`.
4. Merge-train rule: mtcore's own tests on your branch. The ddnative
   replay with the ports wired in, and the benchmark, run in the
   integration build.

## Results

(open)
