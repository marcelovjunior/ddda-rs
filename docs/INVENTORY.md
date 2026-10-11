# Inventory: how much of DDDA.exe is rebuilt (Phase 0 findings)

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

With the platform layer split off (2026-10-11, commit b541eb1; the gate
prints the current numbers after every run):

| status | functions | share | code bytes |
|---|---:|---:|---:|
| platform (Rust replaces) | 12,735 | 25.2% | 5.2% |
| verified (1:1 + differential test) | 28,675 | 56.8% | 47.0% |
| translated (cited, not verified) | 666 | 1.3% | 4.8% |
| documented only | 280 | 0.6% | 1.6% |
| untouched | 8,122 | 16.1% | 41.4% |

Nearly all verified functions come from the lifter (`research/native/lift.py`,
docs/NATIVE_PIPELINE.md "The lifter"): 28,665 lifted, most of them in ext
mode (calls to untranslated code mocked and replayed), with exact x87
arithmetic since L05. The scan has no size bounds; it lifts 31,977 of the
37,728 non-platform functions (85%); the rest lack vectors or hit
unsupported code: "disassembly" 1,025, indirect 860, "falls" 820, sbb 414,
packed 362, div 263, ...

The game to rebuild is about **37,970 functions** (14.3 MB of code).
