# The MT Framework core library (readable ports)

`crates/mtcore` holds readable Rust for the most-called functions of the
engine's core library in DDDA.exe: run-time type descriptors (MtDTI), the
string hash behind their ids, MtObject, MtArray, MtPropertyList and the
allocator registry. Like the math library (docs/MATH.md, whose wiring and
verification this crate shares), each function is bit for bit the
original's, verified against it, and the lifted program calls it instead of
the lifted body. Brief and results:
[tasks/L33-readable-containers.md](tasks/L33-readable-containers.md).

## What the exe contains

Most container and string code is inline: MtArray's push and index,
MtString's reference counting and MtObject's `isKindOf` walks are expanded
into their callers. What is out of line, ranked by the number of functions calling them
(inventory call graph):

| address | callers | function | state |
|---|---:|---|---|
| 0x00CF5E20 | 10,510 | `MtDTI::allocator`: a class's allocator object | ported |
| 0x004027A0 | 1,029 | `MtPropertyList::add`: every `getProperties` uses it | ported |
| 0x004038F0 | 974 | `MtObject::~MtObject` body: back to MtObject's vtable | ported |
| 0x00402530 | 404 | `MtArray::~MtArray` body | ported |
| 0x00402620 | 367 | `MtArray::reserve` | ported |
| 0x004025C0 | 293 | `MtArray::clear` | ported |
| 0x00CF7340, 0x00CF76C0 | 131, 131 | `MtPropertyList` constructor and destructor body | ported |
| 0x00CF55B0 | 25 | `MtDTI::find(id)` | ported |
| 0x00CF5C00 | 25 | `MtObject::operator new` | ported |
| 0x00D01B60 | 24 | CRC-32 of a C string (DTI ids, names) | ported |
| 0x00CF5870, 0x00CF55E0 | 15, 2 | `MtDTI::find(name)` and the subtree search | ported |
| 0x00CF5450, 0x00CF54A0 | 9, 4 | allocator registry add, remove | ported |
| 0x00CF74E0 | 4 | `MtPropertyList::clear` | ported |
| 0x00CF5750, 0x00CF5E40 | 4, 1 | allocator index set and propagated to subclasses | ported |
| 0x00DE0B80, 0x00DE0B90, 0x00CF73F0 | 438, 1,326, 526 | the MT allocator's allocate/free, the node pool | platform models (not ported) |
| class `getDTI` stubs | 4,390 functions | `mov eax, DTI; ret` | lifted (one per class, trivial) |

## Types and layouts

All `#[repr(C)]`; guest objects are reached through small handles over
guest memory (`dti::Dti(addr)`, `array::Array(addr)`), whose accessors use
the layouts' offsets.

| Rust | original | size | layout |
|---|---|---:|---|
| `dti::DtiLayout` | MtDTI | 0x20 | vtable, name, next sibling, first child, parent, hash chain, size+flags (+0x18), id (+0x1C) |
| `array::ArrayLayout` | MtArray | 0x14 | vtable, count, capacity, auto-delete flag (+0x0C), data (+0x10) |
| `property::ListLayout` | MtPropertyList | 8 | vtable, head node |
| `property::NodeLayout` | MtProperty node | 0x28 | the property (0x20), prev (+0x20), next (+0x24) |

Globals the functions use (addresses in the exe, values at run time):

| address | what |
|---|---|
| 0x01876628 | 64 allocator objects; a descriptor's bits 23..28 pick one |
| 0x018768A0 | 256 descriptor buckets by `id & 0xFF`, chained through +0x14 |
| 0x01876730, 0x01876834 | the allocator registry and its length; an allocator's slot at +0x58 |
| 0x01877D24 | MtArray's descriptor (its allocator holds element storage) |
| 0x01877DA0, 0x018C7DA8 | the property node pool's free list and its critical section |
| 0x01558134, 0x01558298, 0x01406E74 | MtObject's, MtArray's and MtPropertyList's vtables |

Conventions:

- An allocator object's vtable allocates at +0x20 `(size, alignment)` and
  frees at +0x24 `(block)`; MtObject's `operator new` uses entry 1 of the
  allocator table, 16-byte aligned.
- A DTI id is `crc(name, 0xFFFFFFFF) & 0x7FFFFFFF` with the reflected
  CRC-32 table (`hash::crc_bytes`), no final inversion.
- Name lookups compare names byte for byte (case-sensitive), depth first.
- MtArray holds pointers; with the auto-delete flag it deletes its
  elements through their virtual deleting destructor (slot 0, argument 1)
  when cleared or destroyed.

## Wiring and verification

The same as docs/MATH.md ("Wiring", "Verification"): each port has an
adapter in `crates/mtcore/src/abi.rs`, listed in `PORTS`, which
`research/native/lift.py gen` imports over the lifted body. Adapters make
every call the original makes the same way: virtual calls and the
critical-section imports through `x86::call_ext`, the platform models (node
pool, memset, memcpy) directly, other ports as machine calls with the
original's return addresses. Recursive functions (the subtree search, the
allocator propagation) take their recursion as a callback, so the adapter
recurses on the machine while the readable function stays the one that is
tested.

`crates/mtcore/tests/differential.rs` replays the lifted program's vectors
for every ported address and edge cases recorded from the original by
`research/native/vectors_core.py`: built descriptor trees with names and
hash chains, arrays with null elements and stubbed destructors, property
lists and the node pool, the registry, strings of many lengths. Virtual
callees and imports are recorded at the call and replayed, the platform
models run as in the recorder, globals are recorded as memory windows.
The CRC table is checked against the exe's (0x014075C0).

## Changing behaviour (mods)

As for the math library: the lifted program calls these functions, so a
change here changes the game, and the differential tests will report it.
Keep each adapter's register and memory contract, and keep such changes
behind a switch with the original as the default.

## Not ported, and why

- The platform models (allocator entry points, node pool, memset/memcpy)
  are replaced by the runtime, not translated.
- The per-class `getDTI` stubs and vtable-setting constructors: trivial,
  one per class (thousands); wiring them one by one costs more than it
  gives.
- MtString, MtArray's push/index and `isKindOf` have no out-of-line body to
  port; they are part of their callers.
