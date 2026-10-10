# Stage navigation

Status: readers implemented and tested: the complete `ddnav` package passed
3 tests, including the installed beach and one field resource, with no
skips, ignores or filtered tests (source `917ce892`, 2026-10-10). Native
path construction, avoidance, host service and NPC body
adapter are still open; there is no direct-line or generic A* substitute.

Addresses refer to DDDA.exe, 2017-12-13 Steam. Native resource vtable slot 10
loads and slot 11 saves. The mesh reader is `0x01099E00 -> 0x01099100`; the
waypoint reader is `0x011FD5C0`. Slot 11's `0x01099BA0` / `0x011FC410` are
writers, not loaders.

## Resources and service selection

`rNavigationMesh`, DTI `0x018D3B78`, type hash `0x4EF19843`, supplies the beach:
`scr/st200/etc/st200_nav` in `rom/stage/stage200/stage200_nav.arc`.
Stage 100's split cells use `rAIWayPoint`, DTI `0x018D6F7C`, type hash
`0x5F36B659`. The opening field cell is `scr/st100/etc/st100h_64m53n_mrg00` in
`rom/stage/stage100/split_way/m60/n50/st100_64m53n_way.arc`.

The native Trace request dispatch `0x0059F840` first tries active
`cAISvOpenField` named `FieldWayPoint` (active byte `+0x1DD`, controller
`+0x30`), then active `cAISvNavPathFinding` named `NavigationMesh` (active
byte `+0x9C`, controller `+0x34`). The latter overwrites the result when
available. `0x0059F7F0` invokes `cAINavPathTrace::vf06` (`0x010BA660`) with
start/goal and final boolean true. `0x0059F8E0` invokes vf07
(`0x010C5620`) with the current position and waypoint reach distance.
`cAIFieldPathTrace` derives from `cAIWayPathTrace`; it is a separate controller,
not the mesh controller despite the common Trace wrapper.

## NAV version 33

Little-endian, sequential, without alignment padding:

| Section | Serialized fields |
| --- | --- |
| Header | `NAV\0`, u32 version `0x21`, u32 type, u32 node-word total |
| Name/counts | u32 name byte count, name bytes plus NUL, u32 vertex/node/node-info counts, u8 vertex-extension flag |
| Vertices | three f32 coordinates; when extension flag equals 1, u8 boolean and u16 label |
| Nodes | s32 index; u32 attribute count and words; u8 flag; vector3; f32 scalar; u8 reference count and u32 references; u32 vertex-index count and indices; u32 link count and links |
| Link | u32 destination node; two u32 edge references; three f32 values |
| Regions | inherited `rAIPathBase` groups, described below |
| Metadata | three u32 totals |
| Node info | u32 node offset and eight f32 words (padded min/max AABB) |
| Spatial index | u8 levels, u32 cell count, padded min/max AABB, cells |
| Cell | u32 entry count; entries are u32 node, u32 key |

Node memory layout differs from disk. `nodeData+0x0C` counts attributes;
`+0x38` counts extra references; `+0x40` counts vertex indices; `+0x48`
counts six-word links. Do not treat the attribute list as polygon corners.
The vector at `+0x20` is retained without assigning it a guessed meaning.
Reader validation rejects truncated counts, nonfinite values, unknown shape
encodings, references outside the resource and trailing bytes.

The beach contains 4,552 vertices and 5,708 triangle nodes, 14,247 links,
one `root` region spanning `[0,5708)`, and 5,461 spatial cells at level 7.
Its 16,881 cell entries can reference a node more than once.

`0x011FB840` reads the inherited regions: u16 group count; each group has
u16 index, counted NUL name, u32 value, u32 shape count, shapes, three u16
words (range start/end and parent), then two separately byte-counted byte
arrays. Shape 0 is min/max vector3, shape 1 is vector3 plus matrix4x4,
shape 2 is radius plus vector3. Two final u16 words follow all groups.
The two footer meanings and some region flags remain unresolved.

## WAY version 38

`WAY\0`, u32 `0x26`, u32 type, u32 attribute-word total, u32 node count,
then four u32 totals (edge count, node attributes, edge attributes,
shape attributes). A node stores u16 index, vector3 position, u16 attribute
count, u16 edge count, attribute words, edges, u8 shape selector, shape
payload, u8 shape-attribute count and attribute words. An edge stores
u16 target, u8 attribute count, attribute words and three f32 values.
Shape 0 has no payload; 1 stores four f32 words; 2 stores twenty.

The footer stores u8 grid levels and max/min vector3 (the opposite bound
order from NAV). Cell count is the sum of `4^i` for `i=0..levels`, with
the upper limit exclusive, from `0x0114D5D0` and its embedded double 4 at
`0x01443318`. Each cell has u32 count and `(u16 node,u32 key)` entries.
The field resource above contains 636 nodes, 3,202 directed edges,
85 cells at level 4, and 636 cell entries. WAY coordinates are the authored
field frame; they are not automatically beach-local coordinates.

## Trace query still to port

The normal path branch is `0x0057BCE0`; constructor `0x0057B4EA` clears
the internal A0..A3 bytes and `0x0057B880` copies only authored fields.
Do not set A3 to select the direct branch `0x0057CC00`. Beach GotoEx
records enable path use/update/avoidance and disable width variation.

Mesh vf06 resolves start/end through `0x01090670 -> cAISvNavPathFinding::vf11`
(`0x010975E0`). Result vf07 invokes task scheduler `0x011BA610` when pending,
then consumes generated trace points. Its simple point-consumption branch
returns 3 while distance squared is greater than or equal to the reach
distance squared; it advances points only at strict less-than, returning
4 after the last point. Dynamic-path and curve branches require further
source tracing. A reader alone does not satisfy the installed GotoEx gate.

`0x010C8B80` installs the mesh controller's four native task callbacks:
pre-straight examination `0x010B9570`, region search `0x010B5700` (calls
`0x0114D100`), node search `0x010B5810` (calls `0x0114B650`), and path
construction `0x010C55C0` (`0x010BA990` or width-adjusted `0x010BCE20`,
then optional curve `0x010B8210`). These phases are not interchangeable
with a generic A* over the reader's links. Constructor `0x010B9180` clears
the dword at `+0x1C4`, including pre-straight enable byte `+0x1C6`;
later owner settings must be traced rather than selecting that branch
to avoid the native search.

`0x0056DCC0` maps only trace result 4 to interface done; result 3 drives
movement and remains running. Results 0/1/2 remain running. A host adapter
must retain current NPC lifetime, command and launch token and reject old
observations after unload, graph replacement or retry. Neither elapsed
time nor request acceptance proves completion.
