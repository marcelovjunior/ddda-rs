# Remaining-system asset survey

Measured 2026-10-09 against the maintainer's Steam installation. This is
resource discovery and selected loader inspection, not implemented gameplay
or comparison against the running original. All addresses refer to the
2017-12-13 PC build in METHODOLOGY.md.

## Reproduction and evidence

`cargo run -p ddtool -- survey <nativePC> [resource class]` opens the ARC
index read-only. Without a class it prints every registered class, including
zero-count classes; with a class it also prints sorted paths and the first
sample's size, header prefix, and XFS class/property definitions if present.
It does not extract files. `ddtool xfs-get` can inspect individual XFS trees.

The scan found **8,536 archives, 358,431 archive entries and 78,003 unique
resource keys**. Counts below are **unique case-normalized paths / archive
entries**, exactly the distinction between `Vfs::paths_of_type` and the sum
of `Vfs::locate` lengths. Unique paths are not content hashes: different
copies may have different data, and VFS currently selects the first sorted
archive. This explains why these counts must not be equated to Riftstone's
distinct-content counts. Samples use that same VFS selection.

Reference: the private MIT Riftstone `formats.md` reference, sections named
below (`E:\ddda-re\refsrc\riftstone_formats.md`). Its descriptions are
leads, not verification of this install. No reference source code, raw game
data, save contents, or decompiler output is included here. The survey CLI
uses existing repo readers and no copied reference implementation.

`loader inspected` means the PC decompilation was read in this survey;
`DTI lead` means only the labelled class/address was located. Field names
from XFS are serialized names; their runtime effects remain untraced unless
explicitly stated. The ten proposed briefs below are proposals, not new
assignments.

## 1. Sound and music

| Resource class | Paths / entries | Location |
| --- | ---: | --- |
| rSoundPackage | 1,490 / 11,866 | `sound\se\...` |
| rSoundRequest | 1,601 / 9,308 | `sound\se\...` |
| rSoundStreamRequest | 2,650 / 3,851 | `sound\stream\...` |
| rSoundRandom | 368 / 3,200 | `sound\se\...` |
| rSoundMotionSe | 92 / 1,251 | `sound\se\em\...` |
| rSoundSequenceSe | 484 / 3,062 | `sound\se\...\seq\...` |
| rSoundPhysicsList | 33 / 1,243 | `sound\se\em\...`, `sound\se\om\...` |
| rSoundSubMixer | 61 / 61 | `sound\submixer\...` |

Sample `sound\se\armor\arm_cha_cha\arm_cha_cha` is 16,392 bytes
as rSoundPackage: `SPAC`, version 12, embedded RIFF headers at offsets
32 and 178. The first RIFF's `fmt` tag is 2 (MS ADPCM), mono, four bits per
sample. The rSoundRequest at the same path is 2,688 bytes, `SREQ`, version
0x13. `sound\stream\bgm\bgm` is 32,140 bytes, `STRQ`, version 0x1b;
header words at +8/+12 are 125/171.

The ARC index has zero rSoundSourceOggVorbis resources, but a separate
filesystem enumeration finds **285 loose `.sngw` files** under
`nativePC\sound\stream`. `bgm\wave\Bridge.sngw` is 454,492 bytes,
starts with `OggS`, and contains the Vorbis identification packet. Thus
an archive-only scan misses actual music/voice streams.

Loaders inspected: rSoundPackage `0x00e21af0` checks SPAC/v12;
rSoundStreamRequest `0x00dc0ef0` checks STRQ/v0x1b, relocates offsets,
and traverses 0x9c-byte request records. rSoundRequest `0x00f73f10`
is a DTI loader lead. Reference: **Sound cues**, including SPC/RIFF and
SNGW notes; the reference leaves some SPC metadata unexplained. Playback,
loop metadata and request selection have not been tested here.

Proposed **sound-resources**: new `mtsound` crate and SOUND.md. Read
SPAC/SREQ/STRQ and resolve loose stream paths without playback. Done when
all instances parse with bounded offsets, sample RIFF/Vorbis streams
validate, and request-to-source links plus unresolved metadata are reported.

## 2. Visual effects

| Resource class | Paths / entries | Location |
| --- | ---: | --- |
| rEffectList | 4,603 / 58,307 | `effect\efl\...`, also `bh6tu\effect\efl\...` |
| rEffectProvider | 656 / 7,576 | `effect\epv\...` |
| rEffect2D | 181 / 558 | `effect\efl\ev\...` and other effect folders |
| rEffectAnim | 48 / 4,059 | `effect\rtt\...` |
| rEffectStrip | 35 / 322 | `effect\efl\...` |

Sample `bh6tu\effect\efl\l_id000_17`: 29,600 bytes, EFL version
0x20110318, words/halves at +0x10/+0x12 are 55/41. The loader
`0x01147790` checks this magic/version, reads these two 16-bit values,
and uses a 16-byte stride derived from the first. `effect\epv\cm\p_cm`
is 17,049 bytes, `epv`, version zero, with embedded effect resource paths.
Provider loader DTI lead: `0x00cb6cf0`.

Reference: **Effects** and **EAN**. It describes provider motion windows,
EFL unit/resource tables and partial particle/generator fields; it explicitly
leaves substantial particle bodies unresolved. No effect rendering verified.

Proposed **effect-links**: new `mteffect` crate and EFFECTS.md; decode
provider motion-to-effect links and EFL resource references first. Done when
all provider/list instances pass bounds checks and one attack's resource
chain is resolved without claiming particle simulation.

## 3. GUI, HUD, menus and loading screens

rGUI **116 / 216**, rGUIFont **3 / 6**, rGUIIconInfo **1 / 2**, and
rGUIMessage **8,488 / 16,338**. rTexture resources are shared with many
systems (**14,159 / 73,706**); this is not a count of GUI-only textures.
Paths include `id\cockpit\npc_gauge`, `id\ddn\co_item\co_item`,
`id\ddn\load\load0_001`, `id\ddn\load\load0_002`,
`id\ddn\menu\option\top_option`, `id\ddn\p_option\option`, and
`id\ddn\minimap\mmap_icon`.

Sample `dl1\id\ddn\barber\barber1`: 226,320 bytes, `GUI`, version
0x00021209, declared size 226,320 at +8. Loader inspected at `0x01075780`
validates the magic/version and invokes another virtual method after reading
the memory image. `id\common\font\ddicon` is 4,372 bytes, `GII`,
version 0x10000, count-like header word 331 at +8 (meaning untraced).

Reference: **GMD** covers text; no GUI widget grammar was found in the
available reference files. The text brief supplies `mtgmd` and TEXT.md.
Visual hierarchy, animation and controller behaviour remain untraced.

Proposed **gui-structure**: new `mtgui` and GUI.md, parse GUI section and
resource tables. Done when every GUI parses, texture/message links resolve,
and the loading-screen and option-menu node trees are documented. Rendering
and input stay outside this reader brief.

## 4. Items and inventory

| Resource class | Paths / entries | Location |
| --- | ---: | --- |
| rItemList | 1 / 1 | `etc\item\itemlist` |
| rItemLevelParam | 3 / 3 | `etc\item\lvparam{wepon,armor,accessory}` |
| rItemSetTbl | 2 / 2 | `etc\item\itemsettbl`, `itememlistsettbl` |
| rItemMix | 1 / 1 | `etc\item\itemmix` |
| rItemRandParam | 1 / 1 | `etc\item\itemrandparam` |
| rItemCurseCnv | 1 / 1 | `etc\item\itemcurse` |
| rShopList | 18 / 18 | `etc\shop\...`, `dl1\etc\shop\...` |
| rArmorTable | 2 / 2 | `model\pl\parts\...` |
| rArmorModel | 3 / 3 | `model\pl\parts\...` |

ItemList: 243,344 bytes, ITL2, stamp 0x01330611, **1,901 records of 128
bytes after a 16-byte header**. Independently checked on every record:
the low 13 bits of the word at record+0x3c equal the record index.
Loader inspected `0x00cbfb30` confirms magic/stamp and count times 128.
sItemManager `0x0045cf60` is a DTI lead for consuming the tables.

The accessory enhancement sample is 18,588 bytes, `ite`, version 0x1400,
43 rows; `(18588 - 12) / 43 = 432` bytes per row. Shop sample
`dl1\etc\shop\n156shoplist` is XFS with 2,416 objects, containing
`mShopLineup`, item ids/counts/rearrival fields and AND/OR judgments.
Item names/descriptions live in `id\message\item\itemname_<lang>` /
`iteminfo_<lang>` per the text brief; icon rendering requires GUI/texture
mapping rather than assuming rGUIIconInfo indexes inventory items.

Reference: **ITL**, **Enhancement levels**, **IST / IMX** has detailed
record/bitfield layouts and example exe traces. This survey verifies the
container and id relationship, not every stat bitfield or inventory rule.

Proposed **item-data**: new `dditem`, ITEMS.md, optional isolated CLI
module. Done when all listed item/recipe/drop/enhancement resources parse,
ids join to game text, and enhancement field meanings distinguish local
exe traces from reference-only claims. Runtime inventory is a later core task.

## 5. Quests, events and cutscenes

| Resource class | Paths / entries | Location |
| --- | ---: | --- |
| rQuestCtrlTbl | 325 / 338 | `etc\questctrl\...` |
| rQuestInfinity | 1 / 1 | `etc\infquest\infquest` |
| rQuestReward | 1 / 1 | `etc\questreward\questreward` |
| rQuestMarker | 22 / 22 | `scr\stNNN\etc\stNNN` |
| rQuestSudden | 1 / 1 | `scr\st100\etc\st100` |
| rScenario | 216 / 216 | `scr\stNNN\etc\...` |
| rScheduler | 806 / 1,253 | `effect\sdl\...`, event/stage schedules |
| rCameraList | 109 / 195 | `event\stNNN\evNN\list\camera\...` |

Sample `etc\questctrl\n001questctrl`: 1,387 bytes, `qct`, version
0x10; +8 is 1,888 (not file length), +12 is 4. Loader inspected
`0x00cd7200` uses +8 for allocation and then reads nested condition/result
tables; do not treat the allocation size as serialized length. Quest reward
XFS has 133 objects with quest number, gold, experience, job/rift points,
three item/count pairs, quest flag and NPC friendship fields.

`effect\sdl\id\s_id001_02` is 1,023 bytes, SDL version 19.
`event\st100\ev05\list\camera\camera` is 124,760 bytes, LCM
version 3. Scheduler loader DTI lead `0x00db8e50`; scenario loader
`0x00cdc0e0`; sQuestManager constructor `0x00482170`.
The loose `nativePC\movie` directory contains six `.wmv` files and one
`.rtex`; ARC rMovie variants all have zero entries.

Reference: **LCM**, **Faces, conversations, schedules, zones** covers
camera frame streams and scheduler tracks. QCT execution remains a separate
trace; general event tracks do not by themselves prove NPC daily schedules.

Proposed **quest-data**: new `ddquest`, QUESTS.md. Parse QCT plus typed
reward/marker/scenario XFS and identify one opening quest's condition/result
ids. Done when all resources parse and that quest's data links resolve;
condition evaluation must be traced before implementation.

## 6. NPC placement, schedules and dialogue

rLayout **6,342 / 7,524** includes NPC `_nNN` files as well as scenery,
enemies and other layouts; not all are NPC records. rNpcLedgerList **1 / 1**
is `etc\item\npclist`; rMsgSet **930 / 1,855**, rMsgSerial **783 / 1,220**;
rTalkMotion **1 / 1** at `etc\talk\talk_mot`; rFacialAnimation
**10,190 / 21,939**. Message serial paths include
`id\npc_wind\npc_base_talk\n001`; NPC layouts include
`scr\st100\etc\st100_00m00n_n01`.

Ledger sample: 29,141 bytes, `nnl`, version 8, **727 entries** at +8.
Loader inspected `0x00ccac90` checks nnl/v8, allocates count times 100
runtime bytes and reads smaller serialized fields; 100 is not a file stride.
The existing `mtlot` reader identifies kind 47 as cSetInfoNpc and exposes
`mNpcId`, priority and transform fields. It cites layout loader `0x00cc1790`.
NPC manager DTI lead `0x004753c0`.

Message serial sample `n001` is 286 bytes: `msl`, version 0x10, count 137,
then 137 u16 values. First message-set path `id\message\pawn\e_base`
is an empty 12-byte mss/v0x20 file, so it cannot demonstrate conversations.
Message-set loader DTI lead `0x00cc95f0`. Dialogue text uses rGUIMessage;
joining serial numbers, conversation rows and actual text requires tracing.
No NPC daily-schedule format is claimed identified in this survey.

Reference: **LOT** and **Faces, conversations, schedules, zones** describes
placement, MSS choices/jumps and SDL tracks; it leaves serial semantics and
parts of dialogue conditions unknown.

Proposed **npc-dialogue-links**: new `ddnpc`, NPCS.md, consume existing
mtlot/mtgmd and add ledger/MSS/MSL readers. Done when all three new formats
parse and one NPC's placement-to-dialogue links are verified; trace the
manager's schedule selection separately from generic SDL playback.

## 7. Day/night and weather

rWeatherEffectParam **64 / 64**, rWeatherFogParam **55 / 55**, rSky
**2 / 30**, rStarCatalog **1 / 14** (`scr\sky\ddstar`). rCloud has zero
ARC entries. Paths include `effect\wep\...`,
`scr\st100\etc\st100_fog_e`, and `scr\sky\st_all_sky_f`.

WEP sample `effect\wep\w_cm_c`: 2,144 bytes, wep/v1, six counts of
eight at +8, followed by **48 rows of 44 bytes**. Loader inspected
`0x00cea2b0` reads six counts and eleven four-byte fields per row into
48-byte runtime objects. Fog sample: 480 bytes, wfp/v1, 13 rows of 36
bytes after the 12-byte header. Loader `0x00ceaa80` reads two integers
then seven floats; its object-field order differs from file order.
Sky sample: 249 bytes, SKY-space/v6. Sky loader DTI lead `0x00e2c350`;
sWeatherManager constructor `0x004bb1c0`, update lead `0x004c0bf0`.

Reference: **Weather, fog and sky** describes hour/minute keyed lighting
and fog and the sky model. Mapping the six correction lists to material
lighting and interpolation/time progression remains untraced here. These
data are suitable leads for the terrain-shading lane, which was notified.

Proposed **weather-data**: new `mtweather`, WEATHER.md. Done when every
WEP/WFP/SKY instance parses and time-key ranges/record bounds are checked;
then trace list consumers and clock interpolation before core integration.

## 8. World map and fast travel

rMapLndMarkPos **50 / 3,251**, rStagePlaceName **23 / 23**, rStartPos
**126 / 126**; GUI paths include `id\ddn\p_map\p_map` and minimap icon
resources listed above. Stage-local map paths use `scr\stNNN\etc\stNNN`.

Map landmark sample `scr\st100\etc\st100` is XFS, 61,817 bytes,
1,135 objects. `rMapLndMarkPos::Info` exposes Pos, Type, NameID, NextStage,
NextIconIndex, Floor, Lock Flag, Group, ID, Usage and judgment commands.
These names identify links but do not prove which icon types represent
portcrystals. StartPos sample `scr\st000\etc\st000` is 461-byte XFS
with three objects and Pos/Ang/OfsPos/OfsAng. PlaceName's st100 sample is
1,872 bytes, spn/v0x30, header count 62.

DTI leads: rMapLndMarkPos constructor `0x00cc8040`,
cMapLandMarkManager `0x005e2ec0`, uGUIMap `0x0067fc70`.
The item table and game text are the starting points for identifying the
travel items. Text-brief inspection found Ferrystone at message indices
66 and 272, Portcrystal at 150 and 274, and Eternal Ferrystone at 1724 in
`id\message\item\itemName_eng`. These are name-table indices; duplicate
names prevent assuming a single gameplay id. Teleport rules, destination persistence and item consumption
remain untraced. Do not equate every landmark or start position with a
fast-travel destination.

Reference: **ITL**, **LOT**, and the XFS grammar supply pieces; no complete
fast-travel execution specification was found in the available reference.

Proposed **map-travel-links**: new `ddmap`, MAP.md, typed landmark/start/name
readers and an exe trace of the travel-item consumer. Done when all map
resources parse, text ids resolve and a destination-selection/consumption
path is explained with addresses. Core teleport implementation is later.

## 9. Saves

No save resource class occurs in the ARC registry; this is a loose file.
Read-only inspection found **one DDDA.sav**, 524,288 bytes, under
`<Steam>\userdata\<account>\367500\remote\DDDA.sav`, and eight
8,192-byte files named `0` through `7`. Their purpose is not established.
Account ids and save values are intentionally absent from this report.

Observed sample structure: 32-byte header, then a zlib stream at +32,
then zero padding. Header u32 at +0 is 21; +4 equals the decompressed
length (**20,362,849**), +8 equals compressed bytes consumed (**338,760**).
The XML parses successfully with root class type `sSave::saveDataAllDA`,
name `dd_savedata1018`. Direct structural members include player manual/base
data, system data and achievement data. This is a sample observation, not
a guarantee of all save versions. Checksums and the other header words
remain unknown; no serializer or save write was attempted.

DTI leads: sSave constructor `0x0048cf00`,
sSave::saveDataAllDA constructor `0x00496a90`,
nSaveStream::DecompressStreamZLib `0x00410090` (constructor) /
`0x00410140` (virtual method). No save specification was found in the
available Riftstone format reference; the XML/zlib relationship above was
measured directly in memory without exporting a save or its XML.

Proposed **save-readonly**: new `ddsave`, SAVE.md, an explicitly read-only
header/decompression/structural reader. Done when a private original save
passes bounded inflate/XML parsing, header checks are traced in sSave and
synthetic corrupt-input tests pass. Never write an original save or include
one in tests; import/export policy belongs to a separate decision.

## 10. Status effects

rStatusParam **2 / 2**: `param\status\enemy` and `param\status\player`.
Enemy sample is 2,460 bytes of XFS, 41 objects: a root rStatusParam plus
40 cStatusParam records. Fields are mCategory (u32), mIsOverwrite (bool),
mOverwriteNum (u32), mTimer/mParam0/mParam1 (floats). These are observed
serialized names, not proof of timer units or stacking rules.

DTI leads: rStatusParam constructor `0x00ce8af0`, cStatusParam
`0x00ce88f0`; likely runtime consumers cStatus `0x00836980`,
cEffectStatusManager `0x0080c300`, uCharacterBase::StatusEffect
`0x00853780`. These candidates were identified by class labels; this
survey does not claim a traced call path from the resource to them.

Reference: **XFS**, **ITL secondary item descriptions**, and **PRP** locate
related status/resistance data, but do not establish the runtime timebase or
stacking policy. The enemy-data brief owns the typed status reader and
ENEMIES.md; this overlap was agreed with codex-4 on the board.

Proposed **status-rules**: STATUS.md and a new headless rules crate only
after head assigns ownership. Consume the enemy-data reader, trace category
dispatch, timers and overwrite policy. Done when each implemented transition
has a source address and synthetic regression test; original-game comparison
requires a separate authorized recording session.
