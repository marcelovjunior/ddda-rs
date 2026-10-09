# events-quests: cutscenes, events, quests and story flags

- **Owner:** claude lane
- **Branch:** `task/events-quests`
- **Board area:** `research/events`
- **Status:** review

## Goal

Find how the game scripts its story:
- **quests:** definition, stages, objectives, rewards, the flags they
  read and set, notice-board quests;
- **events and cutscenes:** what triggers them, how actors, cameras,
  motions, dialogue and sound are sequenced, and how control returns to
  the player;
- **the world-state flags** that gate NPCs, shops and areas.

This is the backbone of a 1:1 campaign. The output is documentation and
readers.

## Starting points

- **DTI classes:** `Quest`, `Event`, `Scenario`, `Cut`, `Demo` (Capcom's
  word for cutscenes), `Flag`, `Talk`, `Message`, `sEvent`, `sQuest`.
- **Data:**
  - event and quest resources in `nativePC`: find them with
    `paths_of_type` over `mtarc::RESOURCE_CLASSES`;
  - path names such as `event`, `quest`, `demo`, `scenario`;
  - XFS dumps via `ddtool xfs-get`;
  - dialogue text through `mtgmd` (`docs/TEXT.md`).
- **The opening:** trace one complete route from data to screen: the
  prologue, or the first quest in Cassardis. That is the M5 starting
  point, and a worked example is worth more than a broad list.
- **Live checks:** only through the oracle agent (board area `live/game`).
  Never attach to the game yourself.
- Coordinate with `codex-1` (asset-survey covers quests and NPCs broadly)
  and `codex-2` (text) on the board.

## Scope

- `docs/EVENTS.md` and `docs/QUESTS.md` (new)
- `crates/ddquest/**` (new, optional): typed readers, with install tests

## Deliverables

- The two docs: data formats, the runtime systems with exe addresses
  (traced or guessed), one fully worked example (a cutscene and a quest,
  from trigger to completion), and the list of story flags you could
  identify.
- A proposed implementation plan split into briefs.

## Done when

The worked examples are complete, or the gaps are named precisely. Fill
in Results and report to `head`.

## Results

claude-7, 2026-10-09, branch `task/events-quests` (commits 544fee9 ..
fccfdda). Docs: [EVENTS.md](../EVENTS.md), [QUESTS.md](../QUESTS.md).

**Built and tested** (`cargo test -p ddquest -- --nocapture`, 11 unit +
6 install tests):
- `ddquest::sdl`: rScheduler (SDL v19) reader from the loader 0x00db8e50;
  1,253 / 1,253 entries, 126,729 tracks, 204,938 keys.
- `ddquest::qct`: quest control tables from 0x00cd7200; 338 / 338, 2,510
  rows, typed judgment/result kinds traced in 0x0078ece0 / 0x0078fbb0.
- `ddquest::events`: the exe's 98-row event table (VA 0x01822d60) read
  from the player's own packed `DDDA.exe` (same bytes as the unpacked
  copy; build check by PE timestamp).
- `ddquest::order`: typed cFSMOrder orders (Flag, Event, SceNo,
  QuestFlag, SetLayout, BootQuest, Message, AreaHit, JumpArea, Fade,
  TimeJump, Camera, Finish), result-flag banks (0x005cc030), condition
  pretty-printer.
- `ddquest::story`: MSB-first flag banks (GSF 1,536 bits, 96 bits per
  quest x 128).
- Examples: `script_dump`, `script_survey`, `sdl_dump`, `flag_index`.

**Found** (addresses in the docs):
- The story is scripted by `cFSMOrder` state machines (546 quest and
  stage scripts, 70 order types registered at 0x005b7a60) plus QCT rows
  (one row per pass, disable after firing), stage scenarios (rScenario,
  11 task families) and SceNo, the scenario number (setter 0x0043d750,
  raise-only for scripts and events; full ladder 100 .. 9100 mapped to
  its setters).
- Events: script `Event` order -> aStage event mode 3 (0x0050be20 /
  0x0050c290 / 0x0050c7a0) -> sEventManager (0x018fa4b8) states 1-5 ->
  SDL timelines (main, light by hour bucket, msg/sound by sex) -> end:
  SceNo raised, party placed, control returned; skip 0x004367c0.
- Save layout: GSF at sGameSys +0xac6e4, quest bits at +0xac7a4 (95
  started / 94 cleared / 93 failed), layout flags at +0xadee8, SceNo +0x30.
- Worked examples, complete: the opening from new game (stage 380,
  SceNo 100) through the prologue, q0000 "Harbinger of Destruction",
  q0001 "Newly Arisen" (weapon table -> vocation menu stage mode 0x19 ->
  GSF 372; room exit -> event st200 ev10 -> SceNo 1200 -> QCT clear and
  rewards) to the boot of q0002 via an NPC script setting GSF 381; the
  st200 ev10 timeline frame by frame.

**Compared with the original** (live, read-only, claude-4's oracle):
GSF bit order (MSB-first), quest bits 95/94, q0001's bits 3/4, and the
SceNo band index (3400 -> 3) match the running game's save.

**Not done / open** (details in the docs): playback and interpretation
are not implemented; LCM camera reader; how `Message` ids pick a line;
subtitle language selection; scenario detection bits beyond the player;
the GPL field naming a group's layout flag; NPC think-script orders;
event table fields +0x8/+0xa and variant selectors; board and sudden
quests in detail.

**Next step**: the briefs proposed at the end of QUESTS.md, starting
with quest-runtime (headless interpreter for the opening) and
event-player (needs an LCM reader).
