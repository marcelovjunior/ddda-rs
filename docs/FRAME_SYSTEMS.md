# Frame systems: unit lines, managers, the story, pause (notes)

These are notes from a tracing fork of the host-architecture brief,
saved by head when the brief was paused. All of it is static, from the
2017-12-13 Steam exe; anything marked **(inferred)** is a guess. It
belongs in FRAME.md once claude-10 resumes.

## Unit lines

**Adding a unit to a line:**
- `0x00DB5800` adds at the head, `0x00DB58B0` at the tail, both with
  arguments (line, unit, group_lo, group_hi). They write the line into
  unit+4 bits 3..9 (0x7F means none), then set the group mask through
  sUnit vf19.
- `0x00DB6BB0` removes a unit.
- Creators that pick the line:
  - `0x004BA540`: by DTI, line in eax;
  - `0x00421FE0`: characters;
  - `0x0049DA10`: objects;
  - `0x00433E00`: enemies. If the id is in a 37-entry list, it uses
    line − 9.

| Class | Line | Group |
|---|---|---|
| uFSMTask, uFSMQuest (story scripts) | 0 Pre Process | 16 only |
| rScheduler, uSchedulerExt | 1 Scheduler | 17 |
| uWater | 2 | |
| stage models, uStageSplitCtrl, sky grass | 4 / 5 Scr | 20 |
| uPawnIntel | 9 | 28 |
| uPlayer | 10 Player | 25 |
| uCmc (pawns) | 10 Player | 28 |
| uNpcIntel | 12 | 30 |
| uNpc, uHumanEnemy, uEnemy | 15 Enemy (listed ids: 6) | 30 / 31 |
| enemy parts | 16 / 17 | 32 |
| shells, uOm objects, set coords | 18 Obj2 | 53 / 34 / 35 |
| uPointShadowExt | 20 | |
| cameras (uCameraCtrl, uFreeCamera) | 21 Camera | 37 |
| sky | 22 | 38 |
| lights | 25 | 41 |
| HUD | 26 / 27 | 42 |
| menus | 28-32 | 44-47 |
| fade, loading, sound | 33 | 49 |
| fade schedulers | 34 | 50 |
| effects | 36 Post Process | 39 |

Effect units attached to an owner go on the owner's line + 1.

**The passes:**
- **Pass A**, `0x00DB6740`:
  1. free deferred deletes;
  2. run line vf09;
  3. for each unit: state 1 calls slot 5 once, then slot 8 (move)
     each frame; state 3 becomes 4; state 4 checks slot 17, then removes
     the unit.
- **Pass B** (sUnitExt vf13) calls unit slot 10.
- **Pass C** (vf11 → `0x004B99D0`) calls slot 9.

A line with flag bit 0 runs its units as parallel jobs. Each unit's
delta (+0x14) is `mDeltaTime` × the line's rate.

## Managers

1. **sGameSys vf06** `0x0043CEB0`:
   - writes +0xB879C = `mDeltaTime` / `mFps` (the saved-clock step);
   - writes the game delta at +0xB87A0, which is 0 when +0xB8844 bit
     0x80000 is set;
   - times out a talk request after 30 s;
   - unless paused, runs `0x0078B760`, `0x0043D2B0`, `0x0043D4F0`,
     `0x004403A0`, `0x00441650`, `0x00441A60`, `0x00441C30` /
     `0x00441D40`, the clock `0x0043F2C0`, `0x0044A350`, `0x00437420` and
     `0x00442900`.
2. **sAreaExt vf06** `0x00419910` → sArea vf06 `0x00DB54D0`. It walks the
   area stack (+0x382C, up to 8, root first): slots 5 and 6 once, slot 7
   every frame. A pending area change (+0x3828) is applied by
   `0x00DB4F10`. The story runs here (next section).
3. **sAI vf06** `0x00F795C0`: slot 6 on each AI object whose flag bit 0 is
   clear, then parallel jobs. The second pass `0x00F79650` →
   `0x00F79210(1)` runs after unit pass C.
4. **sAreaHit vf06** `0x0041A2C0`: slot 7 on each active area hit (the
   story's AreaHit).
5. **sZone vf06** `0x00F74590`: zone listeners.
6. **sWorkRate vf06** `0x004C5FC0`: six timed rate slots (hit-stop and
   slow motion); each resets to 1.0 when its timer ends.
7. **sRecognition vf06** `0x00487B00`: rebuilds the enemy list on an
   interval.
8. **sObjCollision** (`0x00479770`, `0x004792C0`, `0x004797E0`,
   `0x00479CB0`, vf06 `0x00478AB0`): body contact and pushing between
   characters, not attack hits **(inferred)**.
9. **Others:**
   - `0x00413C40`: frees sPlayerManager per-frame request queues;
   - `0x00498300`: the sSave cooldown;
   - `0x00471D80`: NPC intel culling;
   - `0x00455280`: the message queue;
   - `0x00451F80`: the HUD;
   - `0x00420910` → sCamera vf06 `0x00DC4830`: cameras;
   - `0x004AD440`: sound;
   - `0x004C0C70`: weather (clock → weather → zone light);
   - `0x00467F50`: network;
   - `0x00CA23F0`: Capcom protocol.

## Where the story runs (one frame, in order)

1. **aGameMain slot 7** `0x004FB960`:
   - `0x004FB7B0` (one-shot new game / load);
   - then slot 6 of the managers: enemies, human enemies, players, the
     ScrOm manager, NPC intel **(inferred)**, sNpcManager, shells,
     **sQuestManager** (where the QCT runs: `0x00482DD0` → `0x0078C830`
     → `0x007922F0`), **sEventManager**, lock-on, the AI boards,
     **sSetManager** (spawning) and sound.
2. **aStage slot 7** `0x00504330`:
   - play time += dt (capped at 216000);
   - a switch on the stage mode: 2 = event `0x0050B870`; 3 = event mode
     3; and so on up to 0x2C;
   - then **sScenario** vf06 `0x0049A0E0` → `0x00797DF0`.
3. **Unit pass A, line 0:** uFSMTask and uFSMQuest slot 8 → the cAIFSM
   driver `0x00E092A0`. Gated by `0x0044D430`: no scripts run during
   stop flags, talk, or stages 0x321-0x324.

So the order is QCT → stage mode / events → scenarios → scripts.
**(Inferred)**: aGameMain is aStage's parent in the area stack; where
areas are registered was not found. QUESTS.md currently assumes
scenarios → QCT → scripts.

## Pause and unit groups

- **`sMain+0x6C` (`mPause`)** is a capture/benchmark pause only.
  `sMain+0x68` is a frame-step counter. The paused state stops the three
  unit passes, sAI, sAreaHit, sWorkRate, sRecognition, sObjCollision and
  sGameSys's main body.
- **The group mask `sUnit+0xD30`:** the only writer is
  `0x00DB5A60(bit, enable)`.
  - Bit 57 is cleared by `0x0043F6C0` during events and menus: the
    player, pawns, NPCs, enemies, objects, shells, stage models, sky and
    HUD all freeze.
  - Scripts (group 16), schedulers (17), fade and sound (49/50) and menus
    (44-47) keep running.
  - The story scripts still stop, through the `0x0044D430` gate.
  - Other bits: 60 (`0x0043F7E0`), 58 (`0x0043F990`), 55/59 (`0x00514A90`,
    `0x00516460`).

## Open

- The line used by each effect type.
- Where the area stack is registered.
- The runtime line rates (sUnit+0x28 / +0x38).
