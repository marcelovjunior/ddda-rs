# L176: nDraw material parameter lookup, semantic ports

Owner: claude-9. Branch `task/L176-material` (from `task/fps-line`).

## Scope

The head's model-draw ranking (board #13852, stacks_c26vk: 353 samples
under uBaseModel::vf11 0x00EA3130, the shadow job and the main-pass draw
jobs) lists the material path among the unowned nearest-lifted self
costs: 0x010655A0 (12 samples), 0x01066C60 nDraw::Material::vf12 (11)
and 0x01066590 (12 in c26def, called through 0x01066B10). This batch
ports the lookup pair:

- 0x01066590 `this->find(slot, key) -> entry`: the 12-byte parameter
  entry of a slot whose key (bits 20..31 of its first word) is
  `key & 0xfff`. Two or fewer indices are searched in order, more by a
  binary search.
- 0x01066B10 `this->bind(key, value) -> found`: find in slot 0, then fill
  the entry's second word from the binding table at 0x018D12CC and its
  third from `value`.

## Method

Semantic ports (L171's rule): EAX, the callee-saved registers, ESP and
every memory write at or above the returned stack match the original;
ECX, EDX, the flags and the popped stack scratch are not reproduced. Two
details matter:

- The binary search wraps: with five or more indices and a key below the
  first, the upper bound passes below index 0, the midpoint becomes
  0x7FFFFFFF and the original reads the word before the index list. The
  port does all index and address arithmetic with wrapping u32
  operations, so it reads the same words.
- The original reads its key only once the count is non-zero, and
  `bind`'s value only when the key is found; the port reads in the same
  order (no read the original does not make).

Vectors: `research/native/record_material.py` runs the original code from
the user's exe in Unicorn on synthetic material tables: counts 0 to 400;
sorted, duplicate-key and unsorted tables; hits, misses below, above and
between, and key words with high bits set. Every conditional branch is
covered except 0x10665BA's taken edge, which is unreachable (a zero count
returns at the first test). 1528 cases (92 runs whose wrapped search left
the synthetic memory are skipped), private output
`E:/ddda-re/notes/native-L176/material_lookup.json`.

## Results

- `crates/mtdraw/src/material.rs`: `find_parameter`, `bind_parameter`.
- `crates/mtdraw/tests/material.rs`: 762 (0x01066590) and 766
  (0x01066B10) ABI-level cases pass.
- `crates/ddports`: opt-in `DDRUNTIME_MATERIAL=1` registers both.
- Microbenchmark (private, `C:/ddda-targets/L176/bench`: the lifted
  bodies and the ports on the same recorded cases, paged guest memory,
  release build for both): find 28-36 -> 20-25 ns (1.34-1.43x), bind
  121-127 -> 49-53 ns (2.4-2.5x).
- mtdraw and ddports tests pass, with and without the switch.

Not yet measured in game. Next: 0x010655A0 (material state apply, SSE
compares and conversions) and 0x01066C60 nDraw::Material::vf12.
