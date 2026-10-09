# Effect resource links

`mteffect` reads DDDA PC effect providers and the resource-bearing portions of
effect lists. It implements no particle simulation, playback, renderer or game
process access. Evidence is the 2017-12-13 Steam executable identified in
METHODOLOGY.md and the installed ARC resources, inspected read-only on 2026-10-09.
Nothing here has been compared visually against the running original.

## Provider grammar

Loader `rEffectProvider::vf10` at **0x00cb6cf0** reads:

1. `epv\0`, u32 version 0, u32 index count.
2. For each index, u32 element count. Each element contains eight NUL-terminated
   rEffectList paths, one rEffect2D path, then **69 four-byte words**. Empty paths
   mean no resource. String reader **0x00cfee30** consumes through NUL (the
   engine's destination buffer truncates long strings; the reader preserves
   their complete serialized contents).
3. A u32 motion-link count followed by **48-byte records**: motion number,
   selector, three f32 frame parameters, five u32 flags, index and element.
4. A u32 event-link count followed by **12-byte records**: event number,
   index and element. EOF must immediately follow.

The 69 words include transform vectors, color/loop-related parameters, individual
serialized flag words and three LOD records. They are retained verbatim as
`Element::parameters`, not misidentified as a packed 0xe0-byte runtime object.
The reflected DTI classes `rEffectProvider::EffectElement` (0xe0),
`EffectMotSyncParam` (0x24), and `EffectEventParam` (0x10) confirm runtime sizes
but expose no field properties in the harvested PC reflection data.

`Provider::element` follows the two-level index. Parsing validates every motion
and event target, count, string, fixed field and EOF. Motion selector, flag
semantics and the third frame parameter remain uninterpreted. In particular,
end frame zero is common and is not rejected as an inverted interval; this
reader does not evaluate playback windows.

## Effect-list grammar and coverage

`rEffectList::vf10` **0x01147790** checks `EFL\0` and version **0x20110318**.
The header is 48 bytes; +8 is payload length, +12 an id, +16 a u16 unit count,
+18 an auxiliary u16 count, and +20 flags. Four optional payload-relative
offsets occur at +32, +36, +40 and +44. Unknown header words and body regions
are not assigned speculative meanings.

The payload begins with **16-byte unit records**. Each has four packed words;
the upper 24 bits hold a payload-relative section offset and the low byte a
kind/flag field. Zero offsets are null. **0x01147400** traverses generator,
particle and life resources. The third section is retained as an opaque kind
and offset. All section starts are checked against the payload; known resource
fields and fixed 64-byte strings are checked before access. Unresolved bodies
are retained as payload bytes, not claimed fully decoded or validated internally.

Resource field evidence:

| Consumer | Resource-bearing fields |
|---|---|
| **0x01146b30** | Generator-relative u16 offsets at +0xb8 strip, +0xba vibration, +0xbc sound request |
| **0x01146bf0** | Particle-kind dispatch: texture and animation strings, model, grass-wind and alternate inline/relative animation paths |
| **0x01146e80** | Life-kind 3 strip; kinds 0–8 optional +6 relative substructure with two +0x1c/+0x1e relative child-effect paths |
| **0x01147400** | Optional header +44 structure's +10 relative child-effect path |
| **0x011467c0** | Texture lookup falls back to rRenderTargetTexture (both loading-flag modes) |
| **0x01146890 / 0x011468d0 / 0x01146910 / 0x01146950** | Animation / model / strip / grass-wind resource loaders |

`resources` retains references in traversal order, including duplicates. Texture
references use class `rTexture`; callers must honor the documented render-target
fallback. Recursive child lists are references, not recursively opened during
parsing. The corpus test reads each list independently, so cycles cannot recurse
through the reader.

The private MIT Riftstone format summary's **Effects** section supplied initial
format leads and warns that particle/joint/life/movement bodies are incomplete.
The implementation was written from the PC loaders above; no reference code or
game bytes were copied. The auxiliary count is intentionally not promoted into
a guessed joint-table layout.

## Traced attack chain

**`cPlActAssassinAttack::vf05` at 0x00b28d00** requests motion **0x723** through
the actor's virtual +0x90 motion method. A conditional variant requests 0x75e;
the conditions are not implemented here. This is the executable class name,
not a claim about the localized skill name.

Weapon category 4 selects the dagger provider: executable path at **0x015ea170**,
selection instruction **0x00b60163**, load helper **0x00b572e0** using
rEffectProvider. The corresponding provider is `effect\epv\pl\p_pl_dag`.

The motion is in a **custom motion list**, not the base `m0005_at` list:

- Custom table pointer **0x0163dfc0[4]** points to **0x014f1f18**. Entry 19 at
  **0x014f1fb0** holds bank 0x700 and
  `motion\pl\m\m00\m0005_at\m0005_at_19`.
- **0x00b5b400** indexes this weapon-category table using selected skill id minus
  the category's base. Category 4 has base 0x96 and count 0x14 in the tables at
  0x014f6c80 and 0x014f6cb4. Thus this entry corresponds to selected skill id
  169. This is a static load-path trace, not a live equipped-skill observation.
- `uPlayerBase::vf36` **0x00b800c0** separates bank `(motion >> 8) & 15` and
  slot `motion & 255`. LMT slot **35** for 0x723 contains **91 frames**.

The EPV contains these three 0x723 records, in this order:

| Index / element | Start / end / third frame value | Nonempty EFL path |
|---|---|---|
| 19 / 1 | 1 / 0 / 0 | `effect\efl\sk\dag\l_c_dag020_02` |
| 19 / 2 | 0.1 / 15 / 8 | `effect\efl\cm\l_cm010_38` |
| 19 / 3 | 42 / 0 / 0 | `effect\efl\cm\l_cm010_38` |

`l_c_dag020_02` contains **28 units and 51 resource references**, including
texture `effect\tex\cm\t_cm000_00_GM`, effect animation
`effect\tex\cm\t_cm000_00`, and model
`effect\mod\m_cm001_10\m_cm001_10_02`. The common list contains **2 units / 4
references**, including `t_cm030_01_GM` and `t_cm030_00_GM` and their matching
animation resources. All resolve. The alternate 0x75e has equivalent list
links at index 44, elements 0–2; that alternate LMT slot is not part of the
regression test.

The install regression asserts the 0x723 LMT slot/frame count, EPV targets/frame
values, 28-unit EFL, representative texture/model references and asset presence.
This proves the resource chain. It does not prove visible particles, exact spawn
timing, LOD selection, attachment joints or skill execution in the original.

## Verification and next step

Run `cargo test -p mteffect -- --nocapture`. The install test uses
`DDDA_NATIVE_PC`, or the maintainer's default nativePC path; it explicitly skips
when the default is absent and fails if an explicitly supplied root is missing.
There are six synthetic tests plus one installation corpus/attack-chain test.

| Class | Unique normalized paths | Every archive copy | Traversed data |
|---|---:|---:|---|
| rEffectProvider | 656 | 7,576 | 144,146 motion links; 78 event links; 208,497 nonempty resource references |
| rEffectList | 4,603 | 58,307 | 828,769 units; 1,451,397 resource references |

All local EPV targets validate. Provider references resolve to **3,354 unique
class/path pairs**; EFL references to **403**, with **zero missing targets** in
the installation index. These are aggregate occurrences, not distinct-content
counts. VFS precedence between different copies remains outside this reader.
Synthetic cases exercise exact fields, every truncated prefix, bad versions,
overflowing counts, trailing bytes, packed offsets, unterminated fixed strings,
and generator/life-relative child paths.

Next: trace the provider's motion-window evaluator and spawn flags, then one
generator/particle kind and its joint attachment, before adding rendering.
