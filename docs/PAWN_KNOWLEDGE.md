# Pawn knowledge: saved bits, subjects and stars

`ddsave::knowledge` maps the existing `pawns::Knowledge` storage to tables
read from the player's installed executable. It reports progress without
awarding knowledge or writing saves. All source addresses refer to DDDA.exe
2017-12-13 Steam. Each calculation takes an explicit pawn's arrays.

## Storage and subject tables

The save reader already decodes 322 u32 words in each of `mStudyFlag` and
`mLocalStudyFlag`, 72 f32 encounter counters, 72 u32 kill counters and 116
u8 unique counters. See [SAVE_PAWNS.md](SAVE_PAWNS.md). One bit number is
`word = bit >> 5`, `mask = 1 << (bit & 31)`; valid bits are below `0x2840`.

Three executable tables identify which bits contribute to each subject:

| Class | VA / stride / rows | Subject ID | Readers/menu consumers |
|---|---|---|---|
| Quest | 0x01521e90 / 24 / 52 | Quest number at +0 | 0x005d06d0, 0x0070ebc0 |
| Enemy | 0x015231e0 / 20 / 97 | Enemy class ID at +0 | 0x005d0680, 0x005d08f0, 0x0070e910 |
| Area | 0x01522670 / 24 / 18 | Area/stage ID at +0 | 0x005d07a0, 0x005d07e0, 0x0070eea0 |

All have a pointer to ordered u32 bit numbers at +4 and a bit count at +8.
The last word is the category-specific name-message index used by the
menu. Enemy indices below zero disable that menu row; 24 of the 97 installed
rows have this sentinel. There are 96 distinct enemy IDs across 97 rows,
so the reader preserves ordered rows instead of silently deduplicating.
Other metadata is exposed without guessing its meaning.

Quest +0xc is a separate completion/knows-quest bit. `0x005d0760` requires
it to be nonzero and below `0x2840`, then tests that one bit. This predicate
is already used by the Rift quest ordering; it is not the same as a star
count. `0x00416da0` awards this completion bit to eligible party pawns and
can copy known constituent bits from the main pawn to hired pawns. The
awarding/notification behavior is traced here but not implemented.

`0x005d08a0` maps an area bit back to an ID by scanning the area table
backward. It requires `0x2040 <= bit < 0x2840`, skips empty lists and those
whose first bit is greater than the target, then searches exact membership.
`Tables::area_for_bit` preserves that reverse precedence for shared bits.
It returns `None` instead of using zero as a missing-ID sentinel.

`Tables::from_image` uses the existing build-checked `ddexe` reader. It
follows file-backed pointers at runtime; no bit lists, threshold tables,
save records, raw executable bytes or decompiler output are embedded.

## Star calculation

`0x005d05e0` counts present bits from the subject's list. Out-of-range
bits contribute zero; duplicates count separately if present. Its total
is the list count, not the number of valid or unique bit IDs.

`0x005d0830` converts learned and total counts to f32, divides, and returns:

| Condition | Result |
|---|---:|
| total == 0 | -1 (Rust `None`) |
| ratio <= 0 | 0 |
| 0 < ratio < 0.5 | 1 |
| 0.5 <= ratio < 1 | 2 |
| ratio >= 1 | 3 |

The three menu consumers above call this helper and select display text
from the table at `0x0158820c` with **four-byte stride** (assembly
`0x0070ed4f`), although only its low u16 is used. Do not infer byte stride
from Ghidra's displayed symbol type. This brief reports the normalized
star value; it does not render those text resources or reproduce all
comparison-menu visibility rules.

Zero learned bits can hide a row in an ordinary menu; a comparison pawn
may keep a zero-knowledge placeholder visible. Local study flags also gate
name visibility in these menus. `Entry::progress` intentionally reports
raw progress independently of those presentation rules.

## Counters that unlock bits

`0x004172c0` uses 92 `(enemy ID, counter index)` pairs at `0x0153b6e8`,
stride 8. These map to the save's 72 encounter/kill entries; multiple
enemy IDs share counters. **Eleven mapped IDs have no knowledge-menu row.**
The original lookup skips them, so a complete one-to-one join is wrong.

For each mapped enemy it looks up the first five bits in its knowledge
list. Two tables, each five entries per counter, provide requirements:

- f32 encounter thresholds at `0x0153b9c8`;
- u32 kill thresholds at `0x0153bf68`.

An unlearned bit needs both `saved_encounter_frames >= threshold * 30`
and `saved_kills >= required_kills`. Equality passes: disassembly at
`0x00417402` uses `COMISS` followed by `JA`, and `0x0041740f` uses an
unsigned comparison followed by `JC`. The decompiler's rewritten float
expression is misleading at equality. The loop stops at a failed gate
or a zero bit; it skips already learned bits. `Threshold::met` implements
the numerical gate only; no automatic award loop is introduced.

The factor 30 is traced, but this brief has not established how the
encounter counter's update units map to wall-clock time in every state.
Nonfinite inputs are explicitly rejected by the Rust helper instead of
reproducing unordered hardware comparison behavior.

`0x00417470` reads 113 records at `0x0153c508`, stride 12: knowledge bit,
unique-counter index, and required value (low byte). It awards an unknown
bit when the corresponding saved byte is at least that value. The save
has 116 unique counters, so neither index nor rule count may be inferred
from the other's length. The reader preserves these mappings and checks
counter bounds. `UniqueRule::met` is the numerical gate, not the full
party/AI eligibility logic.

## Original-written save examples and checks

On 2026-10-09, the private backup's main pawn decoded through
`mPlayerDataManual/mPlCmcEditAndParam/mCmc[0]` produced:

| Class | Subject ID | Learned / total | Derived stars |
|---|---:|---:|---:|
| Quest | 5 | 6 / 6 | 3 |
| Enemy | 8 | 2 / 11 | 1 |
| Area | 210 | 3 / 3 | 3 |

The same save has enemy ID 5 mapped to counter 0 with encounter value
254.52989 and four kills. Unique counter 1 contains 16; its rule for bit
4176 requires 20, so that gate is not met. These are bounded observations,
not a copied save or a bulk data table. No identity values are reported.

Five new tests cover bit-word edges, the last valid bit, invalid bits,
duplicate counting, the completion-zero sentinel, exact star thresholds,
f32 large-count rounding, counter threshold equality, unique-array bounds,
reverse area precedence, installed table parsing and one private-save
example per knowledge class. `cargo test -p ddsave` passes 18 tests,
including the existing save and pawn-reader tests. Targeted Clippy passes
with the pre-existing `field_reassign_with_default` lint in `pawns.rs`
allowed; that unrelated code is untouched.

Implemented and source-tested: table readers and pure progress/gate helpers.
Compared with original-written data: the three subject examples and saved
counters. **Not visually compared:** the original knowledge menu's displayed
stars, name visibility and comparison state. No process control or writes
were needed for this batch.

Remaining: map all counter increment events and eligibility/notification
rules; connect localized subject names; reproduce menu name/visibility
policy using the viewing player's local flags. Next action: capture the
main pawn's three knowledge tabs and compare the above progress with the
same session's arrays before implementing automatic knowledge awards.
