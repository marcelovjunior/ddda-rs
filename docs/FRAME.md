# The original's frame

What DDDA.exe (2017-12-13 Steam) runs each frame, and in which order. It
is the evidence for the host's system order ([ADR 0002](adr/0002-game-host.md)).

**Sources:**
- **[exe]**: Ghidra, read-only. Function addresses as in the rest of the
  repo.
- **[live]**: claude-4's oracle, read-only, 2026-10-09 (board #872; raw
  JSON in `E:\ddda-re\notes\oracle\frame-order\`, private).
- *Inferred*: reasoned, not traced; marked as such.

Singleton fields of `sBbsRpgMain` were resolved from its constructor
(0x0041B400), which stores each `new`ed singleton into the object. Every
field was then confirmed live by its object's vtable.

## 1. The main loop (`0x00DF0D20`)

`sApp` is the global at 0x018D0F50; its `+0x2558` holds the `sMain`
object, a `sBbsRpgMain` [exe]. Each iteration:

1. Pumps window messages with `PeekMessageA`, `TranslateMessage` and
   `DispatchMessageW`. It exits on `WM_QUIT` or `sApp+0x266c`.
2. On a change of `sApp+0x2566` (window active), calls `sMain` vf09
   (activate) or vf10 (deactivate).
3. If `sApp+0x20` (run in background) and `sApp+0x2566` are both 0, calls
   `sMain` vf11 and `Sleep(1)`: no frame.
4. Otherwise:
   - checks the D3D device (`0x00DAF7C0`, `TestCooperativeLevel`);
   - sleeps instead if the device is lost (`sRender+0x433e68`);
   - else runs **one frame**: `sMain` vf06 = `sBbsRpgMain::vf06`
     (0x0041DD60).

There is no fixed-rate loop. One iteration is one frame at whatever rate
the machine reaches (`mMaxFps` 150; RECORDINGS.md). The step is measured
inside the frame (section 3).

## 2. One frame: `sBbsRpgMain::vf06` (`0x0041DD60`)

The calls in order [exe]. `vfNN` is a virtual slot of the singleton
named; "(empty)" marks a slot that is the shared `return` at 0x005F35C0.
The groups and their titles are this document's.

### A. Frame start

| # | Call | What |
|---|---|---|
| 1 | `0x00E231C0` (if `sRender+0x433e64`) | render-target upkeep |
| 2 | `0x00FA9790` | `IsDebuggerPresent` check |
| 3 | `+0x1c274 += 1` | frame counter |
| 4 | `mDeltaTimeBorder` / `mDeltaTimeLimite` (+0x74 / +0x78) | 2.0 / 2.0; 100.0 / 16.0 while `sGameSys+0xb8844` has bit 0x2000000 or 0x4000000 |
| 5 | `SteamAPI_RunCallbacks` | |
| 6 | `sMain::vf06` 0x00DBFA90 | **the clock**: QPC timer, `mDeltaTime` (+0x70), job threads (+0x98 from +0x9c) |

### B. Persistence and world origin

| # | Call | What |
|---|---|---|
| 7 | `sSave::vf06` 0x00498300 | save system |
| 8 | `sStorageExt::vf06` 0x004B7870 | storage (device) |
| 9 | `sResourceExt` vf09, `sToolExt` vf09 | (empty) |
| 10 | `sWorldOffset::vf06` 0x004C64E0 | floating origin |
| 11 | `sEffect::vf09` 0x00FB5E00 | effects, pre-frame |
| 12 | `0x00DEF940` | |
| 13 | `sZone` vf09 | (empty) |
| 14 | `0x00E76A50` | |
| 15 | `sPrimitive::vf09` 0x00E60D40, `sGpuParticle::vf09` 0x00EB26C0 | |
| 16 | `0x00DA9CD0` | |
| 17 | `sGUI::vf10` 0x00DE11A0 | GUI, pre-frame |

### C. Game state and the area

| # | Call | What |
|---|---|---|
| 18 | `sGameSys::vf06` 0x0043CEB0 | game system (section 6) |
| 19 | `0x004883C0` | |
| 20 | `sAreaExt::vf06` 0x00419910 | the area stack (`aBbsRpg`, `aGameMain`, `aStageNNN`; ORACLE.md) |
| 21 | `0x00E76140` | |

### D. Input

| # | Call | What |
|---|---|---|
| 22 | `sVibrationExt::vf06` 0x004BA840 | rumble |
| 23 | `sQlocInput::vf06` 0x00409EE0 | game buttons (pad and keyboard mapping) |
| 24 | `sKeyboard::vf06` 0x00DF40E0 | keyboard state (ORACLE.md) |
| 25 | `sMouse::vf06` 0x00E04F20 | mouse |

All input is read here, before any unit moves.

### E. Before the units

| # | Call | What |
|---|---|---|
| 26 | `sShader2Ext::vf06` 0x004AB320 | |
| 27 | `sAreaHit::vf06` 0x0041A2C0 | area hit volumes |
| 28 | `sZone::vf06` 0x00F74590 | |
| 29 | `sItemManager::vf06` 0x0045CF60 | |
| 30 | `sAI::vf06` 0x00F795C0 | AI system |
| 31 | `sWorkRate::vf06` 0x004C5FC0 | time-scale rates |
| 32 | if not paused (`sMain+0x68 != 0` or `sMain+0x6c == 0`): reset of `sObjCollision` counters, `0x00479770(0)` | |
| 33 | `0x004792C0` | |
| 34 | `sRecognition::vf06` 0x00487B00 | |
| 35 | `sPawnManager` vf06 | (empty) |

### F. The units (section 3)

| # | Call | What |
|---|---|---|
| 36 | `sUnitExt::vf06` 0x004B97C0 → `sUnit::vf06` 0x00DB6740 | **pass A: every unit's move**, line by line |
| 37 | `sCollisionExt::vf06` 0x00422DC0 | collision |
| 38 | `0x004797E0(sObjCollision)` | |
| 39 | `sGUIExt` vf06 | (empty) |
| 40 | `sUnitExt::vf13` → `sUnit::vf13` 0x00DB5700 | **pass B**, line by line |
| 41 | `0x00413C40` | |
| 42 | `sUnitExt::vf11` → `sUnit::vf11` 0x00DB5690 | **pass C**, line by line |

### G. After the units

| # | Call | What |
|---|---|---|
| 43 | `0x00F79650` | |
| 44 | `sNpcIntelManager::vf10` 0x00471D80 | |
| 45 | `sIdMessage::vf06` 0x00455280, `sIdCockpit::vf06` 0x00451F80 | HUD messages, cockpit |
| 46 | `0x00498370`, `0x00479CB0`, `0x00ED5350` | |
| 47 | `sObjCollision::vf06` 0x00478AB0 | |
| 48 | `sSoftBody` vf06, `sGeneralPoint` vf06 | (empty) |
| 49 | `sAISensorTarget::vf06` 0x00415220 | |
| 50 | `sEffectExt::vf06` 0x004258F0 | effects |
| 51 | `sCameraExt::vf06` 0x00420910 | cameras |
| 52 | `sSoundExt::vf06` 0x004AD440 | sound |
| 53 | `sWeatherManager::vf06` 0x004C0C70 | weather |
| 54 | `sNetworkExt::vf06` 0x00467F50, `sDDCaProto::vf06` 0x00CA23F0 | online (pawn sharing) |
| 55 | `sBenchmark::vf06` 0x00FA0920 | |

### H. Rendering and streaming

| # | Call | What |
|---|---|---|
| 56 | `0x00F5A200` | |
| 57 | `sSceneExt::vf06` 0x0049B030 | scene |
| 58 | `sShadow::vf06` 0x00E277A0 | shadows |
| 59 | `sCollision::vf09` 0x00DEECF0 | |
| 60 | `sRender::vf06` 0x00DAF000 | render |
| 61 | `sGpuParticle::vf10` 0x00EB38A0, `0x00DAED10`, `0x00E76160` | |
| 62 | `sArchiveManager::vf06` 0x00417B50 | archive loading |
| 63 | `sEnvMapManager::vf06` 0x004354E0 | |
| 64 | `sCapture::vf06` 0x00F98AF0 | |
| 65 | `0x004B9660(sUnit)` | queues every unit's draw as jobs |

Many singletons have no call here:
- `sQuestManager`, `sScenario`, `sEventManager`, `sSetManager`,
  `sNpcManager`;
- `sPlayerManager`, `sEnemyManager`;
- `sLockOnManager`, `sShlManager`.

They run inside these calls or as units (sections 4 and 6).

## 3. `sUnit`: lines, passes, jobs, groups, pause

`sUnit` (global 0x018D09E0; `sUnitExt`, ctor 0x004B94C0) holds **37 move
lines**. Their names are at 0x018753A8 (section 4); the count is at
`+0x620`. Each line record is 6 dwords at `+0x24 + 0x18 * i`:

| Offset | Field |
|---|---|
| +0x0 | name |
| +0x4 | flags |
| +0x8 | first unit |
| +0xc | last unit (*inferred*) |
| +0x10 | rate (f32) |
| +0x14 | |

Units of a line are a list through `unit+0xc`.

**The three passes** [exe]. Each pass walks lines 0..36 in order:
- **Pass A** (`sUnit::vf06` → per line vf09: `sUnitExt::vf09` 0x004B97F0
  or `sUnit::vf09` 0x00DB7350). For each unit:
  1. sets `cUnit.mDeltaTime` (+0x14) to line rate × `sMain.mDeltaTime`
     (0x00DB7384-0x00DB73D5);
  2. runs the unit's state machine on `unit+0x4 & 7`:
     - 1, new: becomes 2 and calls slot 5 (+0x14, the unit's init);
     - 3: becomes 4;
     - 4: the unit is detached and deleted, or parked (slot 17 decides);
  3. a unit in state 2 with the Move flag (0x400) and its groups enabled
     calls **slot 8** (+0x20, "move").
- **Pass B** (`sUnit::vf13` → per line vf12 0x00DB6890): **slot 10**
  (+0x28) of the same units.
- **Pass C** (`sUnit::vf11` → per line vf10 0x00DB67B0): **slot 9**
  (+0x24) of the same units.

**Groups:** a unit runs only if its `cUnit.mUnitGroup` (+0x18, 64 bits)
is inside `sUnit+0xd30` (the enabled-group mask):
`~mask & group == 0` (0x00DB5B30) [exe]. Live the mask was all ones
during play [live].

**Line flags:**
- bit 1 skips the line;
- bit 0 queues the line's units as **jobs** (0x00478560 into `sMain+0x8c`
  / `+0xc0`) instead of calling them. `0x00DBE560` then runs the queued
  jobs on `sMain`'s worker threads and waits for all of them before the
  next line [exe].

Live, every line's flags were 0x1FD (bit 0 set, bit 1 clear) and every
rate was 1.0 [live]. So units on one line move concurrently, and lines
run one after another.

**Pause** [exe]: every pass, and `sUnitExt`'s line move, returns at once
while `sMain+0x6c` (`mPause`) is set and `sMain+0x68` is 0. `sMain::vf06`
counts `+0x68` down by one each paused frame, so a pause can let N more
frames run.

**Line order table** [exe]: `sUnitExt` can reorder lines through a table
at `sUnit+0x19f374` (0x004B97F0, 0x00CC68C0: line dependencies with an
"Algorithm error." check). Live the table pointer was 0, so lines ran in
index order [live].

## 4. The 37 move lines

Names from 0x018753A8 [exe]. Contents from one live walk during play in
the field at night (classes with counts; total units in brackets) [live].

| # | Line | Units seen |
|---|---|---|
| 0 | Pre Process | [23] uFSMTask 8, uSoundSequenceSe 6, uResidingCallDraw 5, **uFSMQuest 3**, uInfiniteLight 1 |
| 1 | Scheduler | [5] uScheduler |
| 2 | Scheduler Sub | [3] uScheduler |
| 3 | Filter | [6] screen filters (DOF, color correction, god rays, water reflection) |
| 4 | Scr | [30] uFmSetBase 14, uSkyWaterModelEx 9, uSkyGrassWindDirection 4, uStageSchedulerEdit, uSkyColorFog, uStageLowMdl |
| 5 | Scr Sub | [28] uStageSplitMdl 9, uSkyCloudModel 6, uSkyGrass 6, uStageLowMdl 6, uBaseModel |
| 6 | Obj | [1] uEm8300 |
| 7, 8 | Obj Sub1, Obj Sub2 | empty |
| 9 | Pawn Intel | [5] uPawnIntel 3, uStageSplitCtrl, uConnectOmChecker |
| 10 | **Player** | [4] **uPlayer 1, uCmc 3** (the Arisen and the three pawns) |
| 11 | Player Sub1 | [19] uWeaponPl 10, soft bodies 6, uAmbientShadow 2, uScrollCollisionGeometry |
| 12 | Player Sub2 | [30] uNpcIntel |
| 13, 14 | CMC Sub1, CMC Sub2 | empty |
| 15 | **Enemy** | [8] **uEm0600 5** (enemies), **uMultiNpc 3** (NPCs) |
| 16 | Enemy Sub1 | [9] uWeaponPl 6, soft bodies 3 |
| 17 | Enemy Sub2 | empty |
| 18 | Obj2 | [153] uFmSwingBase 97, uOm* objects |
| 19 | Obj2 Sub1 | [27] uGeometry2 23, uSimSoftBody 4 |
| 20 | Obj2 Sub2 | empty |
| 21 | **Camera** | [1] uCameraCtrl |
| 22 | Effect Ctrl | [2] uSkyExt, uSkyReflection |
| 23 | Effect | [35] uEffectExt 31, uEffect2DExt 4 |
| 24 | Effect Sub | empty |
| 25 | Light | [16] lights |
| 26, 27 | ID1, ID1 SUB | [12] HUD: uGUINpcGauge, uGUINpcMessage, uGUICmcMessage, uIdCockpit, uGUILifeGauge, uGUIMiniMap, uGUISight ... |
| 28-32 | ID2 ... ID2 SUB4 | [2] uGUIArcLoader, uGUIActionPallet |
| 33 | Fade | [3] uSoundGenerator, uSoundTrigger, uSoundOcclusion |
| 34, 35 | Fade Scheduler, Fade Filter | empty |
| 36 | Post Process | [1] uGrassReceiver |

What follows from the passes and the lines:
- **Story scripts move first.** Stage scripts (uFSMTask) and quest
  scripts (uFSMQuest) are on line 0, so they see the Arisen where the
  previous frame left them.
- **The Arisen and the pawns move together.** They share line 10 as
  concurrent jobs.
- **Enemies and NPCs move after the party** (line 15). So they react to
  this frame's party positions.
- **Objects, then the camera, effects, lights and HUD** move later
  (lines 18-32), each after everything it follows.

## 5. Inside a character's frame

Traced [exe] for `uPlayer` (vtable slots via `uPlayerBase` and
`uCharacterBase`), the pawn `uCmc` and the goblin `uEm0100` / `uEnemy`.
The phase names are this document's.

### 5.1 Before the units: hits from the last frame

`0x004792C0` (step 33) queues `0x00479230` as parallel jobs, one per
`cObjCollision`. Each one:
1. merges the frame's hits (`0x0076B7E0`);
2. builds the damage info (`0x00774ED0`);
3. applies damage (`0x0076DB10`);
4. calls the owner's callbacks at +0xCAC and +0xCA4 (with the HitInfo at
   +0x640).

Hit volumes are switched on in pass C (5.5). So **the volumes of frame
N-1 hit at the start of frame N, before anyone moves.** Damage rules:
COMBAT.md.

### 5.2 Pass A, slot 8: the move

The call chain is `uPlayer::vf08` → `uPlayerBase::vf08` (0x00B61280) →
`uObjModel::vf08` (0x00885DC0):
- the first move runs slot 32 once (`uPlayerBase::vf32` 0x00B59240);
- while `+0x1f98 == 1`, the four phases below run in order (slots 90-93);
- `uPlayerBase::vf08` then calls slot 24 when the body parts changed.

Enemies (`uEnemy::vf08` 0x00AA94D0) run the same four phases, skipped in
enemy states 2 and 3, with enemy steps around them.

**Phase 90: status, AI, input.** `uPlayerBase::vf90` 0x00B61A20:
1. Saves the last velocity and acceleration.
2. Ground probe `0x00DEA870` (100 above to 100 below) into +0x32B0.
3. `uCharacterBase::vf90` (0x0084BA60):
   - **status conditions** (`0x00836C70`, timers scaled by `sWorkRate`
     rates rather than the unit step);
   - `uObjModel::vf90` (0x00885E30): the last positions are kept, and
     **the unit step `+0x14` is rescaled by the character's rate
     factors** (the 1.15 locomotion rate of ACCEPTANCE.md);
   - **AI think**, when the `cAICtrl` at +0x2E64 is active
     (`0x0059F9F0`, `0x0059E070`; mode 0 `0x0059E320` runs the
     controller's FSM on an interval, *inferred* to be `cAIFSM`; the
     pawn's mode 5 is `0x0059E7D0`).
4. Fall-out and stuck checks; the last 3 safe ground positions
   (`0x00B63520`, `0x00B62D20`, `0x00B71A50`).
5. Parameters by id: the turn rate +0x2584 = m_pl id 3 × π/180, and the
   fall clamp +0x2564 = id 0x7D.
6. **Input**: slot 200 (`uPlayerBase::vf200` 0x00B6AEA0):
   - reads the stick from the `sQlocInput` object (`0x00B51EE0`);
   - rotates it by the camera (read, not updated: the previous frame's
     camera, *inferred*);
   - maps buttons to commands (`0x00B52880`).

   A pawn's `uCmc::vf200` (0x00B537D0) first copies its AI's virtual pad
   (cAICtrl +0x40) into the same fields. Pawns are "pressed" by their AI
   through the same input path. Enemies have no slot 200.
7. Stats, attack parameters and equipment upkeep.
8. Slot 136: ledge test (MOVEMENT.md).
9. Encumbrance `0x00B81120`, **stamina `0x00B81450`** (STAMINA.md),
   level-up `0x00B5BD20`, vocation rank `0x00B5C230`.

**Phase 91: action and integration.** `uCharacterBase::vf91` 0x0084BF60:
1. The action manager (+0x2DB8, `0x007F1EA0`) applies a pending action
   change: the old action's vf08, then the new one's vf13 and vf05.
2. **The current action's vf06** (the `cPlAct*::vf06` of MOVEMENT.md;
   air control `0x00AC51D0` is called from them).
3. Action time += step.
4. Yaw rotator `0x0084CC00`.
5. **Integration** `0x0084CCC0`, in this order:
   - `pos += velocity × dt`;
   - `pos += push × dt`;
   - `velocity += acceleration × dt`;
   - the velocity clamp.
6. AI post-step `0x0059E0F0`.

**Phase 92: motion.**
1. `uPlayerBase::vf92` (0x00B63E20): action stamina charge `0x00B60F70`.
2. `uCharacterBase::vf92` (0x0084C0A0) computes the animation step and
   writes it to `+0x14` for the duration. The step is the unit step ×
   slow factors × weight factor × one-shot factor.
3. `uObjModel::vf92` (0x00886210) runs the **motion advance**
   `0x010629F0`:
   - pose freeze on a change (`0x01061DD0`);
   - **root motion** `0x0105B740`: pos += the root delta, rotated;
   - frame and blend advance `0x01060730` (ANIMATION.md).
4. `+0x14` is restored.
5. Motion-flag effects and sounds.

**Phase 93: commit.** `uPlayerBase::vf93` 0x00B63F70:
1. Weapon visibility, aim and lock-on sub-states, casts (slot 206).
2. `uCharacterBase::vf93` (0x0084C560) → `uObjModel::vf93` (0x00886280):
   - **world matrix and joint pose** (`uCoord::vf20` 0x00DEABC0,
     `0x01058A70`);
   - collision activation `0x0076B470`;
   - constraints.
3. AI post `0x0059E160`.
4. Ground and water state casts (`0x00853FA0`).
5. The frame's displacement is stored (+0x4B40).

### 5.3 Pass B, slot 10: AI senses

`uCharacterBase::vf10` (0x0084C6A0) → `uObjModel::vf10` (0x00886400):
- slot 94;
- **AI sensors** (`0x005B4510`, `0x005A0E40`, `0x005AADC0`), after
  every unit has moved;
- a timer every 150 frames.

### 5.4 Between passes

`0x004797E0` (step 38) pushes characters out of each other, after pass
A, before pass B.

### 5.5 Pass C, slot 9: push-out and hit volumes

`uCharacterBase::vf09` (0x0084C720) → `uObjModel::vf09` (0x00886530):
1. Body shapes rebuilt from the joints, then a cast and sweeps
   (`0x0076C540`). *Inferred*: the terrain and wall push-out that corrects
   the position.
2. Slot 97 (*inferred*: climbing large monsters).
3. **Notice, attack and hurt volumes switched on** (`0x0076B300`,
   `0x0076B660`; COMBAT.md), for the next frame's hit pass (5.1).

### 5.6 The step

`cUnit.mDeltaTime` (+0x14):
- **Set** by the line move (section 3).
- **Rescaled** in phase 90 by the character's rate factors.
- **Replaced** by the animation step during phase 92.
- **Read** by:
  - actions and air control;
  - integration;
  - the motion advance;
  - the AI interval;
  - stamina;
  - timers.

Status timers use the `sWorkRate` rates instead.

### 5.7 One character frame, summarised

| Order | Step | Where |
|---|---|---|
| 1 | hits from last frame's volumes, damage applied | before pass A (5.1) |
| 2 | status conditions | phase 90 |
| 3 | AI think (pawns, enemies) | phase 90 |
| 4 | input: device stick and buttons, or the AI's virtual pad | phase 90, slot 200 |
| 5 | stamina, encumbrance, level-up | phase 90 |
| 6 | action change, action update (cPlAct vf06) | phase 91 |
| 7 | integration: position by velocity, then velocity by acceleration | phase 91 |
| 8 | motion advance and root motion | phase 92 |
| 9 | world matrix and pose commit, collision activation | phase 93 |
| 10 | character push-out (all characters) | between passes A and B |
| 11 | AI sensors (all characters) | pass B |
| 12 | wall/terrain push-out, hit volumes on for next frame | pass C |

(SECTIONS 6-7 PENDING)
