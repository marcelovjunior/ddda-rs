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

## Talking (traced)

### Which conversation plays

A plain talk (`0x00bb4830`, general branch) asks the chooser for the
current story band (`sGameSys+0x3c`, `World::sce_band`), falls back to
conversation **6** when it finds none, and shows that row of the NPC's base
talk set (`0x00455990`). Special cases not translated: NPCs 0x1f9 and 0x200
under sGameSys `+0xb8844` bit 0x40000 (random 12/15 and 13/15).

The chooser `0x00bb8cf0(band)` works on the base set (the NPC's message
set with `+0x10 == -1`, else `-2`) and the NPC's record in `sNpcManager`
(0x110 bytes per NPC): two 128-bit memories at `+0x898` (said once, type 1)
and `+0x8a8` (said), the last conversation at `+0x8ba`, and one sequence
counter per band at `+0x8c8` (all reset to -1 by `sNpcManager::vf10`,
0x00475c80). Rows are tried in order:
- a row numbered 128 or more is always a candidate; a lower one only when
  neither memory bit is set or it is the last conversation said;
- `mSeason` must equal the band;
- conditions, each only when set: quest flag (`mQuestNo` and
  `mQuestFlgNo`), `mGsfFlgNo` (below 0x600), affinity within
  `mFriend..=mFriendMax`; all must hold;
- with conditions, the band counter first drops to the row's
  `mSeasonSequence` if higher; with or without, the row wins when the
  counter is below its sequence.

With no winner, a counter of 0 or more drops by one and the band is tried
again; then bands above 1 fall back to the band below. The winner sets the
"once" bit for sequence type 1, and for any type but 2 the "said" bit, the
last conversation and the counter. The effect is a story told in order per
band, repeating its last line, with flagged rows slotted in when their
flags come up. Quina (install test): band 1 gives 76 "You should be abed.
I wish you would not strain yourself so.", 13, 90, ...; band 2 gives 20,
18, 21, then 21 again until quest 52 flag 94 is set, then 22 "Benita told
me of the flowers you gathered for the wounded.". Not translated: the
stage-200 case for placement id 995 (returns 300 or 11).

A shop NPC (one with a shop list) opens its **menu conversation** instead
(`0x00b9ddc0`): 100 for most keepers, 110 when sGameSys `+0xb8840` bit
0x8000000 is set (the menus with an equipment option), Olra/Barroch 102,
Devyn 110 (100 with a ready pawn), NPC 400 120/130, nothing for Olra on
stage 200. Kinds 1 and 2 (105 / 106 for most keepers; Caxton: "Take proper
care of that." / "Good, good.") follow a closed shop screen; that kind 1 is
"after buying" and 2 "after selling" is **inferred** from those lines.

### Lines, choices and commands

A conversation's lines are its `mMsgTbl` slots, each a GMD index in the
NPC's talk file (`id/npc_wind/npc_base_talk/nNNN_<lang>`), left out when the
slot's `mGfsFlgNo_msg` names a clear flag (`0x00cc9ca0`). Its choices are
`mSelMsgTbl` (GMD indices); a choice whose `mSelMsgJump` is set continues
with that conversation (Caxton's "Never mind" → 104 "Come again.").

Commands come from **markup in the line text**. `sGUIExt::vf16`
(0x0044f950) handles each tag; command tags set sIdMessage `+0xe4` and
`+0xdc`, and when the message closes `sIdMessage::vf06` (0x00455280) runs
handler `+0x44 + n*4` (table built by 0x00454f80). Tags: `GSKL` 1, `GILD` 2,
`STRG` 3, `SHOP` 4, `TENT` 5, `JUNK` 6, `UPGR` 7, `GV_F` 8, `EQSH` 9,
`BABA` 10, `EQUG` 11, `RMSH` 12, `EQRM` 13, `CHK` 14, `EQJK` 15, `RMVC` 16,
`UPG2` 17, `GSK2` 18; `<FUNC n>` names handler n; `<INN>` starts a free
rest at once (0x005188b0). Caxton's menu line is `<UPGR>They're
masterworks, all. You can't go wrong.`.

The menu handlers read the chosen option (sIdMessage window `+0x38c`) and
open GUI 7 in a mode (SHOPS.md): `SHOP` buy/sell; `JUNK`, `RMSH` buy;
`UPGR` buy/sell/enhance; `EQSH` buy/sell/equipment; `EQUG`
buy/sell/enhance/equipment; `EQRM` buy/equipment; `EQJK` buy/–/equipment;
`UPG2` the level 5-6 upgrade screen. Handler 0 is the paid inn rest and 14
its price check; a handler's return value is the next conversation (the
inn returns 102 / 104).

### How a script's `Message` id picks its line

(claude-7's open question.) `Message` type 2 (0x005c5bc0) passes
`mMesId` and `mQuestNo` to 0x004560d0, which finds a **message set** for
the quest: first the quest object's three message-set lists (0x004846c0;
entries 1.. tested with 0x004575f0, then entry 0 by row lookup), else any
active NPC whose set is tagged with that quest and holds row `mMesId`.
The set then shows **conversation number `mMesId`** (0x00455a50), i.e. an
MSS row, not a GMD index; its lines are that row's `mMsgTbl` entries in
the set's GMD. Type 3 uses the current area's set (`aStage+0xa10`). The
quest object's sets are presumably the `q%04d_n%03d` quest talk files
`uNpc::vf32` loads; that link is not traced.

## Runtime: `crates/ddnpcrt`

Host-owned, inputs → state → requests, N players.

- **Schedules** (`schedule`): `NpcRuntime::spawn` starts the controller
  from `schedule_path`; each frame the controller and the action graph it
  loads run on the shared AI core (`ddai::core`) with an NPC owner:
  `SysCondition` (bit 2 = the interface is done, set by `SetInterface`'s
  state callback 0x00533a90; cleared on a state change, **guessed**),
  `FreeCondition` (`Set FreeCondition`), `GameHour`, `GlobalTime`.
  Interfaces become requests: `Goto` (point, run, route), `Wait` (seconds,
  facing), `Motion`, `Place` (`SetInitPosNpc`, `DemoSetPos`). The host's
  actor layer reports done; until a headless NPC mover exists, the host
  completes moves at once, so an NPC stands at each route point it is sent
  to. NPC graphs set `mFSMAttribute` bit 2 (per-cluster work memory,
  0x00e09750) and have sub-machines, which `ddai` does not port yet; the
  runtime clears the bit, so sub-machines restart on entry (**guessed**).
- **Talking** (`talk`, `NpcRuntime::talk/talk_next/talk_choose`): the
  chooser and menus above; events `Line`, `Choices`, `Action` (shop mode,
  inn, other handler), `End`. One talk per NPC: a second Arisen gets
  `Busy`. Talk memory is world state (`NpcRuntime::memory`), not yet in a
  save chunk.
- **Host** (`ddhost::npcs`): `Host::talk` takes the interaction lock;
  a shop command opens the `ddeconomy` session (`open_shop` for buy/sell,
  `open_upgrade` for the upgrade screens) and the lock stays with it until
  `close_shop` / `close_shop_talk` (which plays the after-screen line);
  any other ending frees the NPC; `leave` aborts a seat's talk. NPC
  schedules step in the tick (pass A line 15) and their requests go out in
  `FrameReport::npc_requests`.

Tests: `cargo test -p ddnpcrt` (4 synthetic: chooser order and flags,
affinity/GSF, menus/tags/actions, a talk session with the lock; 3 install:
Quina's position across a day, Quina's talks by band and quest flag,
Caxton's menu) and `cargo test -p ddhost --test npcs` (talk → shop session
under the lock with two players; Quina's schedule in the host tick).

## Remaining work

- Not compared live: a Quina talk, the 06:00 schedule switch, Caxton's
  menu.
- Not implemented: quest talk sets and quest/daily FSM arbitration, NPC
  locomotion (walk speed, navigation), the per-cluster work memory, the
  inn and equipment handlers in the host, the other command handlers
  (storage, guild, Rift shop), talk memory in the save, MSL serial
  consumers.
- The stored field `mLikeItem` is retained as a number; it is not an item id.

Next action: record one Quina interaction and a 06:00 schedule
transition, logging the selected controller, conversation number and GMD
index, and compare with the runtime.
