# Events and cutscenes

Measured 2026-10-09 against the Steam install and the 2017-12-13 exe
(identity in METHODOLOGY.md). Ghidra was read-only. **Implemented:** the
`ddquest` readers for event timelines (`sdl`), the exe's event table
(`events`) and the quest-script orders that request events (`order`).
**Tested:** every scheduler in the install parses (1,253 / 1,253); the
event table reads from the installed, SteamStub-packed `DDDA.exe`.
**Implemented (event-player):** timeline playback (`ddquest::sdl::Clock`,
`Scheduler::eval`), a headless event player (`ddstory::event`) and a thin
viewer (`ddview --event <row>`); see "Playback". **Not compared with the
original** except where a section says "live".

Story progress, quests and flags are in [QUESTS.md](QUESTS.md). The two
docs share one worked example: the opening, from the prologue to the
first pawn.

## What an event is

Capcom's word is "event" (`ev`). A cutscene is a folder of resources in
its own archive, plus one row of a table inside the exe.

| Piece | Where | Reader |
|---|---|---|
| Event table row | `DDDA.exe` `.data` VA 0x01822d60, 98 rows x 16 bytes | `ddquest::events::EventTable::from_exe` |
| Archive | `nativePC\rom\event\sSSS_EE.arc` (98 files; e.g. `s200_10.arc`) | `ddvfs` |
| Main timeline | `event\stSSS\evEE\stSSSevEE` (rScheduler, "SDL") | `ddquest::sdl::Scheduler` |
| Light timelines | `...\stSSSevEE_light1..4` (rScheduler) | same |
| Subtitles, voice (some events) | `...\stSSSevEE_msg_m/_f`, `..._sound_m/_f` (rScheduler) | same |
| Camera | `event\stSSS\evEE\list\camera\camera` (rCameraList, LCM v3) | not yet (Riftstone notes: per frame eye, target, quaternion, fov) |
| Actor motions | `event\stSSS\evEE\list\motion\pl_00`, `npcNNN_00`, `cmc_NN`, `wp_*`, `om*` (rMotionList) | `mtlmt` |
| Dialogue text | `id\message\event\stSSSevEE_<lang>` (GMD; labels `"0"`, `"1"`, ...) | `mtgmd` |
| Effects | `effect\efl\ev\stSSSevEE\*` | not yet |
| Event-only scripts | `event\stSSS\evEE\FSM\*` (rAIFSM; actor behaviour) | `mtfsm` |
| Movies | `nativePC\movie\event\st100ev00.wmv`, `st230ev00`, `st240ev05`, `st610ev21` | none (WMV) |

A movie event is an ordinary event whose timeline spawns a `uMovie` unit
named "Movie" with `rMovieOnDisk movie\event\stSSSevEE` (found by name by
0x00436290); the rest of the machinery is the same.

### The event table (exe)

Row layout (users 0x00435a10, 0x00515130, 0x00516140, 0x0050be20,
0x004362f0):

| Offset | Field |
|---|---|
| +0x0 | u16 stage |
| +0x2 | u16 event number |
| +0x4 | VA of `"stSSS\evEE\stSSSevEE"`, loaded as `event\` + path |
| +0x8 | u16 passed to `[0x018fa4ec]` vf+0x4c at event start and end (0 or 3; untraced) |
| +0xa | u16 0/1/2; bit 0 sets a bit in sGameSys +0xb8848 after the event (0x00437480; meaning untraced) |
| +0xc | VA of a u16 NPC id list ending in 0xffff, or 0 |

The index at 0x01822d34 gives each hundred of stage numbers its first
row; 0x00435a10 searches only that range. An event is addressed by its
**row index** (e.g. st200 ev10 is row 22) everywhere after the request.
0x00435a10 then swaps some rows for variants: rows 7-12 and 13-18
(stage 100 events 40-45 and 50-55, same scenes) by a value from sGameSys
+0xb3360 through 0x00477530, and rows 0x1a-0x1f by 0x00476d00. What
player property selects them is untraced (`EventTable::resolve`).

The NPC list matches the actors of the timeline: for 85 of 98 rows the
set equals the timeline's `uActorModelPl` units with `mUnitType` 2; in
the other 13 the table has no list and the timeline uses special ids
(783-787, 991-998).

The table is read from the player's exe; its bytes at these VAs are the
same in the packed Steam exe and the unpacked analysis copy. The reader
refuses other builds (PE timestamp 1513180209).

## Timeline format: rScheduler (SDL v19)

Loader `rScheduler::vf10` 0x00db8e50; evaluator `uScheduler` 0x00ddfd50,
key evaluation 0x00ddee70. Full layout in `crates/ddquest/src/sdl.rs`.

- **Header** (0x18): `SDL\0`, u16 version 0x13 (both checked), u16 track
  count, u32 0xee4828d3 (every file; never read), u32 frame count (bits
  0-23; bit 24 "integer frames", 0 in every file), u32 cut track, u32
  string-table offset.
- **Tracks** (0x18 each): kind, MtDTI property type, key count, link,
  name, class hash, key offset, value offset.

| Kind | Meaning | Value |
|---|---|---|
| 1 | root | |
| 2 | spawn a unit of the class (JAMCRC hash), on move line `link`; it dies with the scheduler (0x00ddecb0) | |
| 3 | bind an existing singleton reached from sMain (sCameraExt, sWeatherManager, sShadow, sEffectExt, sGameSys) | |
| 4 | folder (no name, no action) | |
| 5 | member object of the parent | |
| 6-14 | int, vec4, float, bool, reference to another track's object, resource (type hash + path), string, event call, matrix | per kind |

- **Keys**: u32, frame in bits 0-23, mode in 24-31. The key in force is
  the last with frame <= the current frame. Modes: 0 set; 1 and 4 count
  (value + frames since the key); 2 trigger (fires only when the playhead
  crosses it); 3 linear to the next key; 5 Catmull-Rom (vec4/float; ints
  treat it as linear). Measured: 120,351 / 11,275 / 3,273 / 69,434 / 2 /
  603 keys of modes 0-5.
- **Cut track**: when non-zero (94 files, always `OfsMotionCamera.mCut`)
  the playhead snaps down to floor(frame) when floor(frame)+1 is a key of
  that track, so a camera cut is never interpolated (0x00ddeb00).
- **Playback** (`uScheduler::vf08` 0x00de02a0): frame += speed (+0x3c,
  default 1.0) x the unit's `mDeltaTime` unless paused; at the end it
  loops if +0x41 is set, else clamps. Tracks apply only when the frame
  changed.

Totals: 1,253 archive entries (806 paths), 126,729 tracks, 204,938 keys,
0 failures (`cargo test -p ddquest -- --nocapture`). Dump any timeline
with `cargo run -p ddquest --example sdl_dump -- <nativePC> <path>`.

## Runtime: from request to control returning

### 1. The request

Events are requested by scripts, never by data alone. The `Event` order
of a quest or stage script (`cFSMOrder::cFSMOrderParamEvent`: mStageNo,
mEventNo, mDoSetSceNo, mSceNo, mSetPosType, mStartPosNo, mSetPos, Pos,
mSetAngle, Angle, and a result flag) is handled by 0x005bcb70:

1. Unless the current stage (aStage, `[*0x018d099c + 0x3834]`) already
   plays an event (+0x84), find the row (0x00435a10).
2. Build the request (row, SceNo, position) and post it with 0x00516ec0:
   aStage +0x84 = 1, +0x88 = row, request copied to +0xa0. Requests of
   most rows are refused while the player is busy (0x00516e00: sGameSys
   +0xb8844 bits 31/30, player state); rows 1, 0x1a-0x1f, 0x5c-0x5e are
   always accepted.
3. The script's result flag is written as `!mFlagBool` now and as
   `mFlagBool` once aStage +0x84 is clear again (event over).

The script also sees `IsEvent` (aStage +0x84 == 1, getter 0x005bb080)
and `IsEventStart`, and quest scripts use those to wait.

Movement of the request into the stage: in the play mode (aStage mode 2,
0x0050b870) a pending row (+0x88 >= 0) is taken by 0x00516fc0: request
moved to +0xd0, row to +0x8c, stage mode = 3 (event).

### 2. Stage event mode (aStage::move 0x00504330, mode 3)

| Substate | Function | What happens |
|---|---|---|
| 0 start | 0x0050be20 | Sets several sGameSys request bits (0x004011e0, 0x004374d0); fades (0x0043f570, 30.0 for row 0x5a, else 6.0; units untraced); loads the event's resources (0x005171a0: resource id `row + 0x6269`, plus an NPC look for rows 7-18, 0x40, 0x41, 0x4e-0x51 and 0x1f); per-row special cases. |
| 1 run | 0x0050c290 | Prepares (0x00453030, 0x00454810, 0x00514a90; not read in detail); when loaded, hand the row to sEventManager (+0x20 = 1, +0x28 = row) and wait until it is idle again. Before playing, the weather manager's time (+0x4c) is set to the light bucket's hour: 22, 5, 10, 19 or 22 h (hour < 4, < 7, < 17, < 20, else), except rows 3-6, 0x17, 0x18, 0x32, 0x3d. |
| 2 end | 0x0050c7a0 | Raise SceNo to the request's mSceNo if mDoSetSceNo (0x0043d750, raise-only); clear aStage +0x84 (control returns: the script's played flag flips); place the four party members per mSetPosType (0 none, 1 the given position with per-member offsets, 2 a numbered start position); start a chained request if one is pending; restore the weather values saved at the start (sWeatherManager +0xa58/+0xa5c); per-row follow-ups (e.g. rows 0x36, 0x38, 0x39 set fixed GSF bits). |

### 3. sEventManager (singleton 0x018fa4b8, update 0x00435840)

| State | Function | What happens |
|---|---|---|
| 1 request | 0x00435d50 | After two frames run setup 0x004362f0; failure goes to 5. |
| 2 prepare | 0x00435d90 | Movie: wait for the `uMovie` status (+0x38) to reach 3, giving up after 5,001 ms if it stays 0. Real-time: wait until every spawned model has loaded. Then create a uFade (0x00436870), start the movie (0x00f7a720) and unpause the schedulers. |
| 3 play | 0x00435fb0 | If skip is pressed (0x004367c0) go to 4. When the main timeline reaches frame count - 1, pause, fade 0 (0x0043f570) and go to 5. |
| 4 skip | 0x00436070 | Fade over 10.0, pause the schedulers, then go to 5. |
| 5 cleanup | 0x004361a0 | Wait for pending jobs (0x0041fcc0), kill the four scheduler units (which kills every unit they spawned), clear sGameSys +0xb883c bit 31, return to 0. |

**Setup (0x004362f0)** loads `event\<row path>` as a `uSchedulerExt`
(+0x2c, end frame +0x24 = frame count - 1), and `event\<path>_light%d`
(+0x30): light 1 for stages 320, 380, 400, 410, 436, 440, 450, 500,
502, 610, 611; none for 800; otherwise by the hour (sGameSys +0xb876c):
<4 -> 4, <7 -> 1, <17 -> 2, <20 -> 3, else 4. Rows 0x14, 0x16, 0x1a,
0x1b, 0x2f, 0x34 also load `_msg_m/_f` (+0x34) and `_sound_m/_f` (+0x38),
the letter from sGameSys +0xa76dc bit 0 (+0xa7ecc for 0x1a/0x1b). All
units go on move line 1.

**Skip** (0x004367c0): any of four pads with a mask bit, or 0x0040d200.
No "seen before" condition exists in this code.

**Actors** (`uActorModelPl::vf05` 0x0083e010): `mUnitType` 0 is the
player (copies the player's look, loads voice
`sound\se\vo_ev\st%dev%02d_vo_%s%d`; the letter from +0x1324, assumed
sex, untraced); 1 is pawn slot `mUnitNo`; 2 is NPC `mUnitNo` with its
look from `edit\st%03d\n%03d\st%03d_n%03d` (rEditPawn). Ids 991-999 are
special (995 -> sGameSys +0xb3360, 997 -> +0xa76f4, 996/999 ->
0x00476d00).

### Co-op note

The event is a stage-wide mode: every party member is frozen and placed
by the end step. In a two-Arisen session the host should run the event
state machine and the clients follow it; `mUnitType` 0 binds one player
model, so a second Arisen needs a rule (hide or place) that the original
does not have. To be decided in the implementation brief.

## Worked example: the wake-up scene, st200 ev10

Requested by `quest\q0001_b00` state 2 (QUESTS.md). Row 22, NPCs [1, 2].
Archive `rom\event\s200_10.arc`.

Main timeline `event\st200\ev10\st200ev10`: 1,702 frames, 133 tracks,
cut track 4.

| Frames | What the timeline does |
|---|---|
| 0 | `uOfsMotionCamera` (line 21) loads `rCameraList ...\list\camera\camera` (8 cameras); a bind sets `sCameraExt.mViewport.mpCamera` to it. Three `uActorModelPl` units: `pl_00` (player), `npc001_00`, `npc002_00` (NPCs 1 and 2), each with `rMotionList ...\list\motion\<name>` (8 motions). Image-plane fade from black: alpha 1 -> 0.75 -> 0 over frames 0-60. |
| 0, 460, 670, 790, 910, 1030, 1150, 1270 | Cuts: `mCut` = cut number, `mFrame` counts from 0 in each cut (count mode); each actor's `mMotionNo` = cut number. A `uResetFlag` resets soft bodies at each cut; per-cut Draw/Move, look-at and hand IK; camera adjust targets on `pl_00` joint 173. |
| triggers | Bloom, depth of field, effect lists `effect\efl\ev\st200ev10\*`. |
| 1643-1702 | Fade to black (alpha 0 -> 1). |
| 1701 | sEventManager ends the event (frame count - 1). |

Light timeline `st200ev10_light1..4` binds `sWeatherManager.EventCustomParam`
and spawns event lights (e.g. `InfiniteLight_quina`, `_soncho`).

Subtitles `st200ev10_msg_m` / `_msg_f`: a `uGUISubtitlesExt2` with
`rGUI id\telop\event_telop`, `MessageResourcePath
id\message\event\st200ev10_jpn`, `Language` 255, and `IndexName` keys
naming GMD labels: `"0"` at frame 30, `"1"` at 151, `"2"` at 326, ...
`"12"` at 1271, with `Visible`, `SpeakerType` and `ListenerType` keys.
The male file skips label `"3"`, the female variant of line 2 ("His
heart lies silent!" / "Her heart lies silent!"). How `Language` 255 and
the `_jpn` path become the player's language is untraced (guess: the
suffix is replaced).

Voice `st200ev10_sound_m` / `_f`: `uSoundStream` BGM
`sound\stream\event_bgm\event_bgm`, NPC voice stream `event_vo_se`, and
the player voice `uSoundSe` `sound\se\vo_ev\st200ev10_vo_m0`/`_m1` at
frame 514, chosen by `mConditionalNo`.

When it ends: SceNo becomes 1200 (raise-only), the party is placed at
start position 12 of stage 200, and q0001's played flag (FreeFlag 2)
flips; the script waits for `IsFadeBlack` and then `!IsEvent`.

## Playback (`ddstory::event`, `ddquest::sdl`)

**Timeline clock** (`ddquest::sdl::Clock`, translated):

- **Constructor 0x00dde6c0:** frame 0, previous frame -1, speed 1,
  paused, looping, cut snapping on. The event setup 0x004362f0 turns
  looping off for every event timeline and pauses them; the prepare state
  unpauses them.
- **Update (`uScheduler::vf08` 0x00de02a0):**
  1. If the frame differs from the previous update's, the tracks are
     evaluated at it (0x00ddfd50).
  2. The previous frame becomes the current one.
  3. Unless paused, the frame advances by speed x mDeltaTime.
  4. At the frame count it wraps to 0 when looping, otherwise it stays at
     the frame count.
- **Playhead (0x00ddfd50):**
  - integer-frame files floor both frames;
  - the cut snap 0x00ddeb00 floors a frame whose next integer frame is a
    cut key;
  - the key index uses the frame rounded by the x87 `fistp`, *assumed* to
    be in its default round-to-nearest-even mode.
- **Key evaluation (0x00ddee70):**
  - The key in force is the last key at or before the rounded frame, or
    the **first key when the playhead is before it**.
  - A trigger key (mode 2) applies only when prev <= key <= frame.
  - Int keys:
    - modes 0 and 2 set the value;
    - modes 1 and 4 count (value - key + rounded frame);
    - modes 3 and 5 interpolate linearly, truncated;
    - other modes apply nothing.
  - Float and vec4 keys:
    - modes 1 and 4 count (value + frame - key);
    - mode 3 is linear;
    - mode 5 is Catmull-Rom (Hermite form, tangents (p[i+1] - p[i-1]) / 2
      and (p[i+2] - p[i]) / 2, with indices clamped at both ends).
  - Matrix keys are linear in mode 3 only.
  - Every other kind sets its value.

**Event player** (`ddstory::event::EventPlayer::open(data, row, config)`,
then `step(delta)` once per frame):

- **Timelines (0x004362f0):**
  - the main timeline;
  - `_light<N>`, chosen by `light_index`:
    - light 1 on stages 320, 380, 400, 410, 436, 440, 450, 500, 502, 610
      and 611;
    - none on stage 800 or for rows past 0x61;
    - otherwise by the hour: below 4 -> 4, below 7 -> 1, below 17 -> 2,
      below 20 -> 3, otherwise 4;
  - `_msg_` and `_sound_` with the letter `m` or `f` (sGameSys +0xa76dc
    bit 0; strings 0x0155c6fc and 0x0155c700), for rows 0x14, 0x16, 0x1a,
    0x1b, 0x2f and 0x34.
- **End test (0x00435fb0):** once round(main frame) >= frame count - 1,
  every timeline pauses and the event is over. The test runs before the
  units update, so the last frame, 1701 for st200 ev10, is still shown.
  That order of system and unit updates is *assumed*. Skip (0x00436070)
  ends at once; its 10-frame fade is left to the host.
- **Per-frame output (`EventFrame`):**
  - **Camera** (`uOfsMotionCamera`):
    - cut (`mCut`), local frame (`mFrame`) and the base `mtcamera` pose
      from the camera list;
    - the raw `mOffsetPos` and the actor-joint adjustments (target unit,
      joint, weight, mask; 0x007ecc30 and 0x007ed220 average joint
      displacements into eye and target offsets);
    - not applied: those adjustments, sWorldOffset (0x007eb9b0) and
      `mOffsetPos`.
  - **Actors** (`uActorModelPl`): unit type and number, motion list,
    motion number and frame, Draw, Move, offset.
  - **Fade:** the image-plane filter's colour while it draws.
  - **Subtitle:** label (`IndexName`) while `Visible`, the line from the
    GMD, and speaker and listener.
  - **Sound cues:** `uSoundStream` when `mStreamCallFlag` turns true, and
    `uSoundSe` when `mSeCallFlag` turns true (with `mConditionalNo` when
    `mConditionalFlag` is set). The edge rule is *guessed*: the sound
    units were not traced. For variants, the host picks.
- **Language:**
  - The subtitle unit's `MessageResourcePath` setter (0x00726780)
    rewrites every three-letter language token of the path to the game
    language. The rewrite is `localize_path`, 0x00de13a0, with the table
    0x016333e0: jpn, eng, fre, spa, ger, ita, kor, zht, chS, por, rus,
    pol, dut, fin, swe, dan, nor, cze, hun, slo, oth.
  - If the rewritten file is missing, the setter loads the path as
    written.
  - `Language 255` in the timeline is not read by this path.
- **Return of control:** the host sends `HostEvent::EventFinished` when
  `finished` is set. `Story` then runs the stage's end step (0x0050c7a0):
  it raises SceNo and asks for `PlaceParty`. `route::run` and
  `ScriptedHost::play_rows` play chosen rows this way inside the opening.

**st200 ev10, headless** (`cargo test --release -p ddstory --test
event`; dump with `cargo run --release -p ddstory --example event_dump --
<nativePC> <DDDA.exe> 22 [language] [sex]`):

- **Updates:** 1,702 updates, frames 0-1701, finished at 1701.
- **Cuts:** at 0, 460, 670, 790, 910, 1030, 1150 and 1270. Each actor's
  motion number follows the cut. The player is hidden in cuts 2-6.
- **Subtitles:** labels 0 @30, 1 @151, 2 @326 (3 for the female
  variant), 4 @480, 5 @610, 6 @700, 7 @729, 8 @825, 9 @931, 10 @1052,
  11 @1186 and 12 @1271. Lines are hidden at 113, 406, 555, 662, 1000,
  1126, 1256 and 1296.
- **Fade:** black at 0, clear 60-1642, black again from 1643.
- **Cues:** BGM and NPC voice streams at 0; the Arisen's voice at 514, in
  two variants (`mConditionalNo` 2 and 3).
- **Inside the opening:** row 22 plays for real. SceNo goes from 1100 to
  1200 and `PlaceParty` start position 12 follows after the last frame.
  The ladder and the boots are unchanged.

**Viewer:**

```text
ddview <nativePC> --event 22 [--start-frame F --hold --shot out.png]
```

- **Shows:**
  - the stage models (`scr\st200\model\st200_mdNN` at the stage origin,
    and the `st200_00m00n_s00/s10` scenery at its stored positions;
    *assumed* frame);
  - the actors;
  - the event camera and FOV (horizontal FOV converted);
  - the fade and the subtitle text.
- **Actors:** every actor is the base Arisen body as a stand-in, and the
  NPC looks are not read. Each actor is placed by its motion's joint-255
  root track, in stage centimetres (*assumed* from the data: the result
  lines up with the event camera).
- **Screenshots** (2026-10-09, not committed):
  - frame 60 (cut 0): a figure by the wall of the room, line 0;
  - 460 (cut 1): a close-up of the player's torso and hand. Cut 1 weights
    the camera adjustment on the player's joint 173 at 1.0, which is not
    applied here;
  - 670 (cut 2): the two NPCs face to face, player hidden;
  - 790 (cut 3): the two NPCs in close-up with line 7;
  - 1270 (cut 7): both NPCs in front and the player behind them.
- **Not checked against the original:** the poses. The stand-ins stand
  where the player should lie in bed, probably because the event motions
  do not suit the base skeleton or because IK is missing.

## Open questions

- Header +0x08 constant and bits above 24 of +0x0c.
- `uOfsMotionCamera`, `uGUISubtitlesExt2` language selection,
  `uSoundSe.mConditionalNo`, `uSoundStream.mPrepareId`.
- Table fields +0x8 and +0xa; the variant selectors 0x00477530 and
  0x00476d00.
- What the event resource id (`row + 0x6269`) loads (0x00418810).
- Event FSMs under `event\stSSS\evEE\FSM\` (cFSMEvent, cAIActionFSMDemo
  owners) and `event\stSSS\stage_FSM\stage_NNN[_nosave]` (started as
  cFSMEvent tasks by 0x005081a0): not decoded.
- `uOfsMotionCamera`: what `mOffsetPos` feeds and the reference pose of
  the joint adjustment (0x007ed1c0). The base pose is implemented and
  the adjustment is not.
- `uSoundSe` / `uSoundStream`: what starts a sound, and which
  `mConditionalNo` plays.
- Which sGameSys +0xa76dc bit 0 means female, and whether that bit is
  the player record's sex.
- The engine's frame-rate handling for events: do events run on
  mDeltaTime at 30?
