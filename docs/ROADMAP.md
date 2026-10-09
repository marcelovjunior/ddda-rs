# Roadmap: systems

`MILESTONES.md` orders the work by playable route (M0-M10); the current
focus is the opening slice ([OPENING_SLICE.md](OPENING_SLICE.md)). This file
breaks the game into systems: for each, what exists, where its evidence
lives, and who works on it. The work items are in [`tasks/`](tasks/README.md),
and coordination happens on the board ([BOARD.md](BOARD.md)).

**Status** (updated 2026-10-09 evening):
- **done:** compared with the original;
- **working:** implemented and tested, not compared;
- **partial:** some of it is working, the rest open (see the brief);
- **researched:** rules and data traced, readers merged, no runtime;
- **not started.**

Owners: `head` (the coordinating Claude session) and the Codex lane
(`codex-1` and its children). The Claude subagents of the first day are
stopped.

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
| Character creation | partial (`--charaedit`, fields and presets) | CHARA_EDIT.md | uGUIEdit screen, ranges; chara-creation-ui |
| Dressed Arisen (equipment models) | working (from saves) | CHARACTERS.md | - |
| Vocations and skills | partial (four skills) | VOCATIONS.md | skills-runtime follow-ups |
| Items and quick use | working (restoratives) | SHOPS.md, STATUS.md | inventory screen |
| Status effects | working (combat) | STATUS.md | - |

## Combat and creatures

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Hits, damage, stagger | working | COMBAT.md | - |
| Enemy runtime: goblins | working | AI.md | - |
| Enemy runtime: wolves, harpies | partial | AI.md | enemy-families (codex-1) |
| Dragon (opening fight) | partial (AI requests, body, hover, flight) | AI.md, dragon-attack brief | codex-25 |
| Chimera (prologue) | partial (catalogue, multi-part damage) | CHIMERA.md | codex-28 |
| Magic, shells | working (Ingle, Levin, WandShot) | COMBAT.md | more spells |
| Climbing large enemies | not started | - | - |

## Pawns, NPCs, story

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Rift, pawn recruitment, hire costs | researched (pool and costs live-confirmed) | PAWNS.md, RIFT_UI.md, RIFT_LIST.md | Riftstone is met in q0003 |
| Pawns as actors (stats, equipment, tiers) | partial | pawn-actor brief | body, Mage AI (codex-1) |
| Pawn AI | working (main pawn fights) | PAWN_AI.md | inclinations, chatter |
| NPCs: placement, schedules, applied scripts, talk | partial | NPCS.md | guard/talk and variables (codex-26) |
| Quests, story flags, scenario ladder | working (q0000-q0002 headless; q0003 outlined) | QUESTS.md, OPENING_SLICE.md | q0003 trace |
| Cutscenes (timelines, cameras, voices) | working; visual defects open | EVENTS.md, CAMERA.md | cutscene-visuals |
| Movies | working (`--movie`, opening intro) | opening-movie brief | skip input |
| Shops, prices, restock, enhancement | researched (layout live-confirmed) | SHOPS.md | shop screen |
| Interaction lock (co-op) | designed | COOP_DESIGN.md | - |

## Presentation

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Visual effects | researched (readers; particle bodies opaque) | EFFECTS.md | skill-vfx: decode EFL bodies (next free Codex child) |
| Sound and music | partial (events, voices, motion sounds) | SOUND.md | mixer, field BGM (codex-1) |
| HUD | working (bars, prompts) | GUI.md | names, icons, minimap |
| Menus | not started | GUI.md, RIFT_UI.md | - |
| World map, fast travel | researched | MAP.md, TRAVEL.md | - |
| Game text (all languages) | done (reader) | TEXT.md | - |

## Multiplayer, platform, tooling

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Co-op (LAN, host-authoritative) | working (two windows; opening slice) | COOP_DESIGN.md | guest dialogue and menus |
| FPS unlock | working (fixed steps, drawn between frames) | MOVEMENT.md, RECORDINGS.md | - |
| Saves | researched (read-only reader) | PAWNS.md, SAVE_PAWNS.md | our own saves |
| Oracle (live probe) | working | ORACLE.md | - |
| Reflected field names | working | `research/dti/dti_props.py` | - |
| Backups | working (bundles in `C:\ddda-backup`) | - | head after each batch |
