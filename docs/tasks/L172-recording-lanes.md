# L172 recording lanes and command-stream oracle (codex-7)

Assigned by head board #13860, alongside codex-11 (lead). Branch and shared
worktree: `task/L172-parallel-shadow`, `E:/ddda-rs-wt/L172`.
The orchestration brief is `L172-parallel-shadow.md`; this file records the
recording-lane half. No game runs or shard builds are authorized here.

## Source audit

Read-only Ghidra, DDDA.exe 2017-12-13 Steam; addresses are source evidence,
not embedded game data. Existing ports remain the machine-level oracles:

| Source | Contract |
|---|---|
| `00DB63E0` | Existing parallel unit draw path calls FED200, 107FF00, publishes eight lane pointers, queues DB7620, joins, calls 107FF30, then FED3B0. Serial path calls unit vf11 directly between begin/end. |
| `00FED200` | Lane stride `0x33c84`; descriptor starts at `+0xb4`; copies `0x144 + 8*parent[+0x14]` bytes. Divides parent sort capacity among lanes, initializes private cursors, and sets lane `+0x58` to its index. Shared manager/resource pointers are copied, not cloned. |
| `00FED3B0` | Concatenates lane roots (`+0x44` pointer, `+0x48` count) in lane-index order. For each lane's nested-record table `+0x30878` / count `+0x30c78`, copies child entries into the parent's descending tail (`+0x4c`) and rewrites record `+8`. It is not a global sort. |
| `00FED480` | Saves descriptor to `context + 0x81f8 + depth*0x8144`; increments `+0x30874`. Full mode copies `0x144 + 8*context[+0x14]`; short mode copies `0x144` plus six install-selected values. This does not snapshot allocator/root cursors, resources or scheduler state. |
| `00FEDB80` | Also saves caller identifier and `+0x48` sort cursor at `+0x3084c + depth*8`, copies descriptor, increments depth, starts nested recording. |
| `00FEDC00` | Restores descriptor, moves newly recorded entries to the sort-array tail and emits a type-6 nested command referencing them. This is not callback rollback. |

Lane slot words live at `*0x018d0b28 + 0x1bdd0 + lane*0x20` for lanes
0..7 (last `+0x1beb0`). Only the first word of each slot is the recording
pointer. Neighboring slot bytes must not be overwritten by a pointer restore.
`LaneSlots` reads/writes only those eight words; callers must use a private
scheduler or exclude all other scheduler consumers.

`0107FF00` / `0107FF30` are sPrimitive singleton virtual dispatch wrappers,
not the recording-lane merger. Singleton `*0x018d1848`, constructor
`00E66B40`, vtable `01430E08`, slots `+0x2c` / `+0x30` resolve to:

- `00E61010`: begins nested primitive lists via `0111D610` in each primitive
  lane. Primitive lane pointers are sPrimitive `+0x30..+0x4c`.
- `00E68390`: serial mode selects primitive pool using context `+0x58`.
  Parallel mode uses shared merge cursors at sPrimitive `+0x50..+0xcf`,
  queues sorting jobs (`0111E180`, DBE560 kind 2) and optionally primitive
  draws (`010B7440`, kind 3), and writes scheduler lane slots again.
- `0111D610` / `0111D650`: primitive nesting depth `+0x54`; associated
  allocator nesting via `01192260` / `01192290`.

Consequently a scheduler singleton/guard overlay alone is insufficient.
Private recording contexts also need compatible primitive pools and all
nested dispatches contained. Globally changing the nested guard or global
function overrides would expose an experiment to unrelated running threads.
Direct lifted/local calls can bypass an external-call-only observer; a
diagnostic capture must establish its actual boundary before running them.

Cascade splitting is not established independent: `00EFD240` writes shared
shadow texture resource frame-use stamps (shadow `+0xdc/+0xe0/+0xe4` point
to objects written at `+4`), invokes virtual methods, then traverses models.
`00EA5320` also writes shared geometry/index resource `+4` stamps. Equal
values do not by themselves prove the entire callback independent. Keep
cascades serial; contiguous unit partitions and ascending lane merge are
necessary to preserve serial root order. Dynamic work stealing alone does
not provide that guarantee.

## Implemented oracle

`mtcore::shadow_lanes::capture_context(m, context, limits)` snapshots the root
array. `capture_stream` accepts explicit entries/count and a shader decoder
callback; `capture_shader` is the built-in conservative decoder.
`StreamSnapshot::first_difference` reports only an entry-index path and field,
so diagnostics need not dump game bytes.

- Preserves exact ordered sort keys, command headers, scalar arguments,
  nesting and binding pointer-reuse patterns. No speculative header masks,
  sorting, hash-only equality or skipped commands.
- Normalizes relocated root/child arrays and draw binding addresses; copies
  target `0x5c`, fixed `0x14`, stream `0x30` blocks (DAB480/11F0BE0 layouts).
  Types 0..3 draw, 4 clear with associated draw, 5 copy with associated draw,
  6 nested list and 7 query are represented. Types 8..15 refuse capture.
- Shader metadata/resource IDs remain exact. DAA8B0 descriptor sections
  determine float/int/bool source ranges, float low-nibble count limits,
  signed-conversion descriptor bits and derived texture dimensions. Captures
  consumed constant bytes, not just pointers. Unknown derived kinds, temporary
  overreads, inconsistent section lengths and out-of-range indices refuse.
- Null/wrapped ranges, cycles, record/byte/depth limits refuse the entire
  snapshot. Memory faults propagate to the caller's candidate rejection path.

This is strict structural/constant equality, not an image oracle. Resource
objects referenced by unchanged IDs must be frozen, or independently checked
by the runtime memory oracle. The API does not stop threads, capture coherent
input, roll back side effects, sort streams or execute two game callbacks.
It intentionally may reject equivalent renderings (exact headers, unused
values and metadata identities); weakening those comparisons needs evidence.
Two failed captures never count as a match.

## Results

- **Implemented:** lane pointer snapshot/restore and conservative stream
  comparison in `crates/mtcore/src/shadow_lanes.rs`; module export belongs to
  codex-11's integration commit. No production shadow override in this batch.
- **Tested:** 11 focused CPU tests pass. They cover relocation, 13 individual
  key/header/payload/binding/constant corruptions, order/count differences,
  reused-pointer identity, nested mismatch paths, clear/copy children,
  refusal bounds/cycles/unknown shader forms, tagged floats, BOOL widths,
  zero uploads, and exact restoration of only the eight slot words.
- **Compared against original:** eight FED3B0 returns (0..3 lanes, with and
  without nested lists), executing original memcpy too, no mocked calls.
  Original merged output snapshots equal the input lane streams concatenated
  in lane order, including relocated nested payloads. These are synthetic
  input fixtures executed by original instructions, not gameplay captures.
- Private corpus: `E:/ddda-re/notes/native-L172/lanes/manifest.json`, SHA-256
  `50516a50f8430d2409fb753a4c31814bf31ea8bdc44c73ee3fb75d2c7ae7e62d`.
  The manifest records the installed analysis image SHA and both binary
  fixture hashes per case. Regenerator: `research/native/shadow_lane_vectors.py`.
- Runtime light: initial suite reached 116 passed / one inherited Present
  timing failure / five ignored; the isolated timing retry passed. The second
  run (skipping that timing test) caught the concurrently added lead-owned
  stream-audit fixture (116 passed / one failed / five ignored). Lead corrected
  the fixture and owns the final full light gate in `L172-parallel-shadow.md`.
  My private logs are in `C:/ddda-targets/L172-codex7/runtime-*.log`.
- **Not proven:** same-run original serial vs candidate parallel shadow
  streams, callback side-effect isolation, original shader-consumer replay
  for the new diagnostic decoder, image equality, speedup or FPS. Do not
  advertise this prerequisite batch as acceptance of parallel shadow.

Reproduce with `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`,
`CARGO_TARGET_DIR=C:/ddda-targets/L172-codex7`:

```
E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex-7 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/shadow_lane_vectors.py
DDNATIVE_REQUIRE_VECTORS=1 cargo test -p mtcore --test shadow_lanes
cargo test -p ddruntime --no-default-features
```

Blocker: the lead's isolated dispatcher still needs a coherent same-input
boundary and containment of direct calls and host services. Next action:
integrate the oracle with that boundary, run original serial and isolated
parallel candidate, and require successful complete stream and memory
comparisons before requesting a head-only game acceptance run.
