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
- 0x010655A0 `this->apply(context, value)`: the material's states into
  the draw context (below).

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

- `crates/mtdraw/src/material.rs`: `find_parameter` (0x01066590),
  `bind_parameter` (0x01066B10), `apply_material_state` (0x010655A0).
- `crates/mtdraw/tests/material.rs`: 762, 766 and 1080 ABI-level cases
  pass. Memory is compared everywhere but the popped stack scratch (the
  4 KiB below the returned ESP): the recorders put their data below the
  stack, so L171's "at or above the returned ESP" alone would not check
  it (it did not at first here; see below).
- `crates/ddports`: opt-in `DDRUNTIME_MATERIAL=1` registers all three.
- Mutation check (`C:/ddda-targets/L176/mutate.py`, private): six of
  seven mutations are caught; the survivor (`k >= key` to `k > key` in
  the binary search) is equivalent, since equality has already returned.
- Microbenchmark (private, `C:/ddda-targets/L176/bench`: the lifted
  bodies and the ports on the same recorded cases, paged guest memory,
  release build for both, the apply's guest call stubbed): find 26-31 ->
  18-21 ns (1.5x), bind 113-133 -> 40-49 ns (2.7-2.8x), apply 680-870 ->
  536-670 ns (1.3x; its memory accesses and the entry copy loop dominate
  either way).
- mtdraw and ddports tests pass, with and without the switch.

### 0x010655A0, the material state apply

`this->apply(context, value)`: stamps the material with the frame serial
(0x018D2CE8 + 1), writes its shader binding, blend, colour, texture,
extra and flag bits into the context's caches (each group only when it
differs, marking changes in the context's flags at +0x154 and +0x114),
appends a pending record when recording, copies the material's source
entries into the context's slot table, then calls 0x01062F20
(`this->update(context)`, left a guest call) and returns its EAX. The
port keeps the original's read and write order (context and material may
alias), its reloads of `context + 0x6C` and of the source count, and the
colour's SSE steps through the same hardware helpers as the lifted code.

Vectors: `record_material.py --state`, 1080 cases from a warm run (the
original run once untraced, so the caches hold what it sets, then single
cache fields changed) or random caches; colour components at the
threshold, NaN, infinities, denormals and conversion overflow; the
constants at 0x01406CE0 and 0x0161C298 from the image. The guest call is
stubbed: at the call the test checks ECX, ESP, the pushed argument and
return address and every byte written so far outside the stack scratch.
Every conditional branch is covered.

Not yet measured in game. Next: 0x01066C60 nDraw::Material::vf12.
