# The opening slice

The project's current focus (maintainer, 2026-10-09). The aim is one
vertical slice of the real game, played start to finish in co-op, rather
than every system at once. Every system it needs is built in full,
driven by data, and checked against the original.

## Why a slice, and the rule that keeps it general

The opening exercises almost every core system:
- movement and animation;
- combat against several enemy families;
- a boss;
- cutscenes, movie and audio;
- character creation;
- pawns;
- quests;
- the HUD;
- field streaming.

Playing it end to end finds the integration and feel problems that
headless tests miss.

The risk of a slice is hard-coding: "if this is the dragon fight, do X".
The rule against it is simple. **A code path is specific to one event
only where the original exe is.** The original does have per-event
special cases (for example the event start code in EVENTS.md), and they
are copied with their addresses. Everything else is a general system fed
by the game's data:
- the event player plays any event;
- the quest runtime runs any quest script;
- the dragon fights through the enemy runtime and its think table.

When a step needs a system, build the system, not the step.

## The route

The order comes from QUESTS.md "Worked example: the opening" (traced),
extended past q0002.

| Step | Original | Status | Brief |
|---|---|---|---|
| 1 | New game: stage 380, SceNo 100 | traced | opening-prologue |
| 2 | Prologue, stage 380: the playable hero, fights (harpies among them), SceNo 200 → 900, event st380 ev00 | party setup and encounter facts merged; the chimera blocks (chimera-runtime) | opening-prologue |
| 3 | Prologue end → stage 800, q0000 boot, the movie `movie\event\st100ev00.wmv` (the beach and the dragon) | traced, not played | opening-movie |
| 4 | Stage 801: character creation | data readers (chara-edit), no UI | chara-creation-ui |
| 5 | Stage 200: st200 ev00 "It's the dragon!", the scripted fight "Challenge the Dragon", ev01 (heart taken), ev05 | dragon AI requests and the completion check merged; fight and host open | dragon-attack |
| 6 | q0001 "Newly Arisen": weapon table, vocation, ev10, Quina | **playable** (M5 opening slice, 0c291a6) | done |
| 7 | ev15, q0002 "Upon a Pawn": the first pawn, the goblins harassing a woman | q0002 boots; nothing after | upon-a-pawn |
| 8 | q0003: the road out of Cassardis to the Encampment (st210 ev00 at SceNo 2000) | field streaming and the stage handoff merged (STREAMING.md); q0003 untraced | road-to-encampment |
| 9 | Arrival at the Encampment, where the hydra's head is later brought | not started | road-to-encampment |

Steps 7–9 have not been traced yet: who the first pawn is, where the
goblin scene happens, and which stage is the Encampment. Each brief
starts by tracing its quest's scripts and QCT, like q0000–q0002.

## Systems the slice needs

| System | For steps | State |
|---|---|---|
| Movement feel: turn rate, locomotion blending, air attacks, falls | all | done (movement-feel) |
| Enemy runtime: goblins, harpies, wolves, the dragon, prologue enemies | 2, 5, 7, 8 | goblins done; wolves and harpies in progress (enemy-families) |
| Boss and scripted-fight support (fight checks, `EnemyCheck`, `mCntState`) | 5 | traced in part |
| Event player: faces, audio, the brow and ear defects | 2, 5–9 | playable; visual defects open (cutscene-visuals) |
| Movie playback (WMV) | 3 | not started |
| Character creation UI (`uGUIEdit`), applied to the body and stats | 4 | data only |
| Pawns: first pawn meeting, recruitment, the party | 7–9 | data, AI and knowledge; no recruitment flow |
| Quest runtime and log | all | q0000–q0002 headless |
| HUD | all | merged (game-hud) |
| Skill and hit VFX (`mteffect`) | 2, 5, 7 | readers only (skill-vfx) |
| Sound and music | all | event streams, voices, motion sounds and random selection merged; mixer and BGM open |
| Field streaming between cells | 8 | built (`ddworld::stream`, `ddview` stream.rs); the opening hookup is codex-26's |
| Co-op: guest dialogue and menus, event freeze | all | host-driven only |

## Acceptance

Each step's gate is the original's state after it:
- SceNo, GSF, quest words and the active stage, compared with a save or
  oracle read taken at the same point in the original;
- a capture comparison for its look.

The whole slice passes when two players on a LAN play steps 1–9 without
fast-forwards or teleports, and the end state matches the original's.
