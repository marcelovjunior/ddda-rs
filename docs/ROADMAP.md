# Roadmap: systems

`MILESTONES.md` orders the work by playable route (M0-M10). This file
breaks the game into systems: for each, what exists, where its evidence
lives, and who works on it. The work items are in [`tasks/`](tasks/README.md),
and coordination happens on the board ([BOARD.md](BOARD.md)).

**Status** (updated 2026-10-09):
- **done:** compared with the original;
- **working:** implemented and tested, not compared;
- **researched:** rules and data traced, readers merged, no runtime;
- **in progress:** a brief is assigned;
- **not started.**

## Foundation and world

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Archives, textures, models, materials | done | FORMATS.md | - |
| Map geometry (cells, scenery, water, distant ring) | working | WORLD.md | castle and `_p` layouts |
| Shading (terrain layers, lighting, fog, water) | in progress | SHADING.md | terrain-shading (claude-3) |
| Collision (stage `h` meshes, walls, rays) | working | MOVEMENT.md, WORLD.md | - |
| Weather, sky, fog, time of day | researched | WEATHER.md, WEATHER_CLOCK | runtime after M1 shading |
| Streaming and loading screens | in progress | GUI.md | stream-rules (codex lane) |
| Enemy placement and respawn | in progress | - | enemy-placement (codex lane) |

## The Arisen

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Movement (walk, run, sprint, jump, fall) | working; partly done | MOVEMENT.md, RECORDINGS.md | m2-acceptance (claude-4) |
| Ledge grab and climb | working | MOVEMENT.md | live confirmation (claude-4) |
| Turning, stop motions | researched | MOVEMENT.md, RECORDINGS.md | needs motion curves |
| Animation playback, skinning | working | ANIMATION.md | - |
| Animation blending | done (lengths and countdown live-confirmed) | ANIMATION.md | per-joint masks |
| Stamina | researched (rates live-confirmed) | PARAMS.md, RECORDINGS.md | runtime in ddsim |
| Weight, encumbrance, body class | researched | PARAMS.md | chara-edit (codex lane) |
| Character creation and its effects | in progress | - | chara-edit (codex lane) |
| Dressed Arisen (equipment models) | not started | - | - |
| Vocations, skills, augments, growth | in progress | - | vocations-skills (claude-9) |
| Items, inventory, storage | researched | SHOPS.md | economy-core after ddstory |
| Status effects | researched (readers) | ENEMIES.md | after combat-rules |

## Combat and creatures

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Hits, damage, stagger | in progress | - | combat-rules (claude-8) |
| Enemy data (stats, regions, shells) | researched | ENEMIES.md, ENEMY_RESOURCES.md | - |
| Enemy AI (FSM graphs, interpreter) | researched | AI.md | runtime after M4 design |
| Magic, shells | researched (readers) | ENEMIES.md, EFFECTS.md | combat-rules |
| Climbing large enemies | not started | - | - |

## Pawns, NPCs, story

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Rift, pawn recruitment, hire costs | researched (pool and costs live-confirmed) | PAWNS.md, RIFT_UI.md | pawn-builder (claude-5); rift-sim later |
| Pawn data in saves | researched | PAWNS.md, SAVE_PAWNS.md | pawn-knowledge-map (codex lane) |
| Pawn AI, inclinations, chatter | not started | AI.md | after enemy AI |
| NPCs: placement, schedules, dialogue links | researched | NPCS.md | - |
| Quests, story flags, scenario ladder | researched (flags live-confirmed) | QUESTS.md | quest-runtime (claude-7) |
| Cutscenes (timelines, cameras) | researched | EVENTS.md, CAMERA.md | event-player after quest-runtime |
| Shops, prices, restock, enhancement | researched (layout live-confirmed) | SHOPS.md | enhance (claude-6) |
| Interaction lock (co-op) | designed | COOP_DESIGN.md | M3 |

## Presentation

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Visual effects | researched (readers; particle bodies opaque) | EFFECTS.md | - |
| Sound and music | researched (readers) | SOUND.md | playback later |
| UI, HUD, menus | researched (readers) | GUI.md, RIFT_UI.md | - |
| World map, fast travel | researched | MAP.md, TRAVEL.md | teleport runtime later |
| Game text (all languages) | done (reader) | TEXT.md | - |

## Multiplayer, platform, tooling

| System | Status | Docs | Next / owner |
|---|---|---|---|
| Co-op (LAN, host-authoritative) | designed | COOP_DESIGN.md | M3 |
| FPS unlock | working by design | RECORDINGS.md (the clock) | - |
| Saves | researched (read-only reader) | PAWNS.md, SAVE_PAWNS.md | - |
| Oracle (live probe) | working | ORACLE.md | claude-4 |
| Reflected field names | working | `research/dti/dti_props.py` | - |
| Acceptance routes | in progress | - | m2-acceptance (claude-4) |
