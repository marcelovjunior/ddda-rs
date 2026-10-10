# Roadmap: systems

`MILESTONES.md` orders the work by playable route (M0-M10); the current
focus is the opening slice ([OPENING_SLICE.md](OPENING_SLICE.md)). This file
breaks the game into systems: for each, what exists, where its evidence
lives, and who works on it. The work items are in [`tasks/`](tasks/README.md),
and coordination happens on the board ([BOARD.md](BOARD.md)).

**Status** (updated 2026-10-10 afternoon):
- **done:** compared with the original;
- **working:** implemented and tested, not compared;
- **partial:** some of it is working, the rest open (see the brief);
- **researched:** rules and data traced, readers merged, no runtime;
- **not started.**

Owners: `head` (the coordinating Claude session) and the Codex lane
(`codex-1` and its children; on 2026-10-10 afternoon: codex-77 death/retry,
codex-78 pawn chatter, codex-80 beach dragon scene). The Claude subagents of
the first day are stopped. Step briefs S01-S56 are in `tasks/steps/`.

## Foundation and world

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Archives, textures, models, materials | done | FORMATS.md | - |
| Map geometry (cells, scenery, water, low models) | working | WORLD.md | castle and `_p` layouts |
| Field streaming (cells, collision, layouts, low models) | working | STREAMING.md | original's region far models, fade; head |
| Stage handoff (doors, JumpArea, start positions) | working (field + whole stages) | STREAMING.md, OPENING_SLICE.md | opening hookup done; guest streaming (codex-26) |
| Shading (terrain layers, lighting, fog, water) | working | SHADING.md | water shader |
| Collision (stage `h` meshes, scenery, walls, rays, materials) | working | MOVEMENT.md, WORLD.md | - |
| Weather, sky, fog, time of day | working (from a save's clock) | WEATHER.md, WEATHER_CLOCK.md | clock advance in play |
| Loading screens | not started | GUI.md | - |
| Enemy placement and respawn | partial (Cassardis families) | SPAWNS.md | enemy-families |

## The Arisen

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Movement (walk, run, sprint, jump, fall, hard landing) | working; partly done | MOVEMENT.md, RECORDINGS.md | drop-path trace |
| Turning | done (recorded with the oracle) | MOVEMENT.md | pad turning |
| Ledge grab and climb | working | MOVEMENT.md | live confirmation |
| Animation playback, skinning, blending | done (lengths live-confirmed) | ANIMATION.md | per-joint masks |
| Basic and air attacks | working | MOVEMENT.md | aim of the air shot |
| Stamina | working (rates live-confirmed) | STAMINA.md, PARAMS.md | - |
| Weight, encumbrance, body class | researched; measurement helper in progress | PARAMS.md, CHARA_EDIT.md | pawn-actor (codex-1), then head wires the editor |
| Character creation | working (S56: sex, name rules, 16 installed presets, confirm; per-profile EDIT save) | CHARA_EDIT.md, S56 | detailed editor pages, IME |
| Dressed Arisen (equipment models) | working (from saves) | CHARACTERS.md | - |
| Vocations and skills | partial (three base vocations' kits, four skills, vocation menu S35, level-up S43) | VOCATIONS.md | more skills, rank-up |
| Items and quick use | working (host-owned bags S51, item menu S38, new-game bag S28) | SHOPS.md, STATUS.md | equipment screen |
| Status effects | working (combat) | STATUS.md | - |

## Combat and creatures

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Hits, damage, stagger | working (push-out now swept against the ground) | COMBAT.md | - |
| Death, game over, retry | partial (S53: grounded death phases, atomic retry, vitals in saves; protocol 11) | S53, COOP_DESIGN.md | runtime/GUI (codex-77) |
| Enemy runtime: goblins | working (run/dash S45, perception S49, placement rates and exp, bare early looks S47) | AI.md | original comparison |
| Enemy runtime: wolves, harpies | partial | AI.md | enemy-families (codex-1) |
| Dragon (opening fight) | partial (AI, body, flight; fire coordinate/parent binding S30) | AI.md, S30, S31 | fire emission/damage (codex-1), beach scene (codex-80) |
| Chimera (prologue) | partial (catalogue, multi-part damage) | CHIMERA.md | codex-28 |
| Magic, shells | working (Ingle, Levin, WandShot) | COMBAT.md | more spells |
| Climbing large enemies | not started | - | - |

## Pawns, NPCs, story

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Rift, pawn recruitment, hire costs | researched (pool and costs live-confirmed) | PAWNS.md, RIFT_UI.md, RIFT_LIST.md | Riftstone is met in q0003 |
| Pawns as actors (stats, equipment, tiers) | partial | pawn-actor brief | body, Mage AI (codex-1) |
| Pawn AI | working (main pawn fights; orders Go/Come/Help S36; chatter tables, recognition, speech S52) | PAWN_AI.md, S52 | inclinations, chatter GUI history |
| NPCs: placement, schedules, applied scripts, talk | partial (guard talk, authored motions, Reynard combat/HP/gauge veto, beach Demo actors) | NPCS.md | Demo reactions/GotoEx (codex-80) |
| Quests, story flags, scenario ladder | working (q0001-q0002 played on the route; q0003 boots; quest scripts saved S48) | QUESTS.md, OPENING_SLICE.md | q0003 to the Encampment (after the user's playtest) |
| Cutscenes (timelines, cameras, voices) | working (quest cameras incl. the Reynard close-up S40, LAN S44); visual defects open | EVENTS.md, CAMERA.md | cutscene-visuals |
| Movies | working (`--movie`, opening intro) | opening-movie brief | skip input |
| Shops, prices, restock, enhancement | researched (layout live-confirmed) | SHOPS.md | shop screen |
| Interaction lock (co-op) | designed | COOP_DESIGN.md | - |

## Presentation

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Visual effects | researched (readers; particle bodies opaque) | EFFECTS.md | skill-vfx: decode EFL bodies (next free Codex child) |
| Sound and music | partial (events, voices, motion sounds) | SOUND.md | mixer, field BGM (codex-1) |
| HUD | working (bars, prompts, enemy/NPC HP gauges S34, pawn speech S52) | GUI.md | icons, minimap |
| Menus | partial (pause, vocation S35, level-up S43, items S38, graphics settings S42) | GUI.md, RIFT_UI.md | equipment, status, Rift |
| World map, fast travel | researched | MAP.md, TRAVEL.md | - |
| Game text (all languages) | done (reader) | TEXT.md | - |

## Multiplayer, platform, tooling

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Co-op (LAN, host-authoritative) | working (two windows; opening slice; protocol 10: pawn orders, quest cameras, NPC state, speech; profiles park/rejoin S55) | COOP_DESIGN.md | protocol 11 retry/death (codex-77), guest camera for recognition |
| FPS unlock | working (fixed steps, drawn between frames) | MOVEMENT.md, RECORDINGS.md | - |
| Frame rate (180 fps target, 5.5 ms) | partial (S41: field ~10.6 ms; compact skin palettes; finite shadow extent opt-in) | S41 | bind groups, shadow binning (codex-1) |
| Saves | working (own host saves: economy, quest scripts, profiles, EDIT, vitals; atomic load; route checkpoints S46) | PAWNS.md, SAVE_PAWNS.md | original-save import |
| Oracle (live probe) | working | ORACLE.md | - |
| Reflected field names | working | `research/dti/dti_props.py` | - |
| Backups | working (bundles in `C:\ddda-backup`) | - | head after each batch |
