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

claude-8, branch `task/L33-readable-core` (stacked on `task/L24-readable-math`,
which it needs for the PORTS wiring). Details for modders: [docs/CORE.md](../CORE.md).

### Census

Method: the inventory's direct-call counts for every function, the most
called ones decompiled in Ghidra and classified; the DTI system from
docs/DTI.md (constructor 0x00CF5780); MtArray's out-of-line members from
the references to its DTI (0x01877D24) and vtable (0x01558298); the string
hash from the CRC tables' users. "callers" = distinct calling functions. Containers, strings and `isKindOf` are
mostly inline (223 of MtArray's 1,203 DTI references are in constructors);
MtString has no out-of-line function with more than 8 callers.

| address | callers | function | state |
|---|---:|---|---|
| 0x00CF5E20 | 10,510 | MtDTI::allocator | ported |
| 0x004027A0 | 1,029 | MtPropertyList::add | ported |
| 0x004038F0 | 974 | MtObject::~MtObject body | ported |
| 0x00402530 | 404 | MtArray::~MtArray body | ported |
| 0x00402620 | 367 | MtArray::reserve | ported |
| 0x004025C0 | 293 | MtArray::clear | ported |
| 0x00CF7340, 0x00CF76C0 | 131, 131 | MtPropertyList ctor, dtor body | ported |
| 0x00CF55B0 | 25 | MtDTI::find(id) | ported |
| 0x00CF5C00 | 25 | MtObject::operator new | ported |
| 0x00D01B60 | 24 | CRC-32 of a C string (DTI ids) | ported |
| 0x00CF5870, 0x00CF55E0 | 15, 2 | MtDTI::find(name), subtree search | ported |
| 0x00CF5450, 0x00CF54A0 | 9, 4 | allocator registry add, remove | ported |
| 0x00CF74E0 | 4 | MtPropertyList::clear | ported |
| 0x00CF5750, 0x00CF5E40 | 4, 1 | allocator index propagate, set | ported |
| 0x00DE0B90, 0x00CF73F0, 0x00DE0B80 | 1,326, 526, 438 | MT free, node pool, MT alloc | platform models |
| getDTI stubs (4,390), vtable-setting ctors | 1 each | trivial per-class stubs | lifted |

18 functions ported; together 13,952 calling functions (the inventory's caller counts; call sites are more).

### What landed

- `crates/mtcore`: readable `#[repr(C)]` layouts and functions (`dti`,
  `hash`, `object`, `array`, `property`, `allocator`), each marked with its
  source address, and an adapter per port in `abi.rs` (`PORTS`, all eight
  GPRs reproduced). Adapters make every call the way the lifted body does
  (virtual calls, imports and ext callees through `x86::call_ext`, the
  platform models directly), so no caller state is replayed and nothing had
  to stay lifted for dead state.
- Wiring: `research/native/lift.py` now reads the `PORTS` tables of both
  readable crates (`PORT_CRATES`; an address in both is an error, OK'd by
  claude-6) and writes `pub use <crate>::abi::f_X;` over the lifted body,
  which stays as `replaced::f_X`. All 16 shards depend on mtcore;
  regenerated shards 04, 12 and 13 (`lift.json` unchanged, no lifted body
  file changed).
- Shard 07's `replaced` paths moved from `lifted_005` to `lifted_006`: the
  L24 mod.rs was generated before main's L17 adoption shifted that shard's
  chunks, so it is stale on any base that has L17. **Integration: rerun
  `python research/native/lift.py gen` after merging L24 and check the
  shard mod.rs files are unchanged.**
- docs/CORE.md for modders; inventory credits mtcore's ports and tests
  (`READABLE_CRATES`); `scripts/native_gate.sh` runs mtcore's tests.

### Verified

- `cargo test -p mtcore` (debug and release) with the vectors required:
  4 unit tests; the CRC table against the exe's (0x014075C0); 53 lifted
  vectors over the 18 addresses (Sparse memory, every register at every
  mocked call); 5,400 edge cases recorded from the original in Unicorn by
  `research/native/vectors_core.py` (garbage-filled windows, every ESP
  alignment, recorded virtual callees and imports, built DTI trees, arrays
  with null elements, property lists and the node pool, the registry,
  strings of many lengths). Clippy clean.
- Teeth: flipping the name comparison's sign fails 179 of the 5,400 edge
  cases.
- Python: `test_math_wiring` (multi-crate wiring, the duplicate error),
  `test_lift_shards` (synthetic shard workspace with mtmath and mtcore),
  `scripts/test_inventory`.
- Private data (E:/ddda-re/notes/native/vectors, not committed):
  `core_lift.json`, `core_edge.json`, `core_tables.json`.

### Pending the merge train

The full ddnative replay with the ports wired in and the lifted-vs-port
benchmark run in the integration build (merge-train rule); not run on this
branch.
