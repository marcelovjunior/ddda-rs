# Map resources and travel-item links

Implemented: headless `ddmap` readers for landmarks, start positions and
stage place names, with caller-selected GMD name lookup. Tested against
the read-only installed corpus. Travel selection and consumption below
are executable traces, not implemented simulation. No running-game
comparison or process writes were performed.

All addresses refer to the 2017-12-13 Steam DDDA executable identified in
METHODOLOGY.md. Private decompilations and game resources are not included.

## Resources

| Resource | Format | Distinct paths | Archive copies | Records across copies |
|---|---|---:|---:|---:|
| rMapLndMarkPos | XFS | 50 | 3,251 | 51,323 |
| rStartPos | XFS | 126 | 126 | 506 |
| rStagePlaceName | SPN v0x30 | 23 | 23 | 208 |

All 3,400 copies parse. The integration test opens every archive location,
not only the winning VFS resource. It also validates all non-sentinel
name IDs against localized map message tables. Set `DDDA_NATIVE_PC` to a
different install's nativePC root, or use the maintainer's default path:

```powershell
cargo test -p ddmap -- --nocapture
```

The install test skips if the default installation is absent; an explicitly
configured missing installation fails. Two synthetic SPN tests cover field
order, signed fields, all truncated prefixes, version/count and trailing
bytes. The corpus regression also rejects every truncated prefix of the
st000 start-position resource and its use as a landmark resource.

Landmarks retain the serialized `mQuality`, `mStageNo`, `InfoList` and
every entry's `mPosType`, `Pos`, `Type`, `NameID`, `NextStage`,
`NextIconIndex`, `Floor`, `Lock Flag`, `Group`, `ID`, `Usage`, and judgment
commands with all four parameters. The nested object classes and property
types are checked. Reflection `0x00cc87a0` corroborates the entry layout:
position +0x10, NameID +0x20, NextStage +0x24, NextIconIndex +0x28,
Floor +0x2c, Lock Flag +0x30, Usage +0x34, judgments +0x38.
Judgment execution and icon filtering are not implemented.

Start entries retain `Pos`, `Ang`, three `OfsPos` vectors and three
`OfsAng` floats. Reflection `0x00ce8740` declares both arrays with count
three. Each XFS vector has four storage lanes, all preserved. These are
serialized offsets, **not a simulation player limit**. The st000 first
entry's offsets are (0,0,2), (1,0,1), (2,0,0), independently checked by
the integration regression. Their complete placement lifecycle remains
outside this reader.

SPN loader `0x00ce7e80` reads a 12-byte header (`spn\0`, version 0x30,
u32 count), then exactly 30 bytes per entry: XYZ f32, radius f32,
message number u16, check number i16, game-state flag i16, angle f32,
range f32. The last two floats are unaligned on disk. Reflection
`0x00ce7ba0` supplies names and signed types. Runtime records are larger
than disk rows; their size must not be used as the file stride.

## Names and landmark presentation

`0x005e3030` loads `scr/stNNN/etc/stNNN` landmarks for the stage table
at `0x01530348`, and loads the selected-language
`id/DDN/message/common/map_placelist_<language>` resource into the
landmark manager at +0x1814. `uGUIMap::vf58` (`0x00682560`) also loads
the localized map_placelist, pause_map and quest-name message resources.

`0x005e3180` filters landmark entries by their judgments, Usage, floor
and lock-state checks, then `0x005e3350` populates the manager's compact
icon array. `0x00684c50` reads its name ID and calls the bounded GMD
index lookup `0x00dde5f0`; there is no label-string search in this path.
`Landmark::name` and `PlaceName::name` expose the corresponding direct
index lookup on a GMD table supplied by the caller. The SPN message-number
join is verified against the resource corpus; its full HUD consumer is
not independently traced here.

There are 207 distinct non-sentinel IDs across the landmark and SPN
corpus; all resolve in each of the seven language tables, and the English
map table contains 277 messages. Landmark resources also contain 5,733
occurrences of `0xffffffff` IDs. These remain explicit missing-name values:
lookup returns `None`, including any other out-of-range ID. Empty GMD
messages remain `Some("")`. No fallback text is invented.

## Ferrystone destination selection

The following is a trace for later core work, not a public teleport API.
Original globals describe the original engine only; a future co-op request
must carry an explicit actor and destination, with host validation.

1. Map selection confirmation starts at `0x00691580`. This code was
   inspected as disassembly from the analysis copy because canonical Ghidra
   has no function defined at that address. At `0x0069162c` it reads the
   selected row from the menu object (+0x28c, then +0x9c). Rows below the
   built-in count (+0x8ac) keep that index.
2. For subsequent rows, `0x0068ff00` scans ten saved occupied-slot words
   at `sGameSys +0xbe378`, returning the slot for the selected occupied
   row. Confirmation adds three. The queued destination is therefore a
   built-in row or `3 + placed_portcrystal_slot`, not an arbitrary landmark
   or start-position index.
3. At `0x0069164d` confirmation writes that selector to actor +0x3e0c,
   the selected inventory entry to +0x3e04, use mode zero to +0x3e08,
   and an action parameter to +0x3e10. `0x00b77be0` waits for readiness,
   then copies these into active use fields +0x3e00, +0x3df4 and +0x3df8.
   Item category `(record[0x38] >> 15) & 15` dispatches the action.
4. `cPlActThrowUpItem::vf05` (`0x00b06830`) retains the active inventory
   entry. Its update `0x00b06c30` recognizes ordinary Ferrystone item 66
   and Eternal Ferrystone 1724 in the throwing branch. After the motion
   gates and its accumulated delay it calls the generic consumption
   routine described below. The separate call to `0x00401160` remaps the
   selector; its downstream role is not sufficiently traced to call it
   the actual teleport operation.

The three built-in map rows at `0x017f15c4` associate landmark/text IDs
with Gran Soren, Cassardis and Bitterblack Isle. The initialized triples
are (27,27,0), (132,25,3), (243,243,9); the first two words are respectively
the landmark match and displayed map-message ID in the inspected map paths.
`0x00692180` locates matching NameID entries in st100 landmarks and obtains
their positions; `0x0068fb70` fills displayed destination names. Placed
slots obtain positions through `0x0044d0d0`, which reads a float4 at
`sGameSys +0xbe3a0 + slot*16`. The built-in availability gates and complete
stage transition/arrival placement are unresolved; no runtime behavior is
inferred solely from landmark `NextStage` or `NextIconIndex`.

## Portcrystal placement and pickup

Item 150 has use category 4 and object kind `0x29d1`. This is distinct
from item 274 despite both resolving to the English name Portcrystal:
274 has category 0. Likewise Ferrystone entries 66 and 272 must not be
collapsed solely because their text matches. The reader does not own or
duplicate the item-table schema; these individual fields were checked
against the local ITL2 records, whose consumer is described here.

`cPlActUseAnchor::vf05` (`0x00b12a70`) retains the inventory entry,
checks occupied-slot count through `0x0044d170`, and refuses when ten
slots are occupied. It snapshots actor `mPos` (+0x40), subtracting the
current world-origin offset when active.

Its update `0x00b12c20` starts motion 0x277. Once the motion value exceeds
10, it finds the first empty slot among ten, stores `sGameSys.mStageNo`
(+0x34) in the slot word, and calls `0x0044d140` to store the float4
position. Only a successful slot reservation invokes `0x00b77130` to
consume the item. It then creates object 0x29d1, assigns inventory item
0x96 (150), and puts the slot index in the object's +0x29a5 byte. These
are original-engine limits; they do not constrain the number of players.

`cPlActPickUpAnchor::vf06` (`0x00b12fd0`) waits for its motion threshold,
calls item notification/add paths `0x00452930` and `0x0045e680` with
quantity one, then clears the selected saved slot and position and
requests object removal. Inventory-capacity/error handling inside the add
routine has not been traced here. World-origin ownership and replication
also remain later core work.

## Inventory consumption

Both the completed Ferrystone throw and successful Portcrystal placement
call `0x00b77130(actor, inventory_entry, 0, 1, 0, 0)`. For the branch with
item-record +0x58 bit 13 clear, it calls `0x0045f480(item_id, 1, 0)`
**except for item ID 0x6bc (1724, Eternal Ferrystone)**. The inspected
records for ordinary Ferrystone, Portcrystal and Eternal Ferrystone all
have that bit clear, so the explicit Eternal exception is decisive.

`0x0045f480` finds the inventory entry and calls `0x0045f4f0`, then
`0x004588c0`. This updates inventory weight and calls `0x00459880`, which
subtracts quantity from the signed count at entry +0x12 with a zero clamp,
updates auxiliary stack buckets when applicable, and reports whether any
remain. Zero-count entries follow the removal path. This traces one-item
consumption; no inventory or travel simulation is implemented by ddmap.

Next action: trace the consumer of the queued destination through stage
transition and arrival placement, including restrictions and failure paths,
then validate ordinary versus Eternal Ferrystone and Portcrystal pickup
with an authorized original-game observation before host-authoritative
implementation.
