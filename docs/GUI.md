# GUI structure

`mtgui` reads `rGUI` version `0x00021209` without rendering, input, or
runtime globals. All addresses below are the 2017-12-13 Steam executable.

## Evidence and verification

- `0x01075780` (`rGUI::vf10`): magic/version validation and image loading.
- `0x01076170` (`rGUI::vf15`): section relocation, record strides, property
  pools, texture/font/message/sub-GUI resource loading.
- `0x01075c20`: reflection-dependent font filters. Their layouts come from
  `0x01153810` (shadow), `0x01153a60` (border), and `0x01153de0`
  (gradation overlay, whose resource property supplies another texture).
- `0x00e0b840` (`uGUI::vf25`): construct instances by DTI class, assign IDs,
  install the root/child/sibling links and property spans.
- `0x010777a0`: visit children along their sibling chain and assign parents.
  `0x01077500` searches this hierarchy by instance ID.

Implemented and tested, **not compared against an original-game render**:
116 paths / **216 archive occurrences**, **8,091 instance nodes**, and
**4,418 non-null resource references** parse and resolve through the player's
VFS with zero missing resources. Every archive copy is checked, not just the
first path match. All serialized message-resource tables have count zero.
Seven synthetic regression tests cover bounds, truncation, malformed pointers,
cycles/duplicate ownership/unreachable nodes, sentinel resources, filter
textures, pool types, unknown property types and animation-name joins.
Two opt-in installation tests verify all GUI occurrences and the options
menu's executable-loaded messages (7 paths, 29 occurrences, 8,850 messages).

```powershell
cargo test -p mtgui
$env:DDDA_NATIVE_PC='E:/SteamLibrary/steamapps/common/DDDA/nativePC'
cargo test -p mtgui --test install -- --ignored --nocapture
cargo run -p mtgui --example inspect -- $env:DDDA_NATIVE_PC 'id/ddn/load/load0_001'
cargo run -p mtgui --example inspect -- $env:DDDA_NATIVE_PC 'id/ddn/menu/option/top_option'
```

The example prints the complete instance tree, local animation names and
resource references. It writes nothing. The crate only has development
dependencies on `ddvfs`, `mtarc`, and `mtgmd`; its reader is standalone.

## Memory image and tables

All words are little-endian. Header size is `0x110`. `+0` is `GUI\0`,
`+4` version, `+8` image size. `+0x90/+0x94` are virtual-screen width/height.
`+0x24` is the root instance index. Table pointers at `+0x9c..+0x104` and
`+0x10c` are image-relative offsets, not native pointers. The reader retains
raw section bytes; fixed-size sections are checked against the next distinct
section offset. Empty tables commonly share offsets. Pools may include padding.

| Offset field | Count field | Stride | Meaning / evidence limit |
|---|---|---:|---|
| 9c | 28 | 24 | Named animation definitions; ID at +0, name string offset +16 |
| a0 | 2c | 12 | Animation sequence records; string relocation at +8 |
| a4 | 30 | 32 | Animation object definitions; reflected class +16, property index +20 |
| a8 | 34 | 16 | Animation subrecords, property/track references |
| ac | 38 | 12 | Named constant properties |
| b0 | 3c | 20 | Animated property tracks; interpolation not implemented |
| b4 | 44 | 32 | Runtime instance node definitions |
| b8 | 48 | 16 | Tagged sequence/control records |
| bc | 4c | 36 | Animation playback records |
| c0 / c4 / c8 | 50 / 54 / 58 | 8 | Sequence/control references; semantics retained raw |
| cc | 5c | 16 | Sequence action records; string relocation at +12 |
| d0 | 60 | 4 | Sequence-index list |
| d4 | 64 | 20 | Sequence-control records |
| d8 | 68 | 12 | Reflected runtime variables (ID, class hash, initial word) |
| dc | 6c | 48 | Texture/atlas entries; resource path string offset +40 |
| e0 | 70 | 12 | Font entries; path +8 |
| e4 | 74 | variable | Reflected font-filter definitions |
| e8 | 78 | 12 | Message-resource entries; path +8 (empty throughout install) |
| ec | 7c | 12 | Referenced GUI resources; path +8 |
| f0 | — | bytes | NUL-terminated string pool |
| f4 | 40 | 8 | Key records; kind 8 relocates into extra pool |
| f8 / fc / 100 | — | bytes | Boolean / word / vector value pools |
| 104 | — | bytes | Extra object data; not interpreted |
| 10c | 84 (bytes) | bytes | Auxiliary data, retained raw |

Header `+0x98` indexes the b8 table and `+0x108` indexes the b0 table;
these are **not offsets**. The reader exposes sections but does not yet
interpret every nested animation/sequence reference or validate every
animation track's span. Such records remain raw, not a claimed animation VM.

Instance record: `+0` ID, `+4` unknown, `+8` next sibling index, `+12` first
child index, `+16` property count, `+20` DTI class hash, `+24` property index,
`+28` extra-data offset. `0xffffffff` is the absent child/sibling/extra
sentinel. The synthetic runtime parent is outside the file's node array.
The reader checks that every node is reachable exactly once without recursion.

Property record: first word preserves kind/flags, second is a name offset
relative to f0, third is a byte offset into the kind's value pool. Kinds
1/2/7..14/16 use fc, 3 uses f8, 4 uses 100. Kind 6 uses an fc word that
points into f0. Other kinds remain explicit raw values. Scalar words retain
bits, so callers decide whether a value is an integer, float, or resource ID.

**Animation names are not instance names.** Resolve a node's `ResourceId`
and `AnimationId` properties, then the animation definition's ID. An instance
ID coinciding with an animation ID does not establish a name. The helper
`local_animation` only joins `ResourceId == 0xffffffff` (the current GUI),
following the animation-ID setter at `0x0107a8f0`.
Other resource IDs require the matching sub-GUI; there is no invented name.

## Resource and text links

The resource table parser exposes class, ID, path and source-table offset.
Texture records contain atlas information as well as the path, so several
IDs legitimately refer to the same texture. Null paths are `0xffffffff`.
Font-filter textures must also be counted; omitting e4 misses real dependencies.

The executable localizes a resource path through `0x00de13a0` and applies
texture-name substitution through `0x00de11c0`, then falls back to the stored
path if needed. The scan checks **stored paths**, all of which exist. It does
not claim to reproduce runtime language/platform/override selection or VFS
archive precedence.

Zero e8 records does not mean the screens have no text. `uGUIOption::vf58`
(`0x006a6160`) loads both `id\DDN\menu\option\top_option` and
`id\DDN\p_option\p_option`, then separately loads the message resource
`id\DDN\message\pause\pause_option\pause_option_<language>`.
`0x00607610` chooses the suffix; its strings at `0x0155d2b4..0x0155d2cc`
are jpn/ger/fre/spa/ita/zht/eng. All seven files and all 29 archive copies
parse with `mtgmd`. These are executable links, not serialized GUI links.
The exact per-control message indices and runtime formatting are not decoded.

`uGUILoading::vf05` (`0x0067cd60`) obtains a message-resource pointer from
`sGameSys +0xbe344`, and calls `0x006065e0`. The update at `0x0067cf00`
also selects message IDs (including 0x54/0x55/0x56 in particular branches).
`0x006065e0` retrieves text through `0x00dde5f0` and supplies it to
`0x010fee40`; it is runtime text assignment, outside the empty GUI message
table. The original loader of the +0xbe344 resource and the mapping of all
loading-tip choices remain unresolved. No guessed message-file join is made.

## Loading-screen node trees

`0x0067cd60` explicitly chooses these two resources based on
`sGameSys +0x48 == 1`. Both use a 1280 x 720 virtual screen.

`id\DDN\load\load0_001` (3 instances, 2 animation definitions):

```text
instance ID 4: cGUIInstNull [root]
  ID 9: cGUIInstAnimVariable -> local animation 11, Text_01
  ID 8: cGUIInstAnimation    -> local animation 12, Pic
```

The order is stored child/sibling order, not a guessed draw order. The root's
PosX/PosY are 640/360; ID 9 has priority 20 and ID 8 priority 10. The picture
references `id\DDN\load\load01_ID`; the font is
`id\common\font\ddfont_jpn`. Its gradation filter references
`id\common\font\font_gra0_ID`. Message content is runtime-supplied as above.

`id\DDN\load\load0_002` (2 instances, 1 animation definition):

```text
instance ID 4: cGUIInstNull [root]
  ID 8: cGUIInstAnimation -> Pic
```

This picture uses `id\DDN\load\load03_ID`; a font resource is still present.
`0x0067cd60` also starts scheduler `effect\sdl\id\s_id001_02` or
`s_id001_03` respectively. The GUI file alone is not the complete loading
presentation.

## Options-menu node tree

`id\DDN\menu\option\top_option`: 1280 x 720; 170 instances, 40 named
animation definitions, 119 animation objects. Root node array index 4 has
instance ID 20; its animation belongs to a referenced GUI, so the local-name
helper deliberately leaves it unnamed. Below is an annotated structural
summary; names after arrows are resolved **local animations**, numbers are
instance IDs. Repeated null holders are summarized instead of dumping all
170 rows; the `inspect` example reproduces the full hierarchy.

```text
20 Animation [root]
  72 AnimControl -> title; 74/134 AnimControl [external]
  204 AnimVariable -> scroll_bar; 266 AnimVariable -> scroll_bar_pad
  246 Null
    413 Animation -> mask
      248 Null -> 15 null holders
      380 Null
        381 Null -> 382/383 Null, 384 Null -> 385/386 Message
        387 Null -> 388 AnimVariable(c_scale0), 389 AnimControl(cursor0)
        396 Animation(bright), 395 AnimControl(bright_BG)
  22 Null
    7 Animation -> controller
      11 Animation children: control_list0/1/0_l/1_l/2/3/3_l/4/5/6/6_l
    92 Animation -> controller360
      11 Animation children: control_list0/0_l/0/0_l/2/3/7/8/9/9_l/10
    209 Animation -> brightness
    210 Null -> 11 null holders; 222 Null -> 11 null holders
    376 Animation(mask_x) -> 377 Null -> 203/202/201/200 Null
  29 Null
    28 Animation(tab_mask) -> 30/31/32/244 AnimControl(tab_name)
    18 Animation(tab_base); 70/133 AnimControl [external]
  135 Animation -> BG
  8/9 Animation -> c_scope
  138 Null
    139 Null -> 140 AnimControl(cursor0), 141 AnimControl(on_off)
      141 -> 142 AnimControl(select_l0), 145 AnimControl(select_r0)
    146 Null -> 149 AnimVariable(bar00), 150 AnimControl(cursor0)
    151 Null -> 152 AnimControl(select_cursor0), 164 AnimControl(cursor0)
      152 -> 154 AnimControl(select_l0), 156 AnimControl(select_r0)
    267 Null -> 268 AnimControl(cursor0), 269 AnimControl [external]
  165 Null -> 239 Animation(mask) -> 166 Null -> 13 null holders
  183 Null -> 195 Null -> 196/197/198/243 Null
  287 Null -> 293 Animation(mask_keybd) -> 294 Null -> 22 null holders
  357 Null -> 358 Null
```

Texture/atlas paths include `id\menu\option\t_options00_ID` through
`t_options03_ID`, `id\localize\jpn\word_ID_jpn`, and
`id\p_option\options04_ID`. The three referenced GUI resources are
`id\common\npc_wind`, `id\common\a_title`, and `id\common\custskl0`.
The font and gradation texture match those listed for load0_001.

## Remaining work

### Cockpit renderer work in progress

`uGUILifeGauge::vf58` (`0x006781c0`) loads
`id/DDN/cockpit/cockpit0_1`. Its player HP and stamina nodes are at
(243,643) and (243,655) on the authored 1280x720 screen. The bar geometry,
atlas corners and numeric HP glyphs now come from this resource at runtime.
`0x0067c590` crops both width and texture coordinates, retaining a three-pixel
minimum for positive HP/stamina. `0x00679070` fades full stamina to 0.3 alpha.
`0x00678210` truncates HP, retaining one for a positive fraction;
`0x00679290` packs its four numeric glyph nodes leftward without leading zeros.

Object construction is `0x01077ec0`: animation word +4 low16 is object count,
high16 is sequence count, +12 is the first a4 object. Object +4 low8 is
constant-property count; the following byte counts animated properties,
not sequences. Each object's a8 sequence spans select b0 tracks; b0 flags
low8 select the value pool and bits8..23 count keys. `mtgui::layout` exposes
these records without claiming a full interpolation VM.

`cGUIInstAnimControl::vf19` (`0x010782a0`) and `0x0107ec10` consume node
extra data: a count then 20-byte records, selector at +8 and start/end
frames at +12/+16. Normal health and stamina use selector 5
(`0x00678ec0`, `0x00679070`); colours are authored animation keys.

`uGUIActionPallet::vf58` (`0x005ee480`) loads `id/common/custskl0` and the
localized `id/DDN/message/actionpallet/actionpallet` resource. The keyboard
group's nodes 175/190/197/204 hold light/heavy/interact/jump text; the
positions correspond to ref001. `0x005ed3b0` adds 0x8c to keyboard binding
scan codes before looking up the control selector. Key icons are selected
from the authored texture and size keys, not a copied atlas table.

The presentation checkpoint is incomplete: the old overlay is retained for
binding checks, prompt typography uses the viewer font, pawn world anchoring
uses the highest animated joint provisionally (the exact source anchor is
unresolved), and full animation/status transitions are not yet executed.
The screen projection's 70cm camera-facing offset and -40 vertical offset
are traced to `0x00679a10`. Comparison gate remains open.

The reader is a bounded table/node/resource reader, not widget playback.
Animation interpolation, serialized sequence opcodes, extra-data layouts,
GUI font/icon formats, controller behavior and rendering remain unimplemented.
The node graph and links have executable/data evidence, but no screenshot
comparison or input behavior validation. Next action: trace the loading-message
resource assignment and options control-to-message bindings, then define a
separate headless per-instance GUI state model before any presentation work.
