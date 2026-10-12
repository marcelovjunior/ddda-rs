# Inventory: how much of DDDA.exe is rebuilt (Phase 0 findings)

## Exhaustive residual census (L13)

`research/native/residual_census.py --require-evidence` writes one private
`LIFT_DATA/residual-census.json` table covering **every** inventory row whose
status is neither `platform` nor `verified`, including thunks. Run it after
rebuilding `INVENTORY_DB` privately from the generated set that passed Rust
replay. Each row retains inventory status/size/thunk flag, an entry instruction,
category, reason, and matching-body trial logs with seeds and failures.
Lift rejections include diagnostic instruction addresses; direct thunk targets
include their inventory status. Stale-body trials cannot establish evidence.
Missing candidates, unrecorded entries, unadopted cases, and selected entries
without inventory credit make strict census completion fail.

The L12 starting database has **832** such rows: **588 non-thunks and 244
thunks**. Its status columns are 724 untouched, 59 documented, 49 translated.
Those statuses overlap the lifter lists; they are not additional populations:

| Starting category | Entries | Evidence |
|---|---:|---|
| Previously excluded thunks | 244 | Inventory thunk flag, decoded jump and target |
| Liftable but no accepted fixture | 493 | Candidate body and bounded recorder failures |
| Lift rejection | 86 | Strict decoder/emitter rejection |
| Quarantine | 4 | Existing invalid-return/mismatch exclusion list |
| Explicit platform-model overlap | 5 | `harness.PLATFORM` and platform implementation |

The old 497-candidate list also included four **already verified** hand ports:
`0x008D4840`, `0x008FBDD0`, `0x0095A810`, `0x009C5890`. The missing-entry scan
now excludes verified inventory rows and includes unverified thunks. The five
model overlaps are `0x00CF73F0` (pool allocation), `0x00DE0B80` (allocation),
`0x00DE0B90` (free), `0x012EFE20` (memset), and `0x012EFEA0` (memcpy). Code
citations had given these `translated` status after platform classification;
they are explicit replacements, not missing instruction translations. L13
records that distinction without relabeling the shared database.

No otherwise unexplained non-platform entry remains outside these categories.
An entry's failure does not prove it is data or an invalid function: some
supplementary entries require internal stack/register context, e.g.
`0x00D97031` reaches an epilogue that restores a frame not established at that
entry. Such contexts remain unverified rather than being removed from inventory.
The final strict census has **559 entries**, all non-thunks: 467 recorder
rejections, 83 lift rejections, four quarantines and five platform-model
overlaps. There are **zero unaccounted, unrecorded or unadopted entries**.
All 244 formerly excluded thunks have verified tail-transfer fixtures; they
remain excluded from the conventional non-thunk coverage denominator.
Input SHA-256 hashes identify the inventory and matching manifests used.
Current resource/test results are in
[L13 Results](tasks/L13-residual-census.md#results).

## Supplementary discovery (L10)

The Ghidra export is not a complete entry-point census. Static initialization
at `0x012EDABC..0x012EDAC9` reaches thousands of functions absent from it.
`scripts/discover_functions.py` reads the player's installed PE and emits
`extra_functions.tsv` alongside a JSON input-hash/report file, both private.
`scripts/inventory.py build` merges the table with the original export;
existing exported names and sizes win on duplicate addresses. The separate
`supplementary` SQL table retains every entry's sources, evidence and size
classification. Direct-call evidence adds real call edges; initializer and
virtual slots are potential roots, not unconditional graph edges.

Admission is deliberately conservative:

- C and C++ table bounds are checked against instructions at
  `0x012EDA8A/8F/94` and `0x012EDAAB/B0` before reading the slots. Every
  nonzero target must start a decodable executable basic block.
  The registration previously cited at `0x0136DDF0` is inside the initializer
  whose actual table entry is `0x0136DDE0`; discovery records the latter.
- MT Framework RTTI requires a forward-decoded registration through
  `0x00CF5780` from an initializer, a `mov eax,descriptor; ret` slot 4,
  and a matching object-vtable store in a lifted body. For example,
  `aTitle`'s constructor store at `0x00531159` anchors `0x0156F7A8`, whose
  slot 8 at `0x0156F7C8` names missing entry `0x00533360`. `aGameMain`'s
  store at `0x004FAE89` anchors `0x0156CCFC`, including `0x004FB960` and
  `0x004FBA60`. Requiring the store avoids mistaking later slots returning
  a parent descriptor for the start of another table.
- MSVC PE32 RTTI requires a Complete Object Locator, a named type
  descriptor and a class hierarchy whose first base descriptor names the
  same type. Slots must point into executable sections and decode as code.
- MT slot runs stop at another recognized table, a non-code pointer or an
  unconfirmed unaligned target. This avoids pointer-looking trailing
  strings. Arbitrary pointer runs are never admitted. The conservative
  rule can miss tables whose constructor is not in the input manifest.
- Direct calls are decoded only at instruction starts in lifted bodies;
  call-to-next-instruction position acquisition is excluded. The target
  must pass the executable-entry check. A raw search for `E8` is not used.

Sizes are reachable decoded instruction bytes when recursive descent
succeeds, otherwise a one-byte **hint**, identified in `size_kind`. The
lifter discovers bodies independently. Inventory byte totals sum function
sizes; they are not a union of executable bytes (shared tails can overlap,
and hints undercount undecoded bodies). Discovery is not verification:
only generated declarations with tests earn verified status, after the
required replay passes.

The old blanket CRT classification above `0x012E0000` now excludes
supplementary entries. An address in that range alone does not prove that
Rust may replace its initialization. Existing platform shape classifications
remain intact; new entries stay unclassified until evidence or verification
establishes their status.

For a private rebuild, set `INVENTORY_DB` and `INVENTORY_EXTRA` under the
worktree's `LIFT_DATA`; `INVENTORY_EXPORT` can keep pointing at the shared
read-only TSVs. See [pipeline commands](NATIVE_PIPELINE.md#supplementary-inventory-entries-l10).
To regenerate the **shared** inventory after review, the head runs discovery
with the accepted full lifted manifest and
`--out E:/ddda-re/notes/inventory/extra_functions.tsv`, clears private
`INVENTORY_DB`/`INVENTORY_EXTRA` overrides, then runs `inventory.py build`,
`summary`, `partial`, `attribute`, `systems`. Regenerate Rust from the
accepted pattern/vector pair before building the final coverage database.
L10 itself writes neither the shared export directory nor shared native data.

The following earlier snapshots remain historical, not the expanded
denominator. Current L10 measurements are recorded in the
[task results](tasks/L10-inventory-gaps.md#results).

Source: the headless Ghidra export (`research/ghidra/ExportInventory.java`)
loaded by `scripts/inventory.py` into the private tracker
`E:/ddda-re/notes/inventory/inventory.sqlite`. Numbers as of 2026-10-10,
main 26c360c. Regenerate with `python scripts/inventory.py build` and then
`partial`, `attribute`, `systems` (in that order).

## Size of the job

- 50,767 functions (50,478 without thunks), 15.1 MB of code.
- The C runtime block at 0x012E0000+ is 5,588 functions / 335 KB; the
  game + engine to rebuild is about 45,000 functions / 14.8 MB.

## Translated is not complete

"Translated" = the Rust code cites the function's address (1,701, 3.4% of
functions, 8.5% of bytes). `inventory.py partial` checks them:

| check | functions | share |
|---|---:|---:|
| every callee also translated (closed) | 658 | 38.7% |
| a gap note near the citation (not yet / unsupported / stand-in / not traced / placeholder / guess) | 518 | 30.5% |
| **closed and no gap note (complete)** | **480** | **28.2%** |

So about 2.4% of the functions are rebuilt completely. The most incomplete
translated functions are the big orchestrators: `sBbsRpgMain::ctor` (117
untranslated callees), the stage/scenario functions near 0x00501020 and
0x0050DCD0, `cNpcActTalk::vf06`, `uPlayerBase::vf90`, `uEm0100::vf33`.
Those are where route-era translations stopped at the edge of their need.

## Unlabelled code

23,756 functions (8.97 MB, 59% of code) carry no class label (Ghidra
`Global`). `inventory.py attribute` infers a class for 37% of them (46% of
the bytes): 5,798 by contiguity (MSVC links one object file's functions
together, so a run bracketed by one class belongs to it) and 3,037 by
callers (at least 70% in one class). Inferred classes are stored separately
(`inferred`, `method`); labels are unchanged.

## Coverage by system

`inventory.py systems` groups classes into families by name (rough; the
families are regexes in the script). Bytes exclude the C runtime.

| system | functions | bytes | translated (bytes) |
|---|---:|---:|---:|
| unattributed | 9,581 | 4.53 MB | 11.0% |
| enemies | 9,672 | 2.06 MB | 3.7% |
| components/other | 3,122 | 1.01 MB | 6.7% |
| units/other | 2,698 | 0.93 MB | 5.0% |
| GUI | 2,811 | 0.92 MB | 6.6% |
| rendering | 1,165 | 0.76 MB | 10.4% |
| animation/constraints | 812 | 0.71 MB | 8.5% |
| player actions | 2,384 | 0.60 MB | 7.3% |
| effects/shells | 1,741 | 0.57 MB | 4.9% |
| framework (MT) | 1,603 | 0.50 MB | 13.0% |
| player/pawn units | 1,518 | 0.44 MB | 20.4% |
| AI | 1,901 | 0.37 MB | 9.1% |
| resources/archives | 1,524 | 0.29 MB | 8.3% |
| collision/physics | 770 | 0.23 MB | 10.1% |
| camera | 452 | 0.19 MB | 9.5% |
| NPCs | 659 | 0.18 MB | 22.5% |
| sound | 689 | 0.15 MB | 3.9% |
| network/online/rift | 608 | 0.10 MB | 2.8% |
| save/system | 241 | 0.09 MB | 1.8% |
| quests/story/events | 314 | 0.06 MB | 17.8% |
| items/shops/economy | 220 | 0.05 MB | 19.1% |

## What this means for the plan

- Enemies are the largest system (about 2 MB) and among the least covered;
  much of it is per-family code (uEm*), so a shared enemy base first pays off.
- Sound (3.9%) explains the silent build better than any single bug: the
  audio path barely exists beyond playback.
- The framework (MT), resources, collision and AI core are the Phase 2
  foundation; `inventory.py next` lists the untranslated functions the
  translated code calls most (the frontier).
- The unattributed 4.5 MB should shrink as classes are labelled; each Phase
  2 batch should label what it touches.

## Shape families and the platform layer (2026-10-10, later)

Functions that differ only in in-image constants (vtables, globals, data
addresses) and relative call targets are one **shape family**: one
parameterized port, replayed against every member, verifies all of them.
Masking those constants (the unpacked exe has no relocation table, so they
are found by disassembly) gives **1,200 families covering 10,839 functions**;
95 families of 10+ members cover 7,567. They are mostly small (0.86 MB), so
this is the boilerplate layer.

The largest families are C++ boilerplate Rust replaces structurally, now
the tracker's **platform** status (no 1:1 port):

| kind | functions |
|---|---:|
| deleting destructors | 3,961 |
| DTI new-instance allocators (call the allocator selector 0x00CF5E20 first) | 2,474 |
| construct-if-memory wrappers (`if (mem) ctor(mem) else 0`) | 352 |
| MT allocator / memory manager entry points | 5 |
| C runtime / library block (0x012E0000+) | 5,588 |

Gameplay families are worth one port each: the action in-place
constructor (957 classes, `ddnative::action::construct`) was the first.

After L15 (2026-10-11, commit 790a049; 56,593 non-thunk functions including
the supplementary entries; the gate prints the current numbers after every
run):

| status | functions | share | code bytes |
|---|---:|---:|---:|
| verified (1:1 + differential test) | 56,021 | 99.0% | 96.1% |
| not verified (each with a recorded reason) | 572 | 1.0% | 3.9% |

After the address-taken discovery rule (2026-10-11, commit 066d970:
labels whose address is stored as data, such as CRT handlers, become
functions; 59,290 non-thunk functions):

| status | functions | share | code bytes |
|---|---:|---:|---:|
| verified (1:1 + differential test) | 58,628 | 98.9% | 96.0% |
| not verified (each with a recorded reason) | 662 | 1.1% | 4.0% |

58,913 lifted entries with 175,425 recorded cases.

After L17 parts B and C (2026-10-11, commit 4b4c9b9: the SEH prolog/epilog
helpers, the `__EH_prolog3` body and both worker-thread loops): **58,704 of
59,290 verified (99.0%, 96.4% of code bytes)**; 58,989 lifted entries with
175,600 cases. Of the rest, 112 still carry the historical status
`platform` (CRT/library range). They are inside the total but were never
lifted or recorded. Brief L28 (claude-3) brings them through the normal
pipeline; L22 (claude-6) handles the ones the lifter rejects (MXCSR among
them).

After L22 (2026-10-12, task/L22-lift-rejections; **expected, pending the
train-2 replay**): RTTI-less vtables anchored by any vptr store add 210
supplementary entries (59,500 non-thunks); on native-L28b1 plus L22's
468 functions / 1,368 cases (59,497 entries / 177,087 cases), the private
inventory gives **59,212 of 59,500 verified (99.5%, 98.2% of code bytes)**.
The 288 others: 265 liftable without a fixture, 20 lift rejections (17
non-code admissions, two x87-consuming mocked callees, one x87 value across
mocked calls) and three own-return quarantines
([L22 Results](tasks/L22-lift-rejections.md#results)).

After train 2 (2026-10-12, main 1be373e7; replayed by the gate): the
shared data merges L28 batches 1-2 (claude-3) and L22c (claude-6) into
59,502 lifted entries with 177,102 cases. The gate gives **59,217 of
59,500 verified (99.5%, 98.2% of code bytes)**. That is five more than
L22's estimate, because L28 batch 2's records joined it. Known hole: a
function whose lifted body jumps to code past Ghidra's end of it counts
as verified when its vectors never take that path. The CRT memcpy
variant 0x012EDE40 is one (found by `tests/options.rs`); brief L45 fixes
the rule and the count.

After train 3 (2026-10-12, main e3d0fdd6): Codex's L28b batch (aligned
call-stack inputs, `__alloca_probe` promoted, 31 functions added and 24
re-recorded) merged per address with `merge_lift_data3.py`: 59,533
entries with 177,188 cases. **59,248 of 59,500 verified (99.6%, 99.0% of
code bytes).**

After L45 (2026-10-12, task/L45-function-tails; **expected, pending the
train-4 replay**): `gen` declares a function verified only without a
verification gap (unbound direct target, unproven table, stale body).
44 previously verified functions keep unproven tables
(`patterns/verification_gaps.json`). On the train-2 base this gives
**59,173 of 59,500**; on train 3 it gives train 3's count − 44 + 12 boot
additions. The function-boundary rule removes 58 non-code supplementary
admissions (6 of them lifted) on the next inventory rebuild
([L45 Results](tasks/L45-function-tails.md#results)).

After train 4 (2026-10-12, main 863f6817): L45's rules (edge-proven
table slots, frame tails, verification gaps), L27b, the boot triage and
Codex's next batches; 59,571 entries with 177,344 cases. The boundary
rule removed 60 non-code supplementary entries (59,440 non-thunks).
**59,229 of 59,440 verified (99.6%)**. 44 functions with a verification
gap (`patterns/verification_gaps.json`, mostly unproven tables) are no
longer credited. Known overcount of at most 6: memcpy-family records
that still tail-call the inlined frame tail 0x012F9D82 (fixed in L46).

After train 5 (2026-10-12, main 84cf1194): **59,231 of 59,440 verified
(99.6%)**, with 46 gaps not credited. L46 fixed the train-4 overcount
(the memcpy-family records jump to their inlined tails locally).

After train 6 (2026-10-12, main 50b358f8): Codex's guarded-stack and
register-object fixtures add 19 functions. **59,250 of 59,440 verified
(99.7%)**; 59,593 entries with 177,409 cases.

After trains 7-8 (2026-10-12, main 5525e431): L48 proves 14 jump tables
by general rules and slot-sweeps 16 input-indexed switches to full
coverage (30 re-credited, 16 gaps left); Codex records boot stop 14.
**59,282 of 59,440 verified (99.7%)**; 59,595 entries with 177,683 cases.

After train 9 (2026-10-12, main b88555c9): Codex's lazy memset ranges and
tail-wrapper inputs add 10 functions (boot stop 15). **59,292 of 59,440
verified (99.8%)**; 59,605 entries with 177,713 cases.

After trains 10-12 (2026-10-12, main above): Codex's recipes (boot stops
15-16, division/tail and constructed-copy fixtures) and claude-6's L49
(caller-state inputs, more table proofs: 5 gaps left). **59,331 of 59,440
verified (99.8%, 99.4% of code bytes).** The remaining functions, with
class, blocker and owner, are in docs/RESIDUALS.md (Codex, train 13).

The L15 table above and its notes below are
kept for history.

Plus 289 verified thunks; 56,306 lifted entries with 167,621 recorded
cases. The former platform classes (deleting destructors, DTI allocators,
construct-if-memory wrappers, base destructor chains, the CRT, the MT
allocator entries) are lifted like everything else since L14/L15;
`scripts/inventory.py summary` reports them per historical rule. What
remains: 417 lifted entries without a fixture (random and image-backed
inputs cannot drive them), 145 lift rejections (x87 returns, a jump table
the recognizer cannot bound in memcpy, segment-register reads, a few
instructions), ten quarantines (x87 status-word differences between the
host and Unicorn); the CRT's SEH prolog/epilog need a caller-frame
fixture contract (brief L17).

The game to rebuild is about **37,970 functions** (14.3 MB of code).
