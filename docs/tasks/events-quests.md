# events-quests: cutscenes, events, quests and story flags

- **Owner:** claude lane
- **Branch:** `task/events-quests`
- **Board area:** `research/events`
- **Status:** open

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

(filled in by the agent)
