# Rift UI resources and bindings

Evidence: DDDA.exe 2017-12-13 Steam, the installed ARC resources, and the
existing `mtgui` / `mtgmd` readers. This is a resource and executable-binding
survey, not an implemented menu or an original-game visual comparison.
See [GUI.md](GUI.md) for the file grammar and [PAWNS.md](PAWNS.md) for the
Rift pool, hiring and persistence rules.

## What was checked

On 2026-10-09, all five `rGUI` paths containing `pawn2` parsed: six archive
occurrences, 573 unique-path nodes / 637 occurrence nodes. Each is 1280 x
720. The existing reader needs no format change for these trees. All 49
message paths in the seven families surveyed below parsed in all 105
archive occurrences. This includes the older non-DDN skill-name family;
that family is not the one loaded by these UI constructors.

`cargo test -p mtgui` passed seven synthetic tests (the two opt-in install
tests were not run in this batch). The targeted survey used
`Vfs::paths_of_type`, `locate`, `read_at`, `Gui::parse`, `Gui::preorder` and
`MessageFile::parse` to check every copy, then inspected the first copy of
each path. No personal save data, resource bytes or decompiler exports are
part of this report. Archive precedence and rendering remain unverified.

Reproduce any full tree using the existing read-only example:

```powershell
cargo run -p mtgui --example inspect -- 'E:/SteamLibrary/steamapps/common/DDDA/nativePC' 'id/ddn/pawn2/pawn'
```

Replace the final argument with another path in the table. The trees below
are structural summaries, not a draw-order or screen-state specification.
Numbers are **instance IDs**, and names are resolved local animation names.
A local animation name is not an instance name. External animation names
are deliberately omitted.

## Resource ownership and tree groups

| Path | Copies | Nodes / local animation definitions | Loader |
|---|---:|---:|---|
| `id/DDN/pawn2/pawn` | 1 | 322 / 68 | `uGUIRimSearch::vf58`, `0x006cd9f0` |
| `id/DDN/pawn2/pawn_cmcname` | 1 | 16 / 11 | `uGUIPawnInfo::vf58`, `0x006bf210` |
| `id/DDN/pawn2/pawn_detail` | 2 | 64 / 24 | `uGUIPawnDetailBase::vf58`, `0x006bbcb0` |
| `id/DDN/pawn2/pawn_results` | 1 | 108 / 40 | `uGUIPawnReduce::vf58`, `0x006c04d0` |
| `id/pawn2/pawn_review` | 1 | 63 / 19 | `uGUIRimEvaluate::vf58`, `0x006c8490` |

### Search, level selection, filters and result list: `pawn`

These are groups in one GUI resource, not separate GUI files. Root is null
ID 241. Important children:

- 836 `current_lv`, 274 `current_money`; title/subtitle holders 692 and 597.
- Tab groups 716, 279, 756 and 860; list-information labels 757..761.
- 284 menu group: 452 contains level selector 458 `contents_Lv`, preview
  832 `cost_Lv`, button 470, message nodes 457/833/834/835 and arrows
  961/962. Group 246 contains menu text controls 258/555/619/621 and exit
  877; 245 contains cursor 244 and positioning holders.
- 928 list-action group: 930 contains 929 `list_sel_txt`, 933 contains
  cursor 934; 937 is the selection button.
- 739 auxiliary/retained-pawn list group: four row variants under
  771/746/804/805, with skills, name/level, name/gender, and paired names.
  Groups 698/863/871 contain pawn cursor, list cursor and keep effect.
- 304 search-results group: scrollbar 419, header messages under 494/319,
  cursor group 309, and four result-row templates described below.
- 577 filter window: checkbox/text templates 899/900, 580/581 and 911/912
  under 595/584/907; tab group 585, action controls 894/895/896.
- 942 `bg` is a root child. Additional separator group 735 and tab/heading
  holders do not themselves establish a navigation state.

The row templates under 304 are:

| Holder -> mask -> row | Contents (instance IDs) |
|---|---|
| 310 -> 332 -> 320 | rank 630, job 305, name 328, level 329, cost 330; icons 736/737/738/946; placeholder 957 |
| 642 -> 643 -> 644 | rank 641, job 647, name 639, gender 640, quest icon 308; icons 797/798/799/948; placeholder 959 |
| 361 -> 365 -> 366 | rank 632, job 562, skill controls 394/410/412 with frames 392/409/411; icons 766/767/768/949; placeholder 958 |
| 702 -> 703 -> 704 | names 707/712, rank 722, job 861; icons 800/801/802/950; placeholder 960 |

`0x006d4070` constructs seven clones of each result layout, spaced using
positions in the GUI, and binds their runtime integer variables. This is a
seven-row visible window over a larger list, not a seven-pawn search limit.
`0x006d5090` fills up to seven non-excluded rows. The separate auxiliary list
setup at `0x006d25b0` builds five copies of its templates. These counts must
not be confused with the Rift room's summon limit.

### Floating pawn information: `pawn_cmcname`

Root 2 has arrow 8, message controls 15/3, name control 6 `cmc_name`, job 17,
base 4 and mask 7. Mask 7 contains text 5 and holders 9/10. Root child 24
contains status icons 21/22/23/25. `0x006bf210` loads the skill-name resource
and also takes a message pointer from `sGameSys+0xbe320`. The provenance of
that shared pointer and the complete per-field update are not traced here;
the tree alone does not prove the icon meanings.

### Pawn detail and exchange: `pawn_detail`

Root 241 has popup group 915, message group 61, current marker 910, tabs
716/240, arrows 894, shared title 192, content group 282, and background 913.
Under 282, left name/cost/level group 297 has 300/635/875 and icons
847/849/850/919; right group 298 has 301/636/876 and icons 848/852/851/920.
Model groups 853 and 855 contain mask/texture pairs 247/854 and 856/857.
Window groups 286/281 contain 897/898 and content holders 302/303;
additional comparison holders 781/787 contain windows 782/788 and content
784/790. Arrow group 894 contains 900/892/893/917.

`uGUIPawnDetail` and `uGUIPawnDetailExchange` use the base class resource.
The paired tree supports comparison, but exact tab-to-stat bindings, model
render-target updates, and the exchange confirmation state machine remain
open. Search-message IDs 86..96 supply general/profile/equipment/skills,
resistance and knowledge/creator headings; their presence is not proof that
every heading is reachable in each detail mode.

### Return results: `pawn_results`

Root 241 has balance 611, tabs 279, title 192, report body 284, subtitle 449,
and list 304. Body 284 contains menu/cursor groups 575/571, comment card 553,
rating group 520 (messages 524/525/526 and stars 582/596/597), knowledge mask
559 with lines 600..605 and category 606, item-description group 91, and
item group 577 with its icon/equipment/upgrade indicators. Scrollbars 560
and 564 are separate item/knowledge controls.

List 304 has scrollbar 419, loading indicator 423, cursor group 426, header
494/319, and row group 310 -> mask 434 -> row 435, plus repeated holders
441..447. `0x006c04d0` loads `pawn_result`, common messages, item ability,
enemy names, quest names, map place names and `pawn_search`; it retains
shared pointers at `sGameSys+0xbe32c/+0xbe330`. The reward/history-record to
row mapping, shared-pointer provenance and receipt/acknowledgment state
machine are not decoded in this brief.

### Evaluation/comments: `pawn_review`

Root 241 contains title group 240, shared control 192 and body 284.
Body 284 contains subtitle 521, ratings/comments group 520, and backgrounds
533/285. Under 520 are stars 564/573/574, rating-label messages 524/525/526,
three rating controls 529/527/528 with arrow-button children, buttons
530/531, comment cards 553/633, and two text-list groups 552/649 with cursor
groups 543/655. The lists' template text controls are 535/652.

`0x006c9060` initializes three rating indices at object +0x308/+0x30c/+0x310
to 2 and comment choices +0x314/+0x318 to 0xff. This is an index, not a
claim of a displayed two-star default. Search messages 107..109 label the
three ratings, 110..112 and 116..122 classify comments, and the
`pawn_revcom_comment0..107` labels are in `pawn_search`, not a separate
review-message resource. Rating submission, index-to-displayed-star mapping
and the two comment-list selection writers remain open.

## Text resources

All serialized GUI message tables are empty (the existing GUI survey).
For Search, `0x006cd9f0` explicitly loads these localized families; the
pointers in `uGUIRimSearch` are listed below. `0x00607610` appends the language
suffix. Do not derive these links by matching GUI filenames.

| Resource stem | Search offset | Messages per language / copies across seven languages |
|---|---:|---:|
| `id/DDN/message/common/common_message` | +0x61c | 325 / 14 |
| `id/message/pawn_search/pawn_search` | +0x620 | 309 / 14 |
| `id/DDN/message/skill/skillName` | +0x624 | 436 / 14 |
| `id/message/common/nickname` | +0x628 | 885 / 14 |
| `id/message/online/system/online_system_message` | +0x62c | 85 / 14 |
| `id/message/pawn_result/pawn_result` | results loader only | 54 / 21 |
| `id/message/skill/skillName` | not loaded by these constructors | 305 / 14 |

Each family has `eng/fre/ger/ita/jpn/spa/zht`. `pawn_search` has 282 labels
per language; `pawn_result` has 40. Preserve exact label spelling, including
trailing spaces, and inline formatting tokens. A numeric message ID belongs
to its specific resource. For example, 36 is the by-level result heading,
whereas 14 is the search submenu label; messages 66..71 are help text, not
menu captions. The captions are 6..11. Other useful search IDs: 38..42
information tabs, 44..52 column headings, 281..284 result-list actions,
286 summon count and 291 summon-menu title. Result-message IDs 9..12 select
items/knowledge/rating/average-rating headings; 33..38 describe knowledge
and unknown targets. These IDs are inventory evidence unless a consumer is
specified below.

## Menu items and search categories

The PC menu callbacks provide a more precise navigation map than the
message inventory. `0x006cfc80` builds four top-level entries;
`0x006d0350` handles their selection. Item 0 opens the five-entry submenu
built by `0x006d0590`. Items 1 and 2 request categories 6 and 0x12 only when
online; item 3 leaves the Rift through `0x00518ce0`.

The submenu's caption IDs are `[14, 9, 11, 8, 10]`, and its help IDs are
`[73, 69, 71, 68, 70]` (`0x006d0590`, `0x006d08c0`). The callback
`0x006d0b50` proves the category arguments in disassembly (ECX is omitted
from several decompiler calls):

| Selection | Caption ID / meaning | Action/category | Offline |
|---|---|---|---|
| submenu 0 | 14, by level | opens level selector; category 1 route | available |
| submenu 1 | 9, favorites | 0xe | available |
| submenu 2 | 11, friends | 0x10 | blocked by online predicate |
| submenu 3 | 8, past summons | 0xd | available |
| submenu 4 | 10, pawns knowing the main pawn | 0xf | blocked by online predicate |
| top-level 1 | top ranking (help 66) | 6 | blocked |
| top-level 2 | monthly ranking (help 300) | 0x12 | blocked |

Online predicate: `sNetwork+0x545 == 2`, or `==4` with `+0x5a34 == 0`.
`0x006cf3e0` calls `0x0073b5c0` and stores the category at GUI +0x618.
`0x006cc970` selects category headings/help and defaults information mode:
category 1 starts at mode 0; ranking/familiar categories generally start at
mode 4. The generic switch also handles 3/4 (username), 5/6, 7, 9/0xd,
10/0xe, 0xb/0xf and 0xc/0x10. This does **not** prove reachability of those
extra categories through the above PC menu. In particular, the special-pawn
caption exists but is not an entry in these callbacks; category 7's caption
mapping remains unproven. Categories 2, 5, 8 and 0x11 still need callers.

`0x0047a550` builds the underlying list: category 1 requests 100 entries,
uses downloaded results first, then `0x00794f10` offline templates; 9/0xd
uses past summons, 10/0xe favorites, 0xb/0xf the lent list. List records are
`nRim::cPawnInfoList`, stride 0x18, with state +4, index +8, version +0x10,
and `cCmcInfo*` +0x14. This raw list is separate from the filtered visible
list. See PAWNS.md for duplicate prevention and network/error cases.

## Result row binding and filtering

`0x01077560` takes a **node-array index**, gets its original instance ID
through the owning GUI's table, and finds the corresponding node under a
clone. Its arguments are not instance IDs. Matching both index and ID is
necessary when connecting the following setters in `0x006d5090`:

| Node index -> instance ID | Binding |
|---|---|
| 0x16 -> 305 | vocation icon animation selected from `cCmcInfo+0x10` |
| 0x24 -> 328 | pawn display name through `0x005ddf00` |
| 0x25 -> 329 | level at `cCmcInfo+0x700`, `%3d` below 200; a runtime string at `0x018d9230` otherwise |
| 0x26 -> 330 | base cost through `0x0043edf0`, number formatting `0x005dcff0` |
| 0x73 -> 639, 0x74 -> 640 | vocation and gender text from common messages; exact message constants still to trace in disassembly |
| 0x31/0x33/0x35 -> 394/410/412 | three primary skills from +0x270, or secondary skills from +0x27c when information mode is 3; -1 displays the empty marker |
| 0x87 -> 707 | pawn name/nickname selection through `0x00767450`, +0x704 and +0x708/+0x70c |
| 0x8a -> 712 | creator-name text (+0x15fc); formatting/platform policy not fully traced |

The level-at-200 display is deliberately left unnamed: `0x018d9230` is
runtime storage, not an available static string in the analysis image.
The list's Base Cost uses the same hire-cost rule documented and tested in
PAWNS.md. Balance formatting at `0x006cd880` reads `sGameSys+0xa7a1c` and
clamps it at 99,999,999. A future per-player interface must receive the
viewer's level, balance and friendship context explicitly.

The four general-row indicators are set separately from list state:
node 0x9c/ID736 uses `0x0043eff0(cOnlinePawnInfo*)`; 0x9d/ID737 tests a
network pawn ID (-1) and nonzero version; 0x9e/ID738 combines membership
checks `0x00448320`, `0x00447fe0`, `0x00447b80`; 0x132/ID946 is visible when
`cCmcInfo+0x164c` (online-info version) is zero. Their asset names are
`sp_icon1..4`. Exact visual glyph interpretation, especially version zero,
requires an original screenshot; do not name them solely from the number.
Equivalent checks appear in the other row layouts. The quest indicator
uses `0x005d0720`; its complete quest criterion is outside this brief.

**Offline filtering exists in the GUI.** This resolves the explicit
GUI-side uncertainty in PAWNS.md without changing the pool builder:

- `0x006d4bb0` visits each raw list row, calls `0x006d4c50`, stores an
  exclusion byte at GUI +0x428 + raw index, and updates visible count +0x494.
- `0x006d4c50` has no online-only gate. Unless bypass +0x5a4 is set, it
  checks vocation against six selections at +0x588. The index is job-1 for
  jobs 1..6, otherwise job-4 (the original does not bounds-check here).
- Unless bypass +0x5a5 is set, twelve selections +0x58e..+0x599 build a
  12-bit OR mask. Each of six skill IDs +0x270..+0x284 is checked against
  the skill table at `0x014f3658`, stride 0x20. Valid entries select a row
  in `0x01520278`, stride 0xc; its +0xa flags masked with 0x3fff must
  intersect the selected mask. At least one skill must match.
- Unless bypass +0x5a6 is set, gender chooses one of the two selections at
  +0x59a using `cCmcInfo+0xc != 0`. Vocation, skill and gender restrictions
  combine with AND. No selected skill categories means no match when the
  skill bypass is clear. The twelve labels are search messages 21..32.
- Failed-load state 0x8000 is excluded whenever any bypass is clear;
  otherwise it remains a placeholder row. `0x006d5090` skips exclusion
  bytes before filling the seven visible positions.

List state is checked by equality in the observed UI, not an arbitrary
bitmask combination. `0x006d7ba0` maps 0x800 to message 269, 0x2000 to 272,
and 0x1000/0x4000/0x8000 to 263. Successful summon marks 0x400. Placeholder
rendering treats missing data, load failure and offline network records
separately. Do not infer permission to hire from a displayed row.

## Remaining work and next action

Implemented: existing bounded readers only; no new reader or runtime code.
Tested: the above resource corpus and seven existing synthetic tests.
Compared against original: **none in this brief**. No game/process writes
or delegated input were requested.

Open: full state transitions and keyboard/controller behavior; top-level
caption-table initialization; category reachability beyond the callbacks
above; filter checkbox/bypass writers; ranking/download/time columns;
precise common-message constants; detail-tab stat bindings; results reward
bindings; evaluation submission; animation playback, text shaping,
localization texture substitutions and model preview rendering. Existing
PAWNS hire-cost/pool observations are separate evidence and do not validate
these UI bindings visually.

Next action: implement a headless, per-viewer list projection from explicit
pawn records and filter selections, with regression tests for the traced
predicate and seven-row pagination; then compare a known existing offline
Rift capture (or a maintainer-operated session) with that projection. Keep
world-owned summon/hire decisions separate from per-player menu state.
