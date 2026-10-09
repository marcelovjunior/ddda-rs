# NPC ledger, schedules and dialogue

`ddnpc` is a headless reader for the NPC ledger (`nnl`), message sets
(`mss`) and message serials (`msl`). It also exposes the traced NPC-intel
schedule path rule, clock quantization and freshly loaded message lookup.
It has no rendering, input, player singleton or simulation state. Callers
provide NPC identity, stage, scenario, clock and global-flag access.

Implemented and tested against the player's install, 2026-10-09. The
placement/schedule/dialogue chain below is checked against serialized data
and the 2017-12-13 Steam executable identified in METHODOLOGY.md.
**Not compared with a recording or live interaction.** There is no NPC
movement, conversation UI, route follower or host/client integration yet.

## Corpus and tests

`cargo test -p ddnpc -- --nocapture` runs seven synthetic tests and one
install integration test. The latter reads **every archive occurrence**
through `Vfs::read_at`, not only the first sorted copy.

| Class | Resource paths | Archive instances | Distinct byte payloads | Rows across instances | Rows across distinct payloads |
|---|---:|---:|---:|---:|---:|
| rNpcLedgerList | 1 | 1 | 1 | 727 | 727 |
| rMsgSet | 930 | 1,855 | 830 | 26,762 | 8,676 |
| rMsgSerial | 783 | 1,220 | 752 | 27,384 | 12,478 |

All **3,076 instances** parse with complete byte consumption. Synthetic
tests cover every truncation boundary, invalid magic/version/count,
trailing bytes, empty resources, signed sentinels, all field arrays,
duplicate conversation numbers, conditional line access, the loader's
quest-array overwrite, schedule gates and exact scenario/time boundaries.
Fixtures are synthetic; no game files are stored in the repository.

`DDDA_NATIVE_PC` overrides the default maintainer install path. The corpus
test skips if the default install is absent; an explicitly configured
missing path fails. The Quina integration uses first-copy `Vfs::read`;
archive precedence in the engine remains unresolved.

Starting reference: Riftstone's MIT format notes, sections on the NPC
ledger and "Faces, conversations, schedules, zones", consulted privately.
The loader grammars and properties were independently checked in Ghidra;
no reference implementation or private research file was copied.

## Binary layouts

All numbers are little-endian. All three headers are 12 bytes: four-byte
magic, u32 version, u32 record count. Exact supported pairs are `nnl\0`/8,
`mss\0`/0x20, `msl\0`/0x10. Unknown field values are retained; unsupported
versions fail rather than silently assuming the known schema.

### Ledger

`etc/item/npclist` contains 727 variable-size records. Loader
`0x00ccac90`; property names/types `0x00cca670`.

| Serialized order | Type | Properties |
|---|---|---|
| 0..5 | three s16 | mNo, mStageNo, mInitmFriendPoint |
| 6..9 | two u16 | mModelType, mIndex |
| 10..15 | three s16 | mStrayId, mShopType, mSeType |
| 16..21 | six s8 | mFlag, mChild, mTP, mTPType, mLikeItem, mCivilian |
| 22 onward | two NUL-terminated byte strings | mName, mNameJ |

The 100-byte allocation in the loader is a **runtime** stride, not a file
stride. String read helper `0x00cfee30` consumes through the NUL and retains
at most 31 bytes in its 32-byte destination; this reader preserves the full
serialized strings as borrowed bytes and does not guess an encoding or
silently truncate them. The ledger's `mNo` is the identity used by
`sNpcManager` lookup `0x004769c0`; it is not the entry's ordinal index.

### Message sets

Loader `0x00cc95f0`, properties `0x00cc8f60`. Each serialized conversation
is **250 bytes**; the runtime object is 456 bytes (`0x1c8`).

| Relative offset | Type | Property |
|---|---|---|
| 0 | s32 | mNo (conversation number) |
| 4 | s16 | mNpcId |
| 6 | 6 s32 | mSelMsgTbl (choice text indices) |
| 30 | 6 s16 | mSelMsgJump |
| 42 | 15 s32 | mMsgTbl (line text indices) |
| 102 | 15 s16 | mMsgMotTbl |
| 132 | 15 s8 | mMsgFaceTbl |
| 147 | 15 s16 | mGfsFlgNo_msg |
| 177 | 15 s16 | mQuestNo_msg |
| 207 | 15 s16 | mQuestFlgNo_msg |
| 237 | three s8 | mSeason, mSeasonSequence, mSeasonSequenceType |
| 240 | five s16 | mQuestNo, mQuestFlgNo, mFriend, mFriendMax, mGsfFlgNo |

Unused slots commonly contain -1. `active_messages` and `active_choices`
expose the prefix before the first -1, while retaining all later slots.
They describe serialized prefixes, not runtime conversation eligibility.

An executable bug matters: the loader writes both the second and third
per-line short arrays to runtime `+0x130` (`mQuestNo_msg`), leaving
`+0x16c` (`mQuestFlgNo_msg`) at the constructor's -1. Constructor
`0x00cc8ec0` confirms initialization. The reader retains both serialized
arrays; `loaded_quest_numbers` exposes the overwritten runtime value.

Lookup `0x00cc9ca0` searches the **first** row matching `mNo`, bounds line
slots to 0..14, reads `mMsgTbl[slot]`, and returns -1 on absent/sentinel or
failed global-flag condition. With freshly loaded rows, the per-line quest
condition always takes the no-condition path because `mQuestFlgNo_msg` is
-1. `loaded_message_index` translates that specific rule with a caller
provided global-flag callback. It does not apply the conversation-level
season, quest, affinity or choice/jump rules, and does not model runtime
edits to message records.

### Message serials

Loader `0x00cc8ce0` reads `count` u16 `mNo` values (property type confirmed
at `0x00cc8b40`). These numbers are **not** GMD message indices: Quina's
133-line base resource has serial 850 at line index 6. `MessageSerial`
preserves the ordered numbers. Their global namespace and all consumers
remain untraced; the reader does not invent a reverse-lookup policy.

## Placement and daily schedule selection

The daily schedule is an NPC FSM, separate from generic `rScheduler`/SDL
timeline playback. Concrete initialization chain:

1. `cLayoutSetNpc::vf15` (`0x00762000`) dispatches NPC placement creation.
   Its helper `0x007624d0` invokes the placement's virtual field transfer.
2. `cSetInfoNpc::vf08` (`0x007a32f0`) copies placement `mNpcId` at `+0xc8`
   into `uNpcIntel+0x3e4`, and `mUse24Schedule` at `+0xe2` into
   `uNpcIntel+0x457`. Property table `0x007a2480` verifies these names.
3. `uNpcIntel::vf05` (`0x00bbc4e0`) calls `0x00bbd930`, which creates a
   `cAIConsiderFSM_24Schedule` at intel `+0x15c` and a
   `cAIActionFSMDemo` at `+0x160`. It then calls `0x00bbdaa0` to select
   the controller. Reset helper `0x00bbdc80` repeats the same selection.
4. An id outside [0,900), or a schedule flag other than exactly 1, skips
   this controller load. Otherwise the path is
   `AI/FSM/Npc/stSSS/stSSS_nNNN` with these suffix rules:

| Stage | Story scenario | Suffix |
|---|---|---|
| 200 | below 3600 | `_00` |
| 200 | 3600 through 7799 | none |
| 200 | 7800 or above | `_01` |
| 240 | below 7800 | none |
| 240 | 7800 or above | `_01` |
| Other | any | none |

Stage is `sGameSys.mStageNo` (`+0x34`); scenario is `+0x30` (see QUESTS.md
and its traced `SceNo` getter). `schedule_path` translates this rule.
An explicit placement FSM path is also loaded by this setup; the complete
arbitration between quest FSMs and the daily controller is not implemented.
The separate `uNpc::vf115` (`0x00baf2d0`, containing the path reference at
`0x00baf433`) fallback has different gates and is not covered by this API.

`cAIConsiderFSM_24Schedule::vf03` (`0x0059d6d0`) exposes `GlobalTime` at
`+0xe4`. Update helper `0x0059d790` sets it to
`GameHour * 6 + GameMinute / 10` (unsigned integer division). Clock sources
are `sGameSys+0xb876c` and `+0xb8770`, with named getters `0x00435680` and
`0x004372b0`. `schedule_time` implements this quantization for valid clock
values and returns `None` for an invalid hour/minute. The same helper
maintains morning/afternoon edge flags; those flags are not implemented.

## Quina: checked chain

The name **Quina** comes from the game's ledger entry `mNo=2`, not an
external character list. That record has stage 200, model type 1, model
index 1, initial friendship 650. Stage on the ledger is not a restriction
to a single placement: Quina has layouts in multiple stages and quests.

The integration test chooses `scr/st100/etc/st100_55m45n_n180`, record 0,
kind 47 (`cSetInfoNpc`): id 2, schedule enabled, explicit FSM path empty,
placement coordinates `[-43144.04, 38557.266, 52742.195]` in the layout's
coordinate frame. This chooses `ai/fsm/npc/st100/st100_n002`.

That controller has owner `cAIConsiderFSM_24Schedule`, initial state 3,
and three transition-from-all conditions over `GlobalTime`:

| State | Condition | Clock interval | LoadFSM action |
|---|---|---|---|
| 0 | `>=114 OR <36` | 19:00–06:00 | `st100/action/go002_night` |
| 1 | `>=36 AND <72` | 06:00–12:00 | `st100/action/go002_morning` |
| 2 | `>=72 AND <114` | 12:00–19:00 | `st100/action/go002_afternoon` |

Paths in the last column are relative to `ai/fsm/npc/`. Operator codes
8/5/16/17 correspond to >=, <, AND, OR; see AI.md. The morning graph's
actual owner is `cAIActionFSMDemo`; its conditions also refer to inherited
`cAIActionFSMNpc.GameHour` (condition 24: 6..8, condition 25: 8..12).
The test checks the placement, generated controller path, morning state,
condition operators/thresholds, explicit LoadFSM path and action owner.
These checks do not assert that the motion route or pathfinding matches
the original.

Dialogue is selected separately from the daily schedule:

- `uNpc::vf32` (`0x00bace50`) formats
  `id/npc_wind/npc_base_talk/n%03d` from the NPC identity. Quest-specific
  variants use `q%04d_n%03d` and an executable quest list.
- Helper `0x00bad2b0` creates a `cMessageResSet`; its loader `0x00457670`
  reads the base path as MSS/MSL and appends the current language suffix
  for GMD. For Quina, these are `.../n002` and `.../n002_eng`.
- MSS conversation **7** has speaker NPC 2 and `mMsgTbl[0]=6`.
  GMD message 6 is "Till next time."; paired MSL element 6 is 850.
- MSS conversation **76** has line indices 13 and 14. GMD message 14 is
  "I worry for you, cos.". Conversation number 76 is not array index 76.
- `uGUINpcMessage` helper `0x006a2eb0` calls the MSS lookup `0x00cc9ca0`,
  then `0x006a56e0` calls GMD direct lookup `0x00dde5f0` with that line
  index. Thus MSL serial 850 is not used as the text index in this path.

This proves Quina's identity joins placement, daily controller, action
resource and available dialogue lines. It does **not** claim that entering
the morning schedule selects conversation 7 or 76, or that either line was
observed in a live interaction. Conversation eligibility and choice/jump
execution remain separate work with the quest/event lane.

## Remaining work

Untraced or unimplemented: the full NPC manager spawn/priority policy,
quest/daily FSM arbitration, action routes and navigation, conversation
eligibility and choices, MSL global serial consumers, localization name
exceptions, and live comparisons. The stored field `mLikeItem` is retained
as a number; it is not treated as an equipment/item id.

Next action: record one Quina interaction and a 06:00 schedule transition,
logging the selected controller, MSS conversation number and GMD index,
then compare those against the checked data chain.
