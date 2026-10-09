# Roadmap: systems

`MILESTONES.md` orders the work by playable route (M0 foundation, M1 look,
M2 walk, M3 co-op, M4 fight, M5+ campaign). This file breaks the game into
systems, so several agents can work in parallel. Each system lists what
exists, where its evidence lives, and what it depends on. Work items are
briefs in [`tasks/`](tasks/README.md).

Status: **done** (compared with the original), **working** (implemented
and tested, not yet compared), **started**, **researched** (data/exe
located), **not started**.

Lanes say who should take a system:
- **core**: touches `ddsim` / `ddview` / `ddworld` internals. Claude's
  lane (the coordinator merges these).
- **reader**: a new format reader or data survey in its own crate or
  doc. Low conflict; good for any agent.
- **exe**: tracing rules in Ghidra, output a doc section and constants.

## Foundation and world

| System | Status | Lane | Evidence / notes | Depends on |
|---|---|---|---|---|
| Archives, textures, models, materials | done | - | FORMATS.md; scans over the whole install | - |
| Map geometry (cells, scenery, water, distant ring) | working | core | WORLD.md; castle in ref001 missing, `_p` layouts unread | - |
| Shading (terrain blending, lighting, fog, water) | started | core | MRL constant buffers unparsed; MFX feature switches known | map geometry |
| Collision | working | core | `h` merged meshes + `_h00` scenery (MOVEMENT.md, WORLD.md) | - |
| Day/night, weather | not started | exe + reader | sky/light params likely XFS under `scr\`, `rEnvironment*`? | shading |
| Loading screens / streaming | not started | core | cell streaming = `cSplitSbc` / split loader `0x00C5F680` | map geometry |

## The Arisen

| System | Status | Lane | Evidence / notes | Depends on |
|---|---|---|---|---|
| Movement (walk, run, sprint, jump, fall) | working | core | MOVEMENT.md; speeds from `m_pl000_00`, jump from `pljumpparam` | - |
| Ledge grab and climb | started | core + exe | placeholder; `cPlCliffHangBegin::vf05` 0x00AD9180 | collision |
| Turn rate, stop motions | not started | core + exe | recorded ~70°/s; `cPlActRunEnd` | movement |
| Animation (playback, skinning) | working | core | ANIMATION.md, `mtlmt` | - |
| Animation blending | not started | core | the original blends (FUN_00b80260's second arg = blend frames?) | animation |
| Stamina | researched | exe + reader | `param\pl\stamina\plstamina_*` (rPlStamina, per weapon and action) | movement |
| Weight / encumbrance | researched | exe + reader | `param\pl\weight\pltotalweight{ss,s,m,l,ll}`, `pleditweight` | inventory, customization |
| Character customization (size, weight, effects) | not started | exe + reader | body model `model\pl\m\m_base\m000`; height factor? speed factor uPlayer+0xDA8 (0.847) untraced | weight |
| Dressed Arisen (face, hair, equipment models) | not started | core | equipment models under `model\pl\` | animation |
| Vocations (classes), levels, skills | researched | reader + exe | `param\pl\level\lv*`, `param\pl\joblevel\joblv*`, per-weapon motion banks (`m000N_co/at`) | combat |
| Inventory, items | not started | reader | item tables not located yet | - |
| Status effects | not started | exe + reader | `rStatusParam` (XFS), Riftstone notes | combat |

## Combat and creatures

| System | Status | Lane | Evidence / notes | Depends on |
|---|---|---|---|---|
| Melee combat, hit detection | not started | exe + reader | `rObjCollision` (.ocl) shapes + attack data (Riftstone notes) | animation |
| Magic (spells) | not started | exe + reader | `mag_item_act_param`, shot/bullet params | combat, vfx |
| Enemy data (stats) | not started | reader | `rPropParam` (PRPZ), `rRegionStatus`, `rShlParamList` | - |
| Enemy AI | not started | exe + reader | `rAIFSM` (XFS, 4,385 files), `rAIGoalPlanning` | enemy data |
| Climbing large enemies | not started | exe | grab data location unknown | combat |

## Pawns, NPCs, story

| System | Status | Lane | Evidence / notes | Depends on |
|---|---|---|---|---|
| Pawn AI / scripting (inclinations, chatter) | not started | exe + reader | `rAIFSM`, `sPawnManager` | enemy AI |
| NPCs (placement, schedules, dialogue) | not started | reader | NPC layouts and message files not surveyed | map geometry |
| Quests, events, cutscenes | not started | reader + exe | quest XFS, event scripts not surveyed | NPCs |
| Interaction lock (co-op) | designed | core | COOP_DESIGN.md | NPCs, co-op |

## Presentation

| System | Status | Lane | Evidence / notes | Depends on |
|---|---|---|---|---|
| VFX (effects) | not started | reader | effect lists (`rEffectList`, `.efl`) not surveyed | shading |
| Sound and music | not started | reader | sound banks not surveyed | - |
| UI, HUD, menus, settings | not started | reader | GUI formats (`rGUI*`) not surveyed | - |
| World map, fast travel (portcrystals, ferrystones) | not started | reader + exe | map GUI and portcrystal data not located | UI |

## Multiplayer and platform

| System | Status | Lane | Evidence / notes | Depends on |
|---|---|---|---|---|
| Co-op (LAN, host-authoritative) | designed | core | COOP_DESIGN.md; `ddsim` already N-player and fixed-step | movement |
| FPS unlock | working by design | core | simulation steps 120 Hz independent of render rate | - |
| Settings, saves | not started | reader | save format not surveyed | inventory, quests |
