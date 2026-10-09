# Game text: GMD 1.2.1

`mtgmd` reads `rGUIMessage` resources from the player's install. It is a
headless, dependency-free reader; strings borrow the source bytes. The
reader and CLI are implemented and tested against the complete install.
Rendering, substitution of markup, and text selection during gameplay
have **not** been compared with the original game.

## Evidence and coverage

Measured 2026-10-09, Steam PC build identified in `METHODOLOGY.md`:

- 16,338 archive instances, 8,488 resource paths, 7,954 distinct payloads.
- All instances parse with zero failures, including every archive copy
  (the test uses `Vfs::read_at`, not just `Vfs::read`).
- Across distinct payloads: 305,099 messages and 93,846 labels.
- Across archive instances: 829,339 messages and 286,232 labels.
- Every message, label and resource name decodes as UTF-8. Every resource
  is version `0x00010201`; the eight reserved header bytes are zero.
- Independent starting reference: Riftstone's MIT format notes, private
  reference `E:\ddda-re\refsrc\riftstone_formats.md`, GMD section. No
  reference implementation or extracted asset was copied into this repo.
- Loader and lookup evidence: read-only Ghidra inspection of
  `0x00dde1c0`, `0x00dde560`, `0x00dde580`, `0x00dde5f0`.

Run:

```text
cargo test -p mtgmd -- --nocapture
cargo run -p ddtool -- msg-dump <nativePC> id/DDN/message/common/enemy_name_eng
```

The corpus test defaults to the maintainer's install path and skips if it
is absent. Set `DDDA_NATIVE_PC` to another install; an explicitly supplied
missing path fails. Three synthetic tests exercise UTF-8, empty strings,
duplicate/case-sensitive lookup, absent IDs, malformed input and every
truncation boundary. The corpus test also checks item index 0 = `Airtight
Flask` and enemy label `e0100_goblin` = index 0 = `Goblins`.

## Binary layout

All integers are little-endian. Offsets below are absolute until the
variable-sized sections begin.

| Offset | Field |
|---|---|
| `0x00` | Four bytes `GMD\0` |
| `0x04` | u32 version `0x00010201` |
| `0x08` | u32 language ID |
| `0x0c` | Eight reserved bytes (preserved, meaning unresolved) |
| `0x14` | u32 label count |
| `0x18` | u32 message count |
| `0x1c` | u32 label block byte length |
| `0x20` | u32 message block byte length |
| `0x24` | u32 resource name byte length, excluding NUL |
| `0x28` | Resource name bytes, then NUL |
| next | Label count entries of `(u32 message_index, u32 stored_pointer)` |
| next | Label block: one NUL-terminated UTF-8 string per label |
| next | Message block: one NUL-terminated UTF-8 string per message |

`stored_pointer` is **not a file offset**. The loader reads the label
strings sequentially and overwrites each pointer with a runtime address.
The reader preserves the stored value for inspection but never follows it.
The reference notes identify these as authoring pointers; that provenance
is not needed by the reader. Labels map to message indices, not hashes.

The reader validates magic, version, lengths, complete block consumption,
NUL termination, UTF-8, declared string counts, label indices, and absence
of trailing data. It does not require a known language or zero reserved
bytes. Unknown header values are retained rather than assigned semantics.
There is no UTF-16 conversion, decryption, or compression inside GMD;
ARC handles the outer zlib layer.

Empty messages occupy indices. Newlines, whitespace and markup remain
literal text: CRLF/LF, `<ICON ...>`, `<ITNO ...>`, `<SIZE ...>`, and gender
forms must not be normalized by the reader. Resource name is usually
`TextWeb` (8,386 of 8,488 paths), not a lookup key for individual messages.

## Languages and paths

| Header ID | Suffix | Language | Resource paths | Archive instances |
|---|---|---|---:|---:|
| 0 | `_jpn` | Japanese | 1,300 | 2,423 |
| 1 | `_eng` | English | 1,198 | 2,322 |
| 2 | `_fre` | French | 1,198 | 2,319 |
| 3 | `_spa` | Spanish | 1,198 | 2,319 |
| 4 | `_ger` | German | 1,198 | 2,319 |
| 5 | `_ita` | Italian | 1,198 | 2,319 |
| 7 | `_zht` | Traditional Chinese | 1,198 | 2,317 |

No language 6 appears. Most language variants share a path stem. There
are 100 resources without any language suffix, all in the credits
families, whose headers still report language 0. Thus header language
is authoritative for the file format and does not prove that every line
is written in that language. The Japanese count also includes two extra
quest-log paths. VFS paths are case-insensitive and have no extension;
the examples below use original spelling and forward slashes accepted
by `ddvfs`.

## Executable lookup

The loader at `0x00dde1c0` accepts only magic `0x00444d47` and version
`0x00010201`, reads the sequential sections, reconstructs label pointers,
and builds a message-pointer array by walking NUL terminators.

- `0x00dde560`: index into the label-entry array (8 bytes per entry),
  bounded by label count at object offset `+0x70`.
- `0x00dde580`: linear exact byte comparison against label strings,
  returning the **first matching** entry's message index or `0xffffffff`.
  There is no hashing or case folding. `MessageFile::message_index`
  translates this rule and has a synthetic duplicate-label regression.
- `0x00dde5f0`: direct zero-based message-array lookup, count at `+0x80`,
  pointer array at `+0x90`. Out-of-range returns a global fallback pointer
  at `0x018d5928`; its contents are untraced. The Rust API deliberately
  represents missing messages with `None` instead of guessing fallback text.

The same label string can occur more than once: 29 of the 8,488 first-copy
paths contain duplicates. Retain the ordered label vector. Trimming or
renaming labels changes lookup semantics; e.g. `e0403_sauriansage ` has a
trailing space in the English enemy-name file.

## Resource index

These roles are based on inspection of English messages, not just path
names. `<lang>` denotes one of the seven suffixes above. Counts in the
message-count column refer to the inspected English file.

| Role | Resource path or family | Message count / evidence |
|---|---|---|
| Item names | `id/message/item/itemName_<lang>` | 1,903; indices 0/1 are Airtight Flask / Scrag of Beast |
| Item descriptions | `id/message/item/itemInfo_<lang>` | 1,903; corresponding container/meat descriptions |
| Item effect phrases | `id/message/item/item_ability_<lang>` | 117 |
| Enemy names | `id/DDN/message/common/enemy_name_<lang>` | 112 messages, 111 labels; base and expansion enemies |
| Base skill names/commands | `id/message/skill/skillName_<lang>`, `skill_command_<lang>` | 305 names, 48 command strings |
| Expanded skill names/info | `id/DDN/message/skill/skillName_<lang>`, `skillInfo_<lang>` | Expanded skill names: 436 |
| Augment names/info | `id/message/ability/abilityName_<lang>`, `abilityInfo_<lang>` | Names: 83 |
| Pawn inclinations | `id/message/common/personalityName_<lang>` | 9; first is Scather |
| Pawn nicknames | `id/message/common/nickname_<lang>` | 885 |
| Conditions | `id/message/pause/party/status/anomalous_conditionName_<lang>`, `anomalous_conditionInfo_<lang>` | 39 each; poison and its description |
| Quest names | `id/message/common/quest_name_<lang>` | 115 |
| Quest journal | `id/questlog_data/q####_log_<lang>` | 709 paths total; quest title and journal progress entries |
| Notice-board quests | `id/message/quest_request/quest_boardName_<lang>`, `quest_destination_<lang>`, `quest_request_<lang>` | 726 each; titles, objectives, request prose |
| NPC base dialogue | `id/npc_wind/npc_base_talk/*_<lang>` | 1,792 paths; NPC/enemy utterances, combat sounds, greetings |
| Quest-specific NPC dialogue | `id/npc_wind/npc_quest_talk/q####_*_<lang>` | 3,822 paths; quest/NPC-specific lines |
| Stage interaction messages | `id/npc_wind/stage/st###_<lang>` | 371 paths; locked-door and other stage text |
| Event/cutscene dialogue | `id/message/event/st*ev*_<lang>` | 378 paths; voiced-scene dialogue |
| Pawn chatter | `id/message/pawn/*_<lang>` | 896 paths; enemy calls (`em0100` etc.), education, area remarks |
| Shared UI | `id/DDN/message/common/common_message_<lang>` | 325; shared menu phrases |
| Map/place names | `id/DDN/message/common/map_placelist_<lang>` | 277 |
| UI by screen | `id/message/pause/**`, `id/message/shop/**`, `id/message/main_menu/**`, `id/message/cockpit/**`, `id/message/charactor_edit/**`, `id/DDN/message/**` | Menus, HUD, character editor, shops, expansion screens |
| Save/load and system UI | `id/DDN/message/system/saveload_system_<lang>`, `system_announce_<lang>`; `id/message/online/system/online_system_message_<lang>` | 76 / 29 / 85 |
| Tutorial and added messages | `easytu/message/easytu_message_<lang>`, `id/DDN/message/tutorial/tutorial_<lang>`, `tu4/id/message/ext_message/ext_message_<lang>` | 11 easy-mode notices / 262 tutorials / 33 added-item notices |
| Credits | `id/credit_02/*`, `id/DDN/DDNcredit_01/*`, `id/message/end_credit/*` | First two families have 46 / 54 unsuffixed resources |

Enemy labels provide useful evidence but **are not a universal model-ID
map**. `e0603_succubus` and `e0603_gargoyle` have the same numeric prefix;
expansion labels use `em` instead of `e`; some labels are generic or have
typos. Preserve exact strings and verify each consumer's mapping. Similarly,
matching item-name/description indices does not itself prove every gameplay
item ID maps directly to that index.

## Other resource types and remaining work

Searching `mtarc::RESOURCE_CLASSES` for Message/Text/Talk/Font finds
`rGUIMessage`, `rGUIFont`, `rTalkMotion` (plus texture class names when using
a substring search). The install has six font instances at three paths,
magic `GFD\0`, under `id/common/font/`; these are separate font resources,
not GMD message tables. The one `rTalkMotion` at `etc/talk/talk_mot` has
magic `tmd\0`, version 16. Its loader `0x00ce8fd0` reads counts and compact
motion records, not a NUL-terminated dialogue string table. Neither font
rendering nor talk-motion playback is implemented here.

Other serialized resources may contain incidental strings or references
to messages; this brief does not claim all strings in every binary type
are GMD. Generic XFS inspection remains available through `mtxfs`.
The route from scenario/FSM message IDs and type/quest selectors to a
specific GMD resource remains to be traced. Archive precedence also
remains the existing `ddvfs` limitation: CLI reads the first sorted copy,
while corpus validation checks all copies. Next action: trace a concrete
NPC interaction's resource selection and markup rendering.
