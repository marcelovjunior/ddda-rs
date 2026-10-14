# Decoupled simulation and rendering: design, routes to 240 fps, plan

Task L192 (research only; no code, no builds, no game runs). Goal from the
user: 240+ fps with 1:1 gameplay, and a snapshot-able fixed-tick simulation
that the 2-player co-op campaign can reuse. Original on the user's PC (5700X3D,
RTX 4060) at the benchmark spot: ~80 fps (12.5 ms). Ours: ~62 fps (16 ms).

**Tags.** [exe] static (Ghidra, read-only, checked in this task unless a doc is
cited); [doc] taken from an existing repo document; [prof] profile numbers from
`docs/tasks/frame-bound.md` and `docs/HOT_PATHS.md` (L172 tree, ~50 fps phase,
~1500 frames per 30 s sample, +-3 fps noise); *inferred*; **UNKNOWN** (nobody
has measured or traced it; each has a verification step in section 8).

Related: FRAME.md (frame order), FRAME_RATE.md (L30 audit), NDRAW.md (command
stream), SOFTBODY.md, COOP_ON_REBUILD.md (replication), adr/0002-game-host.md.

---

## 0. Summary

1. The original has **no fixed tick**. One loop iteration is one frame; the
   step `sMain.mDeltaTime` (+0x70) is measured per frame, in units of 1/30 s
   (`mFps` = 30). "Original tick rate" therefore has to be *chosen*: 30 Hz
   (dt = 1.0, the rate the rules were authored at) or 60 Hz (dt = 0.5). At
   80 fps the original runs dt ~= 0.375, so no fixed tick equals it exactly;
   the differences are small and listed (section 2).
2. Simulation and "render production" are interleaved but separable at a
   clean cut: FRAME.md steps 1-55 (sim) vs 56-65 (scene, shadow, draw jobs).
   The original's own network step (54) sits on that cut, so the same
   snapshot serves interpolation and co-op replication (sections 3, 7).
3. **Decoupling alone does not give 240.** A displayed frame still has to
   produce ~3,500-5,400 draws. Today that costs ~6.6 ms of main-thread/job
   time for production (sRender+camera ~3.3 ms, draw job batch ~3.3 ms wall
   before the shadow tail) plus ~12.7 ms on the render worker [prof]. A 4 ms
   frame needs the displayed frames to be **replays of a retained, already
   produced command stream with patched matrices** (route A2), the expensive
   passes (shadow, soft-body GPU sim, reflections) reused from the last tick,
   and a ~4x cheaper ndraw sink (route C). Route B (making the sim faster)
   gets us to native parity (~80 fps) at best.
4. Recommended order: measure (sim share, GPU time, per-pass worker time) ->
   fixed-tick clock with identical frames -> in-engine 2x interpolation as a
   validation harness -> command-stream replay at 4x (240) -> optional
   threaded sim. Details in section 9.

---

## 1. How the frame advances; where dt lives

### 1.1 Loop and clock [exe][doc FRAME.md 1, 1a]

- `sApp` global 0x018D0F50, its +0x2558 is the `sBbsRpgMain` (`sMain`). The
  loop is `0x00DF0D20`; each iteration calls `sBbsRpgMain::vf06` 0x0041DD60
  once. No fixed-rate loop, no accumulator.
- Step 6 of that frame is `sMain::vf06` 0x00DBFA90, the clock. Fields (sMain
  = `*0x018D0B28`): `mTimer` +0x20 (ms, QPC), `mFps` +0x38 = **30.0** (written
  at 0x0041CEC9, overriding sMain's default 60.0), `mMaxFps` +0x3C (cap; 3000
  at construction, 150 live, option `MaxFPS` 30-999), `mFrameWait` +0x40
  (fixed-step mode), `mActualFps` +0x44, `mPause` +0x6c, step counter +0x68,
  **`mDeltaTime` +0x70**, `mDeltaTimeBorder` +0x74, `mDeltaTimeLimite` +0x78
  (2.0 in play, 16.0 in some states), `mGlobalSpeed` +0x80, `mDelayFrame`
  +0x84, `+0x7c` = 30/max(1, mActualFps).
- Variable mode (the normal path): `mDeltaTime` = QPC ticks since the last
  frame / ticks per base frame; sleeps then spins until `mFps/mMaxFps` has
  elapsed (the cap); a hitch (> Border) becomes 1.0; clamp to Limite; then
  `*= mGlobalSpeed`.
- **Fixed mode** (`mFrameWait` != 0, accessor 0x00DBD0A0): sleeps 1 ms at a
  time to the next 1/30 s boundary, `mDeltaTime` = exactly 1.0. A built-in
  30 Hz deterministic mode (a 60 Hz one does not exist).
- Per-unit step: `sUnit::vf09` 0x00DB7350 sets `cUnit.mDeltaTime` (+0x14) =
  line rate x `sMain.mDeltaTime` (0x00DB7384-0x00DB73D5); rates are 1.0 on all
  37 lines live. Phase 90 rescales it by character rate factors, phase 92
  replaces it by the animation step (`+0x2EEC`), restored afterwards.
- Other clock outputs: `sGameSys::vf06` 0x0043CEB0 writes `+0xB879C` =
  `mDeltaTime/mFps` (seconds) and the game delta `+0xB87A0`.
- Consumers: 296 functions load `*0x018D0B28` and read a timing field: 192 read
  `mDeltaTime`, 66 `mFps`, 49 the step counter, 8 `mTimer`, 5 `+0x7c`
  [doc FRAME_RATE.md 2]. Most gameplay reads the unit step instead.

### 1.2 What consumes dt vs assumes fixed steps [doc FRAME_RATE.md 3-5]

dt-correct (scale with the step; unchanged under a fixed tick): unit step,
hit-stop/slow-motion (`sWorkRate` 0x004C5FC0), game clock/weather, status
timers, air control, yaw rotators, actions, animation and blending (0x01060730),
AI think/sense timers, combat recovery, stamina, constraints, grass/wind,
effect playback, GUI/fade timers, sound (ms = 1000/mFps x dt).

dt-approximate (step used, result depends on step size): character integration
0x0084CCC0 (explicit Euler: jump apex 95.0 cm at dt=0.5, 92.2 at 144 fps, 99.75
at dt=1.0 by the same formula v0^2/2g + v0*h/2 with v0=19, g=2), camera springs
(0x007BC400/0x007BC800, substeps of min(rest, 1.1)), long chains `uChain`
0x00F0C160 (substep count fixed, substep size shrinks with dt).

Frame-counted or bugged (rate-dependent; FRAME_RATE.md has fixes):
`sRecognition::vf06` 0x00487B00 (timers add *this frame's* dt once per
interval: recognition 2 s becomes 2 s x F/30; the original already shows 3-6 s
at its usual rate), cloth `uSimSoftBody::vf11` 0x00F20640 (accumulator
remainder dropped; 60 steps/s only at some rates), Frozen/Petrify velocity
scaling (0x0084CD0D/0x0084CD53, per frame), `cEm5501ActAtckRushBite::vf06`
0x00A36DF0 (per-frame lunge), effect noise "time source 4" (`sEffect+0x74`
incremented once per frame at 0x00FB64DC; 23 readers), end credits (`+0x7c`).

Real-time (wall clock, not sim time): input repeat on `mTimer`, event manager
throttle (`GetTickCount`, 5000 ms, 0x00435D90), pawn-sharing protocol
(`sDDCaProto`), movies (`timeGetTime`), rumble, hair chains `uCnsTinyChain`
0x00E99010 (own seconds accumulator, correct).

### 1.3 Which tick

| Tick | dt | Pros | Cons |
|---|---|---|---|
| 30 Hz | 1.0 | the authored rate: every frame-counted rule and the bugs above behave as designed; `mFrameWait` fixed mode exists; half the sim CPU of 60 Hz (33 ms tick budget: the current 16 ms frame fits twice); halves co-op bandwidth | +1 tick interpolation delay = 33 ms extra input-to-motion latency; jump apex +6% vs the original at 80 fps |
| 60 Hz | 0.5 | +16.7 ms delay; closest to what PC players run (apex 95.0 vs 93.8 at 80 fps: +1.3%) | the tick must finish in 16.6 ms: at today's 16 ms/frame there is no headroom until sim speedups land; recognition timers run at half speed (4 s; the original at 80 fps is slower still, ~6 s) |
| variable (as now) | per frame | no change | no decoupling; no determinism |

Recommendation: a constant tick (`SIM_DT` in base frames) with both 30 Hz and
60 Hz supported; default **60 Hz**, 30 Hz as the "authored" mode and as the
fallback when the machine cannot sustain 60 ticks/s. 1:1 parity is then
defined *at the chosen tick* (the existing record/compare gates already use
constant steps: ddhost default 1.0, ddaccept `dt_main/30`). It cannot be
defined frame by frame against the 80 fps original, because that run has a
variable dt. **UNKNOWN:** whether the lifted-game verification so far ran with
a recorded variable dt stream or a constant; the gate must be re-run at the
chosen constant.

How the lifted runtime forces the tick (no boot hacks): a virtual QPC at the
service boundary (RUNTIME.md allows a replay clock "at the service boundary,
labelled as a deterministic test input") advancing exactly `SIM_DT/30` s per
tick; `sMain::vf06` then computes `mDeltaTime` = SIM_DT and its sleep/spin
pacer exits at once. Alternative: write `mDeltaTime` after the lifted `vf06`
returns, with `mMaxFps` = 3000. Either keeps `mTimer`-based input repeats
consistent. Real-time consumers (`GetTickCount`, `timeGetTime`: events,
movies, pawn sharing) stay on wall time; if the machine runs below real time
the two drift (**UNKNOWN effect**; movies are the visible one).

---

## 2. Frame-rate-dependent risks at a fixed tick

At a constant tick all of section 1.2's items become constant rather than
rate-dependent; the question is which constant. Against the user's 80 fps
original (dt ~= 0.375):

| Item | 60 Hz tick | 30 Hz tick | Fix for parity? |
|---|---|---|---|
| jump apex (Euler) | 95.0 vs 93.8 cm (+1.3%) | 99.75 (+6%) | optional substepping; FRAME_RATE.md 6 |
| sRecognition | 4 s recognise, 20 s forget | 2 s / 10 s (designed) | original at 80 fps ~6 s; pick and document |
| cloth step | 60 steps/s if the remainder is kept | 2 steps/tick, exact | keep-remainder fix (float edge cases at exact divisors) |
| Frozen/Petrify decay | per tick: 2x faster than at 30 Hz | designed | `k^step` |
| em5501 lunge | 2x distance vs 30 Hz | designed | scale by unit step |
| effect noise mode 4 | 2x flicker rate vs 30 Hz | designed | advance by dt |
| camera springs | substeps 0.5 (step-aware) | 1.0 | none |

Tick-dependent but not rate-dependent: **input sampling**. All devices are
read in steps 22-25 once per tick (FRAME.md 2D). A click or key-up between
ticks must be latched (edge flags), not sampled. The camera is the exception
that should stay at render rate (section 4.2).

Known high-fps issues in the sources: Capcom's PC FAQ caps at 150 fps "to avoid
game-breaking issues such as physics interaction and rendering errors" [doc
FRAME_RATE.md 1, a lead only]; the code-level candidates are the rows above.
**UNKNOWN:** the 71 step-reading candidates not checked line by line, and
integer frame counters outside the constant scan. None matters for a constant
tick (they matter only for a variable-dt unlock), another argument for the
fixed tick over simply raising `mMaxFps`.

---

## 3. The sim/render boundary

### 3.1 By frame step (FRAME.md section 2 numbering)

| Steps | Content | Class |
|---|---|---|
| 1-6 | upkeep, `+0x1c274` frame counter, SteamAPI callbacks, clock | sim (clock) |
| 7-17 | save, storage, `sWorldOffset::vf06` 0x004C64E0 (floating origin), **`sEffect::vf09` 0x00FB5E00, `sPrimitive::vf09` 0x00E60D40, `sGpuParticle::vf09` 0x00EB26C0**, GUI pre-frame | mixed: effects and GPU particles advance state here |
| 18-21 | `sGameSys`, `sAreaExt` (managers, quests, stage modes, scenarios) | sim |
| 22-25 | input | sim input |
| 26-35 | AI objects, last frame's hit volumes, recognition | sim |
| 36-42 | **unit passes A/B/C** (37 move lines as job batches; barrier between lines) | sim |
| 43-55 | AI second pass, HUD, `sEffectExt` 0x004258F0, **cameras `sCameraExt` 0x00420910 -> `sCamera::vf06` 0x00DC4830**, sound, weather, **network 0x00467F50 (step 54)** | sim |
| 56-65 | `sSceneExt` 0x0049B030, `sShadow::vf06` 0x00E277A0, **`sRender::vf06` 0x00DAF000** (sort/publish the stream), `sGpuParticle::vf10` 0x00EB38A0, archive loading, env maps, capture, `0x004B9660` | render production |
| async | render worker `0x00DAC090` executes the stream of the previous frame (double buffer `sRender+0x33DB8` ^= 1 at `0x00DA9CD0`); present limiter `0x00DAED10` | render consumer |

The cut is between step 55 and 56. Step 54 (`sNetworkExt`) is the original's
own "capture and apply replicated state before rendering" slot, which
COOP_ON_REBUILD.md 4.2 already chose for host snapshots: **the render snapshot
and the network snapshot are taken at the same point.**

### 3.2 What runs inside the render production (verified in this task)

- `sUnit::vf16` 0x00DB63E0 (`this` = sUnit 0x018D09E0, arg1 = view/draw
  context, arg2 = line mask). Per call it walks all 37 lines and, for each unit
  with state 2, flags `& 0x4800 == 0x4800`, matching view mask (`ctx+0x16a`,
  `ctx+0x16c/0x16e`) and enabled groups (`~mask & group == 0`), queues job
  callback `0x00DB7620` and runs the batch with `0x00DBE560(1)`. `0x00DB7620`
  calls **unit vtable slot 11 (+0x2c), the draw method** (or the DLC consumer
  when `0x00DA7DB0` says so). A unit's draw is a function of (unit state, view
  context) and is invoked **once per view**: main scene, each shadow cascade
  (~10 `EE2C90` calls per frame [prof]), reflections.
- `sCamera::vf09` 0x00DC4770 loops the 8 view records (stride 0x190 from
  sCamera+0x40), sets pass and view index in the context (`ctx+0x164`,
  `ctx+0x168`), calls the view setup `0x00DC1F80`, scene setup `0x00FF63B0`,
  then `sUnit` vtable +0x40 = `vf16` above.
- `0x004B9660` (step 65): FRAME.md says it "queues every unit's draw". The
  code queues two other callbacks: `0x004BA000` = `mov eax,[ecx]; jmp
  [eax+0x8c]` (unit slot **35**, for units whose DTI chain contains
  `0x019955fc` and whose `+0x2398` bit 0 is set) and `0x010B7450` = `jmp
  [eax+0x40]` (unit slot **16**, every unit on every line), each batch joined
  with `0x00DBE560`. These are not slot 11. **UNKNOWN:** what slots 16 and 35
  do (suspect per-unit pre-draw or visibility/LOD prep). FRAME.md step 65
  should be corrected once known.
- **Simulation hidden in the render phase** (must not run per displayed frame
  unless made idempotent): `uSimSoftBody::vf11` 0x00F20640 (cloth/hair PBD
  solved in ~900 tiny render passes of 64x8/64x16 A32B32G32R32F targets;
  accumulates `acc += min(mDeltaTime/mFps, 0.05)`, runs n steps; SOFTBODY.md);
  `sGpuParticle::vf09/vf10` (GPU particle state advance; time source
  **UNKNOWN**); `sPrimitive::vf09/vf11/vf12`; `sArchiveManager::vf06` and
  `sEnvMapManager::vf06` (streaming; harmless per tick); occlusion queries
  (L120 async). The soft-body result (model-space positions) is read by scene
  and shadow vertex shaders (`tSoftBodySrcTex*`, `fWorld`).

### 3.3 Where the data lives

Offsets from decompiles in this task unless marked:

| Item | Location | Written | Read |
|---|---|---|---|
| position / rotation / scale | `uCoord` +0x40 (vec3 cm, floating-origin frame), +0x50 (quat), +0x60 (scale) | integration 0x0084CCC0, root motion 0x0105B740 | `uCoord::vf20` |
| **world matrix (4x4, row vectors)** | `uCoord` **+0x70..+0xAF**: rows at +0x70, +0x80, +0x90, **translation row +0xA0..+0xA8**, w=1 at +0xAC (`uCoord::vf20` 0x00DEABC0, called from `uObjModel::vf93` slot 20 +0x50). Note WORLD.md says "+0x90" is the translation row: it is row 2; the translation row is +0xA0 | phase 93 commit | draw, hit volumes, camera |
| model matrix | `uModel` **+0xB0..+0xEF**, translation row +0xE0 [doc COMBAT.md 274; +0xE0..+0xE8 is read as the position by 0x00888640] | commit | draw ("joint -1") |
| joint array | `uModel+0x364` pointer, count `+0x360`, stride **0xA0**: quat +0x60, scale +0x70, trans +0x80 (0x01058A70 pose blend) | motion advance 0x010629F0 (0x01060730 frame/blend) | skinning |
| frozen blend pose | `uModel+0xDD0`, stride 0x40 | pose freeze 0x01061DD0 on a motion change | blend 0x01058A70 |
| motion layers | `uModel+0x380`, 0x140 each, count +0x374 | motion advance | -- |
| joint **world** matrices (skin) | **UNKNOWN** (not reflected; COOP_ON_REBUILD 8.3 "to locate"). Phase 93 computes them after the pose (`0x00886280`, sub-steps selected by `+0x2398` bits) | phase 93 | draw (`uBaseModel::vf11` 0x00EA3130 -> `FFC350`/`EA5320` -> shader constants / `tMatrixMap`) |
| camera | `sCamera::vf06` per view record i: view matrix 16 f32 at `sCamera+0xC0+i*0x190` (from the camera object's vtable +0x50), projection at `+0x100` (vtable +0x54); **previous-frame copies** at `+0x140` / `+0x180`, copied only while not paused (offsets from the decompile, stride assumed from vf09's 400 bytes) | step 51 | `sCamera::vf09`, shader constants |
| step mask | `uObjModel+0x2398` (0x4 collision, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000 select sub-steps of vf93) | -- | `vf93` |

The renderer reads these at **draw job** time (slot 11 per view, on the main
thread and job workers), after the unit moved in the same frame, and turns them
into records. The render worker (`0x00DAC090`, guest thread 0x1034) executes
the *previous* frame's stream (NDRAW.md 3: "double buffering, not main-thread
D3D emission per draw"). The stream is `{u32 sort_key; u32 record_ptr}` entries
(pass id in the top 5 bits of the key, NDRAW.md 4); draw records point at
shader-binding value slots whose float constants (world matrix, skin palette,
view-projection) the executor reads at apply time (NDRAW.md 5; L180: the sink
re-reads them when a binding changes). That pointer indirection is what makes
a stream patchable.

---

## 4. What interpolation has to snapshot per tick

Snapshot point: after step 55 (after `sCamera::vf06`, at the original's network
step 54/55), once per tick. Keep two generations (prev, curr).

### 4.1 Mandatory

1. **Per drawn unit, keyed by stable identity** (unit pointer + a generation
   counter, so a freed slot reused by another unit is not matched): `uCoord`
   pos/quat/scale; the world matrix (+0x70) and `uModel` matrix (+0xB0); the
   joint pose (quat/scale/trans, stride 0xA0, count +0x360) or the joint
   world/skin matrices once located. Interpolate as quat slerp/nlerp + lerp of
   translation/scale, not raw matrices, so rotations do not shrink (a tick is
   small but swords and head turns reach tens of degrees per tick).
2. **Floating origin.** Positions are cell-local relative to `sWorldOffset`
   (step 10, 0x004C64E0; `mWorldAbsoluteOffset`). When the origin shifts the
   previous snapshot must shift by the same delta (or be stored in absolute
   coordinates), or every object flies one cell for a frame. COOP_ON_REBUILD.md
   7.2 already requires absolute coordinates for replication.
3. **Camera** per view record: eye, orientation, FOV, near/far. The existing
   prev-matrix fields (+0x140/+0x180) are tick-based and used by
   motion-vector-style effects: a replayed frame needs its own previous
   matrices (the interpolated pose of the previous *displayed* frame).
4. **Discontinuity flags:** teleport/warp, fast travel, area change, event
   start/end (camera cut), respawn, pawn warp, model rebuild, unit
   spawn/despawn. Rule: no prev entry, a flagged unit, or displacement over a
   threshold => snap to curr. Hooks: the unit state machine (`unit+4 & 7`: 1
   new, 2 live, 3, 4 delete) and `sUnit` add/remove (0x00DB5800/0x00DB58B0/
   0x00DB6BB0).

### 4.2 Hard cases

- **Effects and particles spawned mid-tick.** Effect units are ordinary units
  on lines 22-24 (or owner line + 1) with their own `uCoord`; a spawn has no
  prev (snap), a despawn keeps drawing until the next tick. **GPU particles
  (`sGpuParticle` vf09/vf10) advance in the render half**: move their step to
  the tick or, in replay mode, skip the step and reuse the last output (they
  then run at tick rate: fast sparks visibly 60 Hz, noticeable at 30 Hz).
  Emitters attached to a moving owner jump by one tick (<= 11 cm at sprint,
  60 Hz): invisible.
- **Cloth, hair, soft bodies** (`uSimSoftBody::vf11`, ~900 passes/frame): must
  run **once per tick, never per displayed frame** (replaying it advances the
  cloth 4x). The output is in model space (the last passes convert back,
  SOFTBODY.md), so interpolating the model matrix is enough; the cloth shape
  lags < 1 tick. In the replay route the ~900 tiny-RT passes are simply not
  replayed; in the in-engine route (A1) `vf11` needs a "tick has run" guard.
  `uCnsTinyChain` and `uChain` live in sim: nothing to do.
- **Skinned attachments** (weapons `uWeaponPl` on lines 11/16; hair, beard and
  outfit parts sharing the base skeleton, CHARACTERS.md): units with their own
  matrices derived from joint matrices in the same tick. Interpolating each
  independently differs from interpolating the product by O(step^2) (< 1 mm at
  swing speeds); acceptable. They need the same identity key so a weapon swap
  mid-tick snaps.
- **Shadows.** Cascade maps (`uSkyStableShadow` 0x00EE2C90, 512x1536 maps, ~10
  calls, 3.9 ms/frame CPU solo and the last job of the batch [prof]) are
  rendered from tick state. Reusing tick-N maps for displayed frames N..N+3
  makes shadows lag interpolated geometry by <= 1 tick (11 cm at sprint speed
  at 60 Hz, 22 cm at 30 Hz). The shadow matrices used by the main pass must be
  the same tick's as the map. Visual risk: contact-shadow slide on fast
  objects; mitigations: re-render shadows from the interpolated pose every
  second displayed frame, or offset the shadow camera by the camera delta.
  This is both the biggest cost win and the biggest visual risk.
- **HUD/GUI.** Screen-space; gauges that follow world points
  (`uGUINpcGauge`, `uGUINpcMessage`, minimap, lock-on sight) are placed in the
  tick from that tick's camera. Replayed unchanged they stick to the tick's
  screen position while the world moves under them. Options: GUI at tick rate,
  or re-project anchors with the interpolated camera. Fades are dt-driven and
  fine at tick rate.
- **Camera latency.** The camera controller (`cCamImp*` springs, collision
  `0x007E1400`) is sim. To keep mouse-look at render rate, apply the look delta
  since the last tick to yaw/pitch of the interpolated camera (late latching);
  collision stays tick-based. Optional.
- **Units whose matrix is computed in the draw.** **UNKNOWN** whether any unit's
  world matrix is only derived inside vf11. To check: `uBaseModel::vf11`
  0x00EA3130, `uPlayerBase::vf11` 0x00B6A070 (copies the fade multiplier to
  `mTransparency` +0x158, sets model flag 0x40000000), `uSwingModel::vf11`
  0x00FA5860. Fade itself is computed at tick rate (vf97, CAMERA.md).
- **Culling.** Per view in the draw job (`cull_bounds` 0x00EA42E0,
  `scene_collect_visible` 0x00FEB1F0). In replay the visible set is the
  tick's: an object entering view between ticks pops in <= 1 tick late
  (invisible at 240 Hz); a cull margin of one tick of motion removes it.

---

## 5. Architecture options

**A1. In-engine interpolation (validation harness).** On a displayed frame
that is not a tick, skip steps 1-55 (except the clock), write the interpolated
matrices into the live structures (uCoord +0x70, uModel +0xB0, joint arrays,
sCamera view records), run steps 56-65 normally, restore the tick state.
Production is paid per displayed frame, so it caps at about
1/(production + worker) ~= 60-100 fps. The gain over today is the skipped sim
share on in-between frames (sim share **UNKNOWN**; passes A/B/C are 22% of
main-thread CPU [prof]). It exercises the snapshot and discontinuity logic
against the real renderer with pixel-comparable output, which is why to build
it first; it is not the 240 route.

**A2. Retained command stream replay (the 240 route).** Production (steps
56-65) runs once per tick as today but the stream is kept, with a per-draw
patch table: for each draw, the constant slots that carry (a) world/model
matrix, (b) skin palette, (c) view/projection/camera position, (d)
previous-frame matrices. The ndraw sink replays the stream K = 2-8 times per
tick with constants lerped from (prev tick, curr tick) at alpha = k/K.
Tick-bound passes are replayed only on the first replay or reused: soft-body
sim passes, shadow cascades, reflection, GPU particle sim. Display-frame cost
= patch + sink + GPU; main thread and job workers are free for the next tick.
It is also frame pipelining (production of N+1 overlaps replay of N; the
original already double-buffers). Open problems: (i) draw identity across ticks
(draw i of tick N vs tick N-1: key by shader binding, VB ptr+offset, IB range,
nth occurrence); (ii) which constant registers hold which matrix (shader
constant tables exist; SOFTBODY.md/L113 read names like `fWorld`,
`fSbLWMatrix`, `CBSkinning`; **UNKNOWN** register map per shader family);
(iii) the original skips re-uploads when the value pointer repeats (L180,
0x00DAA8B0), so a patched replay must patch the memory behind the pointers and
force the re-upload.

**A3. Threaded sim.** A sim thread produces snapshots, a render thread
consumes them (A2 replay) at display rate. In the lifted runtime the sim half
is the main guest thread plus the job workers (`sMain::vf20` 0x00DBDC90); the
render worker is already a separate guest thread. The change is mostly to take
production off the display path (A2) and hand snapshots over through a
lock-free slot.

---

## 6. Routes to 240 fps

Budget for 240 fps: **4.17 ms per displayed frame**. Facts used [prof]:

- native original ~80 fps = 12.5 ms; ours ~62 fps = 16 ms.
- main thread serial: 12.7 ms/frame CPU in the ~50 fps phase, of which memory
  helpers `fast_u32` 1.80 + `fast_set_u32` 1.03 = 2.8 ms; Present wrapper
  0.72 (cached since), dispatch 0.46, join machinery 1.2 ms.
- batch of 298 jobs: mean 5.09 ms; the shadow job (`DB7620`/`EE2C90`) starts at
  +0.06 ms and runs 5.0 ms on one worker; all other jobs end by +3.26 ms: the
  shadow overhang is ~1.75 ms (skip-shadow ceiling 64.5/62.3 vs 57.5/57.2 fps).
- render worker 0x1034: 22.6 s of 30 s (75%) ~= 12-15 ms/frame at 50-60 fps;
  ~3,440 native scene draws at 3.94 us plus ~1,930 other draws still on the
  D3D9 path at ~3x cost; `draw_call` 21.8% self (~2.8 ms). The same
  `draw_call` from caches is 0.20-0.30 us in the L180 microbench: a ~13x gap
  that is decode and guest reads, not encode.
- constants: ~250 KB really change per frame, the translation uploads ~9 KB per
  draw (~48 MB/frame) [doc NATIVE_RENDERER.md].
- wgpu floor (L110): 0.88 us/draw (0.39 with bundles, 0.33 raw Vulkan); a
  render pass ~7 us in wgpu vs ~1 us in ash; ~900 of ~977 passes are soft-body
  sim.
- GPU time: **UNKNOWN** for this build at 1080p. Older tables show GPU 32-36%
  busy at 21 fps (combo21/24), which taken literally is ~15 ms/frame; DXVK
  runs the original at > 100 fps on this machine, so the original's GPU work is
  < 10 ms. 240 fps is infeasible if the GPU work per frame exceeds 4 ms;
  shadows (512x1536 x cascades), the 1080p scene and post-processing set it.
  Measure before anything else.

### 6.1 Route comparison

| Route | What | Evidence for gain | Estimated effect | Cost / risk |
|---|---|---|---|---|
| **A** decoupled sim/render + interpolation | A1 in-engine, A2 retained replay, A3 threads | skips sim on displayed frames; production once per tick | A1: 62 -> ~75-100 fps (sim share **UNKNOWN**); A2: display frame = patch + sink + GPU, independent of sim, production and shadow | A1 medium; A2 high (identity matching, constant map, tick-bound pass list); co-op synergy |
| **B** faster simulation/production | per-lane `sPrimitive` state (8 lanes, global 0x018D1848, vf11/vf12 at 0x0107FF00/0x0107FF30), shadow job parallelism, serial pass splitting, ports, memory helpers | shadow overhang 1.75 ms; skip-mode ceiling +7 fps (~10%); main serial 12.7 ms | 62 -> ~70 (shadow, if lanes proven) -> ~78-80 (ports/helpers): **native parity, not 240** | medium-high per item. L172 root cause: the view-mode crash is the 8 sPrimitive lane state objects, pushed/merged/popped by all lanes; cascade independence not proven; L191 ports crashed/needed fixes; 885DC0 still skipped |
| **C** renderer-side (ndraw) | delta-only constants, retained decoded draws, fewer/instanced draws, raw Vulkan or render bundles, compute for soft-body sim, native path for the ~1,930 D3D9-path draws | 3.94 us vs 0.2-0.3 us microbench; 48 MB vs 250 KB constants; ~900 sim passes; non-native draws 3x | worker 12.7 ms -> ~3 ms is the *necessary* part for 240 and helps every route | medium; mostly Rust we own (not lifted code); GPU limit unknown |
| **D** other | amortize tick-bound passes; frame pipelining; skip redundant per-frame work at high fps (streaming/archive/env map per tick; skip sim passes when replaying) | shadows 3.9 ms solo and last job; 900 soft-body passes; per-frame upkeep in steps 1-17 | removes ~30-40% of a displayed frame's work when replaying | low-medium; visual validation (shadow lag) |

### 6.2 Why B alone and A alone cannot reach 4 ms

- B: the tick still has to produce a frame's worth of draws. Native is 12.5 ms
  with the same game code; our serial main is 12.7 ms CPU. A perfect B (shadow
  overhang gone, helpers halved, passes ported) lands near 9-11 ms: ~90-110 fps.
- A (A1/A2) without C: replay cost = the sink = 12.7 ms/frame today => a
  ~78 fps ceiling however well the sim is decoupled.
- C without A: the render worker stops being the bound but main (12.7 ms CPU,
  44% waiting on the batch) remains: ~80-100 fps.
- Only **A2 + C (+ D)** reaches 4 ms, and only if the GPU does.

### 6.3 Estimated displayed-frame budget at 240 fps with A2 + C + D

| Stage | Today | Target | How |
|---|---|---|---|
| sim | in the frame | off the display path (tick thread) | A3 |
| production (sRender+camera 3.3 ms, draw batch ~3.3 ms wall, shadow tail 1.75 ms) | in the frame | once per tick; together with sim it must fit 16.6 ms (the *tick budget*, why B is still needed) | A2 |
| constant patching | -- | 0.2-0.4 ms (~4,000 draws, ~50 skinned models x ~100 joints) | A2 |
| sink decode+encode, main view only (~3,400 draws; ~1,500 shadow draws and ~900 soft-body passes not replayed) | 12.7 ms (all ~5,400) | 1.5-2.5 ms (cached decoded draws 0.25 us; wgpu 0.9 us/draw on 2 encode threads, or 0.33 us ash/bundles) | C |
| GPU | **UNKNOWN** | < 4 ms | measure; shadows amortized |
| present/pacing | `0x00DAED10` spin, Present wrapper | < 0.2 ms (vsync/flip, no spin) | C |
| **Total** | 16 ms | **~2.5-4 ms CPU side**, GPU-dependent | |

Honest read: the CPU side can plausibly reach 240; whether the 4060 draws the
shadow-amortized frame in 4 ms is open, and ~200 fps is a likely intermediate
outcome. The tick budget (sim + production in 16.6 ms at 60 Hz) is close to
today's whole frame; 30 Hz gives 2x headroom at the price of latency, so the
tick rate must be a switch.

---

## 7. Co-op fit

The fixed tick and the per-tick snapshot are what co-op needs (COOP_DESIGN.md:
host-authoritative, 1/30 s host frame by default, snapshots delta-encoded
against acked baselines, clients interpolate others and predict their own
Arisen; COOP_ON_REBUILD.md 4.2: snapshot at step 54).

- Snapshot point = after step 55 / at step 54: transforms (`uCoord` +0x40/
  +0x50, absolute = + `sWorldOffset`), motion layers (`uModel+0x380`, 0x140
  each: `mMotionNo` +0x04, `mState` +0x06, `mFrame` +0x4C, `mBlend` +0x48,
  `mInterFrame` +0x40, `mInterCount` +0x44, `mSpeed` +0x5C), animation step
  `+0x2EEC`, status bits `+0x26A0`, vitals. The render snapshot needs the
  *derived* joint pose; replication needs the *inputs* plus a replica move
  (COOP_ON_REBUILD 7.3: motion advance 0x00886210 and commit 0x00886280 only).
  Same cut, two consumers: on a client the render snapshot is the replica's
  committed pose.
- A replica client interpolates between received ticks exactly as the local
  renderer does between local ticks: **one interpolation implementation, fed by
  the local sim or by the network**.
- Determinism: lockstep stays excluded by job interleaving and the single
  `MtRandom` (0x018D017C); a constant tick removes the timing nondeterminism
  (`mDeltaTime` from QPC) but not thread interleaving.
- Each seat has its own camera (COOP_DESIGN); `sCamera` already has 8 view
  records, so the render snapshot is per view.

---

## 8. What to verify first (cheap, gates the rest)

| # | Question | How | Decides |
|---|---|---|---|
| V1 | sim share of a tick: steps 1-55 vs 56-65 | stage-probe timestamps around the two halves in one run | A1 gain; tick budget at 60 Hz |
| V2 | GPU frame time at 1080p, main view vs shadows | wgpu timestamp queries per pass | whether 4 ms is possible |
| V3 | per-pass draw counts and worker time (shadow, main, soft-body) | `frame_stats` (d3d9frame) / sink stats | tick-bound pass list |
| V4 | constant register map for world matrix, skin palette, view-proj, prev matrices per shader family | shader constant tables (sShader2/SPK reflection names), L113 | A2 patch feasibility |
| V5 | do draw streams match draw-for-draw between consecutive ticks (count, order, binding keys)? what fraction changes? | compare two consecutive captured streams (`.ddframe`) | identity-matching design |
| V6 | locate joint world/skin matrices in `uModel`; any unit whose matrix is computed only inside vf11 | trace `uObjModel::vf93` slot 20 and the constant sources of `0x00EA3130`/`0x00FFC350` | snapshot content |
| V7 | what slots 16 and 35 queued by `0x004B9660` do | decompile the DTI class methods | FRAME.md fix; per-frame work |
| V8 | `sGpuParticle` time source | trace 0x00EB2550 | tick-bound or not |
| V9 | identical frames: render at the tick rate with the virtual clock vs constant-dt oracle frames | frame oracle (FRAME_ORACLE.md) | stage 1 gate |
| V10 | interpolated frame at alpha=1 equals the tick frame bit-for-bit; alpha=0.5 against a true 2x-tick render of a scripted path | ddaccept scripted route in the sandbox | interpolation correctness |

---

## 9. Recommended plan

Order chosen so each stage is verifiable and pays off alone.

**Stage 0: measure (no behaviour change).** V1, V2, V3, V5, V7. Output: the
sim/production split, GPU per pass, draw-stream stability. Gate: if the GPU
needs more than ~4 ms for the main view (shadows excluded), re-scope to ~200
fps or cut render cost first.

**Stage 1: fixed tick, identical frames.** Virtual QPC (or `mDeltaTime`
override) giving a constant `SIM_DT` (60 Hz default, 30 Hz switch); rendering
stays 1:1 per tick. Verify: V9 (frame oracle against constant-dt recordings,
including the existing gate counts), the ddaccept replay at the constant step,
fps unchanged (~62) within noise, input latching for sub-tick presses.
Difficulty: low-medium; main risk is real-time consumers (movies, events,
`GetTickCount`) drifting when below real time. Co-op: this is the host tick.

**Stage 2: tick-rate snapshot + in-engine 2x interpolation (A1).** Snapshot
after step 55 with identity keys, floating-origin correction and discontinuity
flags; on odd frames skip sim, write interpolated matrices, run 56-65, restore;
guard `uSimSoftBody::vf11` and GPU particles to once per tick. Verify: V10,
then visual review of the hard cases (warps, events, spawns, weapons, cloth).
Gain: sim share removed on half the frames (62 -> ~75-100, V1-dependent).
Worth doing even if the gain is modest: it proves the snapshot contents (V6)
and discontinuity logic on the real renderer. Difficulty: medium-high.

**Stage 3: renderer-side prerequisites (C), in parallel with 1-2.** Delta
constant upload, retained decoded draws (target 0.25 us), move the ~1,930
D3D9-path draws native, soft-body sim as compute (`DDGPU_NATIVE_SIM` exists),
no spin at present. Verify with the existing native-vs-D3D9 compare (0
mismatches) at each step; fps by median of >= 2 runs (RULES.md). Independent
value: the render worker is the next bound at ~57 fps.

**Stage 4: retained stream replay (A2) at 4x.** Patch table from V4/V5,
tick-bound pass list from V3, replay of the main view with interpolated
matrices; shadows, soft-body and GPU particles at tick rate. Verify: replay at
alpha=1 pixel-identical to the tick frame; alpha steps continuous on a scripted
route; worker frame time vs target; frame pacing (240 Hz, no tearing, 1%
lows). Difficulty: high; this stage reaches 240.

**Stage 5: thread split (A3).** Sim thread produces, render thread consumes via
a snapshot slot; production off the display path. Verify: sim output unchanged
(replay equality), input-to-photon latency, tick overrun behaviour (drop to a
30 Hz tick rather than slowing the game).

**In parallel (B), only where it serves the tick budget:** the shadow overhang
and `sPrimitive` lane work from L172 (<= 10%, root cause documented), the
memory helpers (2.8 ms) and thin-dispatch ports. They matter because tick =
sim + production must fit 16.6 ms at 60 Hz; do not expect them to reach 240.

**Optional, later:** render-rate camera late-latching; re-projected HUD
anchors; extrapolation to cut the one-tick latency; an opt-in high-fps fixes
profile (FRAME_RATE.md 6), independent of the tick.

| Stage | Difficulty | fps expectation | Needed for 240 |
|---|---|---|---|
| 0 measure | low | -- | yes (gates) |
| 1 fixed tick | low-medium | ~62 | yes (foundation, co-op host) |
| 2 A1 interpolation | medium-high | 75-100 | validation only |
| 3 C renderer | medium | worker bound lifted | yes |
| 4 A2 replay | high | 150-240+ | yes |
| 5 threads | medium | stable pacing | yes for no-stutter |
| B items | medium-high each | +10-25% | no (tick budget only) |

---

## 10. Measurements (shadow agent, 2026-10-14; fast2 build, benchmark save, 1080p windowed, instrumentation in the L172 tree)

Switches (all off by default): `DDRUNTIME_MEASURE=1` (main-thread halves + worker per target size, report every 240 frames),
`DDRUNTIME_MEASURE_STREAM=1` (draw-stream stability; slows the game to ~25 fps, so only its ratios are meaningful),
`DDGPU_BUSY_STATS=1` (GPU busy from submit-completion callbacks, one line per second). Code: `crates/ddruntime/src/measure.rs`,
`crates/ddgpu/src/gpu_busy.rs` (task/L172-parallel-shadow, commits "L192: ..."). 3 measurement runs.

**V1 sim vs production (main thread, 7440 frames, mean ms/frame, with the timers on: ~44 fps run):**
frame 13.27 = **sim half 9.49 (entry .. sSceneExt::vf06 0x49B030, i.e. steps 1-55) + production half 3.78 (steps 56-65)**.
Inside: unit pass A (0xDB6740) 2.73, pass C (0xDB5690) 1.61, pass B 0.09; sRender 0xDAF000 2.73 (the draw job batch and camera setup are
here); `sShadow::vf06` 0xE277A0 only 0.09 and 0x4B9660 0.12 (the shadow *drawing* is a job inside sRender's batch, not these).
So sim is ~71% of the main thread's frame, passes A/B/C are 4.4 ms of it, and the remaining ~5 ms of the sim half is AI/input/effects/
sound/network. This **corrects the doc's "production ~6.6 ms" figure**: on the main thread production is 3.8 ms; the rest of the cost
is on the job workers and the render worker. A1 (skip steps 1-55 on in-between frames) can therefore save up to ~9.5 of 13.3 ms of
main-thread time per skipped frame, much more than the 22% (passes only) assumed in section 5.

**V3 render worker per target size (per frame; draws seen / sink-decode ms, decided draws only):** 1920x1080 main view 1448 / 5.38 ms;
512x1536 shadow maps 206 / 0.66; 64x8 soft-body 178 / 0.87; 64x16 soft-body 79 / 0.36; 960x540 67 / 0.26; 192x128 23 / 0.09.
Total decode ~7.6 ms/frame (this run has the timers' own cost). The "~900 soft-body passes" assumption does not hold in this
scene: 257 soft-body draws (64x8 + 64x16) per frame; and shadow maps are 206 draws, not ~1,500. The 1080p main view is 70% of
the worker's sink time and ~1,450 draws, not ~3,400.

**V2 GPU time: inconclusive, an upper bound only.** No timestamp queries exist on this path; the completion-callback metric says the
GPU queue is "busy" 97-99% of wall time (about 21-23 ms per present at ~44 presents/s, 650-730 submits/s ~ 15 submits per
frame). That cannot be read as 22 ms of GPU work: callbacks are delivered when wgpu polls, so a late poll counts as busy. It does not
rule out a GPU-bound frame either. **Before any 240-fps decision a real measurement is needed:** enable `Features::TIMESTAMP_QUERY`
and write timestamps at the start/end of the main pass and the shadow passes in `vk/pass.rs` (the passes are built in one place), or
compare fps at a lowered resolution (GPU-bound => fps rises). The cheap test: DDRUNTIME_DISPLAY / config Resolution=1280x720 run.

**V5 stream stability (consecutive frames, taken draws, 1080p main view):** loose key (shaders, declaration, primitive counts, texture
resources) equal at the same index 30.5%, strict key (also vertex buffers/offsets, render states, targets, viewport) 14.1%, loose
multiset overlap 32.8%. Shadow, soft-body and 192x128 draws: 0% on both keys. Draw streams are therefore **not** stable
draw-for-draw between consecutive frames with these keys: per-frame dynamic resources (stream offsets in ring buffers, render-target
and texture handles for shadow/soft-body targets, visibility changes) differ. The measurement used handles, so it is a lower bound for
"same draw"; matching by (vertex shader, pixel shader, declaration) alone was not measured. Route A2's identity matching must be
designed with the dynamic handles in mind (the draw index is not an identity), and the replay of shadow/soft-body passes (0%) must be
treated as tick-bound passes, as section 4.2 already assumes.

**Gotcha found while measuring:** the user's config.ini was changed to FullScreen=ON, RefreshRate=240Hz; the game then stops at boot
with "ERR09: Unsupported function" / MissingInstruction at 0x12F507F (device creation) in the sandbox. Runs must force
FullScreen=OFF in the sandbox copy.
