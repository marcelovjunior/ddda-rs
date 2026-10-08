# MtDTI: the engine's class registry

MT Framework registers every engine and game class with a runtime type
descriptor (MtDTI). It is the naming accelerator for reading DDDA.exe: it
gives a name, size and parent for about 4,300 classes, and through each
class's `getDTI` virtual it identifies every class vtable.

All addresses refer to the Steam build in `METHODOLOGY.md` (unpacked
SHA-256 `a1831a43...b5`). Evidence is from static reading of the exe with
the scripts in `research/dti/`; the outputs (`dti-registry.json`,
`dti-labels.csv`) stay in the private tree.

## Registration

Each class has a static-initialiser stub:

```
push a6 ; push a5 ; push a4        ; usually 0, 0, 0
push size                          ; sizeof(class) in bytes
push parentDTI                     ; parent class descriptor
push nameString
mov  ecx, thisDTI
call 0x00CF5780                    ; MtDTI constructor (thiscall)
push dtor ; mov [thisDTI], dtiVtable ; call atexit
```

- 4,405 call sites, found by scanning `.text` for `call 0x00CF5780`
  (Ghidra's auto-analysis had disassembled only 4,343 of them), all
  parsed. Every class except the root `MtObject` has a registered parent.
- Descriptor: 0x20 bytes. Observed fields from the constructor:
  `+0x00` vtable, `+0x04` name, `+0x08` next sibling, `+0x0C` first child,
  `+0x10` parent, `+0x14` hash-bucket chain, `+0x18` size (low 23 bits)
  plus flag bits 23-28 and 29-31, `+0x1C` id = hash(name) & 0x7FFFFFFF
  unless an explicit id is passed.
- Hash table: 256 buckets at `0x018768A0`, indexed by `id & 0xFF`.
- Sizes are raw bytes (`uPlayer` 23,056, `sMain` 115,024).

## From descriptor to class vtable

- **DTI vtable** slot 0: deleting destructor; slot 1: `newInstance`, which
  gets the class allocator (`0x00CF5E20`), allocates `size` bytes and calls
  the class constructor. Constructors may take `this` in `esi`, not `ecx`
  (link-time optimised conventions). Take conventions from disassembly.
- **getDTI** is a tiny `mov eax, <DTI>; ret` stub, found for 4,390 classes.
- **Class vtables** have `getDTI` at **slot 4**. Checked on all 4,598 vtable
  words that point at a getDTI stub: the word 16 bytes earlier is a
  deleting destructor in 4,010 cases by flag-test pattern, and the
  remainder are destructors that store their own vtable address there.
  Slot 0 is the deleting destructor.
- Result: 4,343 classes with a resolved vtable (47 ambiguous, 15 none),
  25,791 distinct virtual functions attributed to the class that
  introduces them; 43,850 labels in all.
- Applied to the project: about 16,100 functions that only vtables
  reference were created. 99 targets were not code (UTF-16 text after a
  vtable, see below) and were rejected; they are listed with reasons in
  the private `dti-labels-failures.csv`.

## Naming in the Ghidra project

`research/ghidra/ApplyDtiLabels.java` writes, per class namespace:
`DTI`, `DTI_vftable`, `DTI_newInstance`, `vftable`, `dtor_deleting`,
`getDTI`, `vfNN` (slot number), and `ctor` where the constructor was
verified to install the class vtable. Labels use SourceType.ANALYSIS, so
later manual names (USER_DEFINED) always win.

## Known issues

- The vtable length is found by walking forward while words point into
  `.text`. Data after a vtable can look like a code address: every
  resource class's vtable is directly followed by its extension string
  (`tex\0` = `0x00786574`, `mod\0`, `mrl\0`, ...), and others by ASCII,
  UTF-16 or UTF-8 text. `dti_labels.py` now trims each run at the first
  slot (past slot 4, not a getDTI stub, used by one vtable only) that is
  unaligned (48,579 of 48,633 real slots are 16-byte aligned; the other 54
  are the shared `_purecall`), or printable without a preceding function
  boundary, or a resource class's 2-3 letter extension. 143 vtables are
  trimmed; this removes all 99 rejected rows plus 57 targets that Ghidra
  *had* accepted as functions. The trailing word is kept as
  `vtable_trailing`, which also gives each resource's extension straight
  from the exe.
- **Pending (needs the Ghidra project lock):** run
  `RemoveBogusDtiFunctions.java` with the private
  `dti-bogus-functions.csv` (the 57 garbage functions, removed only if
  they still carry their ANALYSIS name), then `ApplyDtiLabels.java` with
  the regenerated `dti-labels.csv`. Not yet done because the MCP server
  holds the project.

## Not done yet

- MtPropertyList reflection (field names, types and offsets, e.g.
  `mPawnID`). This would name struct fields the way DTI names classes.
- The 47 classes with more than one candidate vtable.
- A tighter vtable end (stop at the next vtable's start or at
  non-function targets).
