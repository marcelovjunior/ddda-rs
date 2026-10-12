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

## 1a. The clock and the frame pacer (`sMain::vf06`, `0x00DBFA90`)

`sMain`'s properties (the DTI list in `sMain::vf03`, 0x00DBEB70, decoded
from its records) name the timing fields [exe]:

| Field | Offset | Type | Notes |
|---|---|---|---|
| `mTimer` | +0x20 | u64 | elapsed milliseconds (QPC-based) |
| `mFrameTimer` | +0x30 | u32 | `mTimer` × `mFps` / 1000: elapsed frames at the base rate |
| `mFps` | +0x38 | f32 | the **base rate**: **30.0 in the game** (`sBbsRpgMain`'s constructor stores it at 0x0041CEC9 over `sMain`'s default 60.0), so one unit of `mDeltaTime` is 1/30 s |
| `mMaxFps` | +0x3c | f32 | the **cap**, 3000.0 at construction; the `MaxFPS` option sets it (range 30-1000) |
| `mFrameWait` | +0x40 | bool | fixed-step mode (accessor 0x00DBD0A0) |
| `mActualFps` | +0x44 | f32 | measured rate |
| `mPause` | +0x6c | bool | capture/benchmark pause (section 3) |
| `mDeltaTime` | +0x70 | f32 | this frame's step, in base frames |
| `mDeltaTimeBorder` | +0x74 | f32 | above this, the frame is treated as a hitch (100.0 at construction) |
| `mDeltaTimeLimite` | +0x78 | f32 | the largest step (16.0 at construction) |
| `mGlobalSpeed` | +0x80 | f32 | multiplies the step (slow motion) |
| `mDelayFrame` | +0x84 | f32 | accumulated base frames dropped by hitches |
| (QPC ticks per base frame) | +0x1c030 | i64 | |

Defaults come from the constructor (0x00DBE810) [exe]. It sets `+0x48` = 30.0
(not yet named), registers the application option **`MaxFPS`**
(0x0142A8D0, f32, range 30.0-1000.0, setter 0x00F5BB60) and the job-thread
count option. `FrameRate`, `Time` and `System` are property groups. `ResetFrame`
(0x00DBD710) and `ResetTimer` (0x00DBE210) are property actions.

**What `vf06` does each frame** [exe]:

1. It reads QueryPerformanceCounter through 0x0130A5A0 into
   `+0x1c000..`, and updates `mTimer` and `mFrameTimer`. It also
   accumulates the elapsed ticks at `+0x50` (`+0x58` is the tick count at
   the last frame boundary).
2. **Variable mode** (`mFrameWait` = 0), the normal path:
   - `mDeltaTime` = ticks since the last frame / ticks per base frame.
   - The minimum step is `mFps / mMaxFps`. If the frame came sooner than
     that, it `Sleep`s for (minimum − step) × 1000/`mFps` − 1 ms (when that
     is at least 2 ms). It then **spins** on QPC until the step reaches the
     minimum. This is the frame cap.
   - A negative step becomes 0.
   - A step above `mDeltaTimeBorder` is a hitch: the excess is added to
     `mDelayFrame`, the step becomes 1.0 and the timebase is resynced.
   - A step above `mDeltaTimeLimite` is clamped to it.
   - The boundary then advances by the frame just taken.
3. **Fixed mode** (`mFrameWait` ≠ 0): it sleeps 1 ms at a time until the
   next base-frame boundary, then advances it by exactly one base frame,
   and `mDeltaTime` = 1.0.
4. `mDeltaTime` ×= `mGlobalSpeed`.
5. `+0x7c` = 30.0 / max(1, `mActualFps`): `DAT_018d0b28` is `sMain` itself
   (its constructor stores `this` there), so this is the 1-s averaged rate in
   base frames (L30).

**What this means for the frame rate** (*inferred* from the code above;
not yet checked live; the per-system audit is [FRAME_RATE.md](FRAME_RATE.md), L30):
- The game is built to run at any rate. Gameplay advances by
  `mDeltaTime` in units of `mFps` frames: units take line rate ×
  `mDeltaTime` (section 3), and the saved clock uses `mDeltaTime / mFps`
  (section 6.1).
- The rate is capped only by `mMaxFps`. The user-facing `MaxFPS` option
  allows 30-1000, and RECORDINGS.md measured 150 live.
- In the rebuild, an unlocked or higher frame rate is a data change:
  `mMaxFps` (and `mFrameWait` = 0). It is not a code change. The base
  rate `mFps` = 30 only sets the unit of `mDeltaTime`. Changing it would
  rescale gameplay, so leave it alone.
- Anything that misbehaves at high rates is a per-system bug to find
  where that system ignores `mDeltaTime`. Physics, animation blending,
  cloth and UI timers are the usual candidates, each to be measured.
- The pacer's `Sleep` + spin can also be replaced in the host (for
  example by waiting on present/vsync) without touching the step logic.
  The game only sees `mDeltaTime`.
- `mDeltaTimeLimite` (16 base frames during the states in table A4, else
  2) bounds how far one frame can step after a hitch.

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
| 7 | `sSave::vf06` 0x00498300 | save cooldown |
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
| 18 | `sGameSys::vf06` 0x0043CEB0 | game clock step and game delta, talk time-out, the game-time clock `0x0043F2C0` (section 6) |
| 19 | `0x004883C0` | |
| 20 | `sAreaExt::vf06` 0x00419910 | the area stack (`aBbsRpg`, `aGameMain`, `aStageNNN`; ORACLE.md): **managers, quests, stage modes and events, scenarios** (section 6) |
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
| 27 | `sAreaHit::vf06` 0x0041A2C0 | area hit volumes (the story's AreaHit) |
| 28 | `sZone::vf06` 0x00F74590 | zone listeners |
| 29 | `sItemManager::vf06` 0x0045CF60 | items |
| 30 | `sAI::vf06` 0x00F795C0 | AI objects, slot 6, as parallel jobs |
| 31 | `sWorkRate::vf06` 0x004C5FC0 | six timed rate slots (hit-stop, slow motion), back to 1.0 when their timers end |
| 32 | if not paused (`sMain+0x68 != 0` or `sMain+0x6c == 0`): reset of `sObjCollision` counters, `0x00479770(0)` | |
| 33 | `0x004792C0` | **hits**: the last frame's volumes tested, damage applied (5.1) |
| 34 | `sRecognition::vf06` 0x00487B00 | enemy list, rebuilt on an interval |
| 35 | `sPawnManager` vf06 | (empty) |

### F. The units (section 3)

| # | Call | What |
|---|---|---|
| 36 | `sUnitExt::vf06` 0x004B97C0 → `sUnit::vf06` 0x00DB6740 | **pass A: every unit's move**, line by line |
| 37 | `sCollisionExt::vf06` 0x00422DC0 | collision |
| 38 | `0x004797E0(sObjCollision)` | characters pushed out of each other (5.4) |
| 39 | `sGUIExt` vf06 | (empty) |
| 40 | `sUnitExt::vf13` → `sUnit::vf13` 0x00DB5700 | **pass B**, line by line |
| 41 | `0x00413C40` | frees `sPlayerManager`'s per-frame request queues |
| 42 | `sUnitExt::vf11` → `sUnit::vf11` 0x00DB5690 | **pass C**, line by line |

### G. After the units

| # | Call | What |
|---|---|---|
| 43 | `0x00F79650` | `sAI` second pass (`0x00F79210(1)`) |
| 44 | `sNpcIntelManager::vf10` 0x00471D80 | NPC intel culling |
| 45 | `sIdMessage::vf06` 0x00455280, `sIdCockpit::vf06` 0x00451F80 | HUD messages, cockpit |
| 46 | `0x00498370`, `0x00479CB0`, `0x00ED5350` | |
| 47 | `sObjCollision::vf06` 0x00478AB0 | |
| 48 | `sSoftBody` vf06, `sGeneralPoint` vf06 | (empty) |
| 49 | `sAISensorTarget::vf06` 0x00415220 | |
| 50 | `sEffectExt::vf06` 0x004258F0 | effects |
| 51 | `sCameraExt::vf06` 0x00420910 | cameras (`sCamera::vf06` 0x00DC4830) |
| 52 | `sSoundExt::vf06` 0x004AD440 | sound |
| 53 | `sWeatherManager::vf06` 0x004C0C70 | weather (clock to weather to zone light) |
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

**Pause and freezing** [exe]:
- `sMain+0x6c` (`mPause`) is a capture/benchmark pause only. While it is
  set and the frame-step counter `sMain+0x68` is 0, these return at once:
  - the three unit passes and `sUnitExt`'s line move;
  - `sAI`, `sAreaHit`, `sWorkRate`, `sRecognition`, `sObjCollision`;
  - the main body of `sGameSys::vf06`.

  `sMain::vf06` counts `+0x68` down once per paused frame, so a pause can
  let N more frames run.
- **Gameplay freezes through the group mask.** The only writer of
  `sUnit+0xd30` is `0x00DB5A60(bit, enable)`. During events and menus,
  `0x0043F6C0` clears **bit 57**. That freezes:
  - the player, pawns, NPCs and enemies;
  - objects, shells and stage models;
  - the sky and the HUD.

  These keep running:
  - scripts (group 16) and schedulers (17);
  - menus (44-47);
  - fade and sound (49, 50).

  The story scripts stop anyway, through the gate `0x0044D430` (section
  6). Other mask bits: 60 (`0x0043F7E0`), 58 (`0x0043F990`), 55 and 59
  (`0x00514A90`, `0x00516460`).

**Adding units to lines** [exe]:
- `0x00DB5800` (at the head) and `0x00DB58B0` (at the tail) take (line,
  unit, group low, group high). They write the line into `unit+4` bits
  3-9 (0x7F = none) and set the unit's groups through `sUnit` vf19.
- `0x00DB6BB0` removes a unit.
- The creators that choose lines:
  - `0x004BA540` (by DTI, line in eax);
  - `0x00421FE0` (characters);
  - `0x0049DA10` (objects);
  - `0x00433E00` (enemies; an id in a 37-entry list goes on line - 9).

| Class | Line | Group |
|---|---|---|
| uFSMTask, uFSMQuest (story scripts) | 0 Pre Process | 16 |
| uScheduler | 1 Scheduler | 17 |
| stage models, uStageSplitCtrl, sky grass | 4 / 5 Scr | 20 |
| uPawnIntel | 9 | 28 |
| uPlayer | 10 Player | 25 |
| uCmc (pawns) | 10 Player | 28 |
| uNpcIntel | 12 | 30 |
| uNpc, uHumanEnemy, uEnemy | 15 Enemy | 30 / 31 |
| enemy parts | 16 / 17 | 32 |
| shells, uOm objects | 18 Obj2 | 53 / 34 / 35 |
| cameras (uCameraCtrl, uFreeCamera) | 21 Camera | 37 |
| lights | 25 | 41 |
| HUD | 26 / 27 | 42 |
| menus | 28-32 | 44-47 |
| fade, loading, sound | 33 | 49 |

Effect units attached to an owner go on the owner's line + 1. The live
walk (section 4) agrees for every class it saw.

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

## 6. Managers and the story

### 6.1 `sGameSys::vf06` (0x0043CEB0), step 18 [exe]

- Writes `+0xB879C` = `mDeltaTime / mFps`, the saved clock's step in
  seconds.
- Writes the game delta at `+0xB87A0`. It is 0 while `+0xB8844` has bit
  0x80000.
- Times out a talk request after 30 s.
- Unless paused, runs the game-system updates. Among them is the
  game-time clock `0x0043F2C0`; the others are `0x0078B760`, `0x0043D2B0`,
  `0x0043D4F0`, `0x004403A0`, `0x00441650`, `0x00441A60`, `0x00441C30` /
  `0x00441D40`, `0x0044A350`, `0x00437420` and `0x00442900`, not yet named.

### 6.2 `sAreaExt::vf06` (0x00419910), step 20 [exe]

`sAreaExt::vf06` calls `sArea::vf06` (0x00DB54D0). That walks the area
stack (`+0x382C`, up to 8 areas, root first): slots 5 and 6 once, then
slot 7 every frame. A pending area change (`+0x3828`) is applied by
`0x00DB4F10`. Live, the stack is [aBbsRpg, aGameMain, aStage100]
(ORACLE.md), so in one frame:

1. **`aGameMain` slot 7.** It is not a function in the Ghidra project;
   it was read from the disassembly at 0x004FB960. It:
   - runs `0x004FB7B0` once (new game or load);
   - then calls slot 6 of the managers through their globals:
     - `sEnemyManager` (0x018FA4B0), human enemies, `sPlayerManager`
       (0x018FA4EC);
     - the ScrOm manager, NPC intel, `sNpcManager` (0x018FA4E0), shells;
     - **`sQuestManager`** (0x018FA4F0, written by its ctor 0x004821A0):
       quest updates and the **QCT** (`0x00482DD0` → `0x0078C830` →
       `0x007922F0`; QUESTS.md);
     - **`sEventManager`**, lock-on, the AI boards;
     - **`sSetManager`** (spawning), sound.
2. **`aStage` slot 7** (`0x00504330`):
   - play time += step, capped at 216,000;
   - then a switch on the stage mode `+0x4 & 0xff`. Mode 2 is the
     **event** mode `0x0050B870`; mode 3 runs `0x0050BE20` / `0x0050C290`
     / `0x0050C7A0` (event load, play, end; EVENTS.md); mode 4 is the
     simple event `0x0050DCD0`; there are more, up to 0x2C;
   - unless `sGameSys+0xb8844` has bit 0x80000, slot 12;
   - then **`sScenario`** (global 0x018FA4FC) slot 6 = `0x0049A0E0` →
     `0x00797DF0`: trigger volumes and the scenario prompt
     (`sScenario+0x108`).
3. **Unit pass A, line 0** (step 36): uFSMTask (stage scripts) and
   uFSMQuest (quest scripts) slot 8 → the cAIFSM driver `0x00E092A0`.
   - They are gated by `0x0044D430`: no script runs during the stop
     flags, during a talk, or on stages 0x321-0x324.
   - Line 0 also runs as jobs (flags 0x1FD), so the scripts of one frame
     run concurrently.

So the story's order in a frame is:
1. quests and the QCT;
2. stage mode (events);
3. scenarios;
4. input (steps 22-25);
5. scripts.

The prompt a scenario reads (`sScenario+0x108`) is set by `uPlayerBase`
vf204 (0x00B6EAC0; QUESTS.md). *Inferred*: vf204 runs in the player's
move, from the button commands of phase 90 on line 10; its caller was not
traced. If so, **a press made in frame N reaches the scenarios in frame
N+1**.

*Differences from ddstory today* (QUESTS.md "Frame order", marked
guessed there):
- ddstory runs scenarios → QCT → stage scripts → quest scripts in one
  `tick`.
- The trace gives QCT → event mode → scenarios → scripts.
- Stage and quest scripts share line 0 as concurrent jobs, so no order
  between them is defined.

### 6.3 Other systems [exe]

- **AI.** `sAI` runs at step 30 and again after pass C (step 43). The
  characters' own think runs in phase 90, and their senses in pass B
  (section 5).
- **Hits.** Step 33 resolves the hits of the previous frame's volumes
  (5.1), before anything moves.
- **Spawning.** `sSetManager` runs in `aGameMain`'s managers (6.2), so
  units it creates join their lines and first move in the same frame's
  pass A (state 1 → slot 5, then slot 8).
- **Presentation.** Cameras run on line 21 and at step 51; effects at
  steps 11 and 50 and on lines 22-24; sound at step 52 and on line 33;
  weather at step 53. All of them follow the characters.

## 7. The host's order

`ddhost::Host::tick` follows sections 2-6, cut down to the systems that
exist as crates. Rows marked "next" name where a system goes when it
arrives.

| Host phase | Original step | Crate | State |
|---|---|---|---|
| 1. step | 6 `sMain::vf06` | `FrameClock` (constant step) | done |
| 2. world completions (stage loaded, event ended) | 18-20 results | `StoryWorld::events` | done |
| 3. story: QCT, events, scenarios, scripts | 20 (aGameMain, aStage, sScenario), 36 line 0 | `ddstory::Story::tick` | done; internal order per 6.2 requested from ddstory |
| 4. game clock, day counter, restock | 18 `sGameSys` clock | `ddstory::World` clock, `ddeconomy::update` | next |
| 5. input latch, presses | 22-25 | `input::Edges` | done |
| 6. AI objects | 30 | AI runtime | next |
| 7. hits from the last frame's volumes, damage | 33 | `ddcombat` | next |
| 8. the Arisen and pawns move | 36 line 10 | `ddsim::Sim::step` (one engine frame) | done (Arisen) |
| 9. enemies and NPCs move | 36 line 15 | enemy/NPC runtime | next |
| 10. push-out between characters | 38 | `ddworld` / ddsim | next |
| 11. AI senses | 40 pass B | AI runtime | next |
| 12. wall push-out, hit volumes on | 42 pass C | `ddsim`, `ddcombat` | next |
| 13. requests performed; cameras, sound, effects | 50-53 | `StoryWorld::handle`, views | done (stand-in) |

Two consequences for the host:
- **The story sees positions from the last frame.** The story runs
  before line 10 moves.
- **A scenario press arrives one frame late** (6.2). `Host::tick` keeps
  it for the next frame, and when two seats press on the same frame, the
  lower seat takes the one prompt slot.

Units on one line are concurrent jobs in the original (section 3). The
host runs them in seat order, then creation order (ADR 0002, section 3).

## 8. Open

- Not yet confirmed live by a call-order log: the oracle can record calls
  through private vtable copies without patching code (claude-4, #872),
  but the probe command does not exist yet. Sections 2, 3, 5 and 6 are
  static; the singleton fields and the line contents are confirmed live.
- The worker-thread count (`sMain+0x98` / `mJobThreadNum`). With one
  worker, or one job, a line runs inline.
- Which line each effect type uses; where the area stack is registered;
  the runtime line rates (all 1.0 in one read).
- The 1.15 locomotion rate (ACCEPTANCE.md) is applied to the unit step in
  `uObjModel::vf90`, phase 90 (5.2). ddsim models it as `unit_rate`.
