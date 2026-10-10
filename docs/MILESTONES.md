# Milestones

**Current focus (2026-10-09): the opening slice.** M5 is now a vertical
slice from the new game to the Encampment, played in co-op; see
[OPENING_SLICE.md](OPENING_SLICE.md). Other systems advance only where the
slice needs them.

Each milestone is a route the players can actually do, with a gate checked
against the original game. Co-op comes early (M3) on purpose: it validates
the architecture before many systems are built on it.

| # | Route | Gate (checked against the original) |
| --- | --- | --- |
| **M0** | Foundation: tooling, exe identity, DTI registry in Ghidra, `.arc` reader, data inventory | Every `.arc` opens; DTI names applied (**passed** 2026-10-08) |
| **M1** | Look: Cassardis and its surroundings render like the original (terrain layers, lighting, water, fog, sky, weather, time of day) | Screenshot comparison at recorded camera poses, morning/noon/night |
| **M2** | Walk: the Arisen moves, animates, collides, climbs and spends stamina like the original | Recorded paths, speeds, jump arcs and stamina curves match within tolerance |
| **M3** | Together: LAN host and client, two Arisen in Cassardis, shared main pawn | Both see each other move smoothly; client input applied by the host |
| **M4** | Fight: melee and magic, one enemy family (goblins), status effects, the main pawn fighting and talking | Damage, stagger, AI choices and pawn chatter match a recorded fight |
| **M5** | The opening: character creation, the prologue, the Cassardis dragon attack, the first quests | The route plays through; quest and flag state matches the original's save after each step |
| **M6** | Gran Soren and the core loop: shops, forging, inns and storage, the Rift and hiring pawns, notice-board quests, NPC schedules, day/night, fast travel | Shop stock and prices, Rift costs, schedules and quest flags match the original |
| **M7** | Gransys: every region, enemy, vocation, skill and augment; streaming and loading screens | Every area loads; spawn tables and stats match the data and recordings |
| **M8** | The campaign: every main and side quest, both endings, the post-game Everfall | Full playthrough; save state comparable with the original at each chapter |
| **M9** | Bitterblack Isle (Dark Arisen) | Full Bitterblack run |
| **M10** | Release quality: settings, saves, performance, FPS unlock, co-op polish | Stable sessions; no regressions on the acceptance routes |

Research runs ahead of the active milestone. The systems are broken down in
[ROADMAP.md](ROADMAP.md), work items in [tasks/](tasks/README.md), and the
multi-agent process in [BOARD.md](BOARD.md). The original game is observed
through the in-process oracle ([tasks/oracle.md](tasks/oracle.md)) and the
reference recordings in `E:\ddda-re` (private).

### Research documents

| Topic | Doc | Status |
|---|---|---|
| Formats | FORMATS.md, TEXT.md, PARAMS.md, ENEMIES.md, SURVEY.md | merged |
| World | WORLD.md | merged |
| Movement, animation | MOVEMENT.md, ANIMATION.md | merged; ledge and blend in progress |
| Shading | SHADING.md | in progress (claude-3) |
| Pawns and the Rift | PAWNS.md | in progress (claude-5) |
| Shops and economy | SHOPS.md | in progress (claude-6) |
| Events, cutscenes, quests | EVENTS.md, QUESTS.md | in progress (claude-7) |
| AI | AI.md | in progress (codex-5) |
| Weather, NPCs, GUI, map, sound, effects | WEATHER/NPCS/GUI/MAP/SOUND/EFFECTS.md | assigned to the Codex lane |
| Live oracle | ORACLE.md | in progress (claude-4) |

## Status

**Done: M0** (2026-10-08)

Done:
- JDK 21 (Temurin 21.0.12) installed; Ghidra 12.1.4 runs headless.
- Analysis copy of DDDA.exe unpacked with Steamless 3.1.0.5 (SteamStub 3.1).
- Exe imported into `E:\ddda-re\ghidra\DDDA.gpr`; auto-analysis done
  (11.5 min, 33,870 functions). Backup in `E:\ddda-re\ghidra-backups`.
- GhidraMCP 6.0.0 built against 12.1.4, running on 127.0.0.1:8089,
  registered with Claude Code as `ghidra-ddda` (user scope).
- Repo created with a whitelist `.gitignore`, AGENTS.md, method and co-op
  design docs.
- Format survey (`E:\ddda-re\notes\formats-survey.md`): 8,536 `.arc` v7
  archives, 358k zlib entries, 144 resource types; MIT specs available
  (Albam, Riftstone).
- DTI class registry harvested: 4,405 classes, 4,343 vtables, 25,791
  virtual functions named ([DTI.md](DTI.md)). Reproducible from
  `research/dti` (identical label set verified).
- DTI labels applied to the Ghidra project. Vtable runs trimmed at the
  first non-code word (143 vtables); the 57 functions an earlier run had
  created at non-code addresses were removed. Final state 2026-10-08
  20:10: 30,540 functions and 13,153 data labels named, 0 failures.
- `mtarc` crate (ARC v7 reader) and `ddtool` CLI. **Gate passed
  2026-10-08:** `ddtool arc-scan <nativePC> --inflate` opens all 8,536
  archives, inflates all 358,431 entries to their recorded sizes, 144
  types all named from the DTI registry, 0 failures, 33 s.

**Active: M1** (Look: one area with a free camera)

Done (format readers; checked by re-running the scans 2026-10-08 20:05):
- `mttex`: all 73,706 `rTexture` v0x99 parse. Format byte to D3DFORMAT
  is the exe's own table at `0x01440698` (byte-for-byte; one copying
  slip at index 4 fixed).
- `mtmod`, `mtmrl`, `mtshader`: all 28,094 models (70.2M vertices, 71.3M
  triangles) and 28,152 material lists parse. All 2,077,947 material
  commands resolve through `BbsShaderPackage.mfx`, which also gives the 24
  vertex layouts. Format notes are in [FORMATS.md](FORMATS.md).
- World placement rule from the live game ([WORLD.md](WORLD.md)).

Open data anomalies (unexplained, not yet ruled harmless):
- 123,205 of 70.2M normals off unit length by more than 0.05.
- 25,811 skinned vertices reference a joint at or above the bone count.

Viewer (`ddvfs`, `ddview`), first renders 2026-10-08 20:30:
- Fixed before it would build or show anything: a compile error, the cell
  model path (folder comes from A, not B), corrupted build artifacts
  from the drive failure (`cargo clean`), and screenshots taken before
  Bevy's async pipeline compilation finished (`--shot-delay`, default
  10 s).
- 5x5 cells around `st100_58m50n` load and join seamlessly (25 cells,
  231 meshes, 56,527 triangles, 28 textures, 0 problems): the placement
  rule holds. Triangles are CCW-front against the stored normals (12,000
  vs 2 in four cells), so Bevy's default culling is right.
- The ground-level shot from the live player pose shows plausible terrain.
  **Not yet compared with the original game**: no reference screenshot.

Reference comparison (2026-10-08, private `E:\ddda-re\shots\refs`):
- Screenshot listener records the live camera pose (uCameraGame +0x30..+0x6c)
  at each F12. Six usable references from two areas; poses checked by a
  ground fit (player feet on the origin cell's terrain within 1 cm for
  ref001, 002, 004, 005). ref006 unverified, ref007 uncertain.
- **No mirroring.** Terrain, coast and sea sides match in every reference.
- Static scenery (`_s00` layouts via `mtlot`) lands where the original has
  it: a sword prop (ref001) and a large tree (ref004) match exactly.
- Water (sea plane at y 30000 plus river/lake/wave models, stage frame) and
  a ring of distant `fmfore` cell models are drawn. Water material is a
  placeholder.

M1 gate comparison (2026-10-09, branch task/m1-gate; side-by-sides in
the private `E:\ddda-re\shots`, rendered with `ddview --ref`):
- Since the list above: terrain layers, the original's lighting (SH,
  sun with shadow map, linear fog), the stage filter chain (vignette and
  tone curve), the Rayleigh/Mie sky dome, water materials, the castle and
  the other `uFmSetBase` landmarks, the far cell terrain, the stage
  low models (`st100_all00` and the area backdrops: the horizon mountains)
  and the primitives' pass and LOD rules are drawn ([SHADING.md](SHADING.md)).
- Without `--light` the lighting, fog and sky come from the game data for
  a clock and weather (`--clock`, `--weather`; default 12:00, fine).
  The sun light colour reproduces the four live day reads within 1 %.
- ref001 (castle view, clock unknown, at 12:00 fine): mean colour of the
  frame (8-bit sRGB) ref (97,103,103) / ours (100,107,107); distant
  mountain (106,142,186) / (104,140,185); mid ground (110,127,139) /
  (110,129,142). The haze, the horizon mountains and the cypresses match.
- ref002 (coast cliff, 12:00 fine): frame (98,106,108) / (113,120,121);
  the party, the HUD and the event chest are not drawn.
- ref004 / ref005 (coast with a tree, 12:00 fine): frame (80,92,89) /
  (113,122,111) and (105,116,119) / (119,129,130). The original's clock
  is unknown; its tree shadow shows a lower sun than 12:00.
- refs-lit 12:21 and 17:45 (live lighting, cloudy weather found from the
  fog): ground (30,39,40) / (31,40,41) and (26,34,34) / (25,34,35); a
  second ground box matches to the level.
- Night and water (2026-10-09, branch task/m1-night-water): the sea is
  unlit forward with the environment cubes (traced shaders), the cloud
  domes of the weather turn the horizon blue under the fog, the sky spot
  light lights the ground at night, and the stage DOF is drawn. At 12:00
  fine: sea ref / ours ref001 (63,78,96) / (48,62,77), ref004 (56,68,78) /
  (44,55,62) (was (74,97,116)); horizon ref004 (142,166,189) /
  (134,153,169) (was (184,184,184)), ref002 (138,160,183) / (129,146,163);
  refs-lit 23:04 ground (5,9,11) / (5,13,14) (was (1,4,5)); the day
  captures are unchanged (within 1-2 levels).
- Gaps: stars and the moon; night point lights (lamps, lantern) and the
  moon light; the water's normal maps and the foam effect; characters and
  event objects. Captures with the clock and weather recorded (sea and
  sky in view, a lamp-lit night) would let refs 001-005 be compared at
  their real time.

**Started: M2** (Walk), alongside the M1 shading gaps, at the maintainer's
request (2026-10-08). See [ANIMATION.md](ANIMATION.md).

Done:
- `mtlmt`: every motion list decodes (4,313 files, 92.2 M keys) with value
  checks (key spans, unit quaternions).
- Skinning fixed and explained: skinned vertices use a per-model skin space
  D (`inverse_bind x rest world`, identical for every bone of 400 models),
  not the bounding box; `IASkinBridge2wt` joint bytes.
- `ddview --character`: the male body plays motions. Slot 0 of
  `m0000_co` is a standing idle and slot 3 a walk cycle; correct by eye,
  **not yet compared with the original game**.

- Motion ids from the exe: `cPlActWait` / `cPlActWalk` / `cPlActRun`
  request motions 0 / 3 / 4 (`bank << 8 | slot`, bank 0 = `m0000_co`),
  confirming the slots identified by eye; walk starts on frame 0 or 18.
- Root motion (joint 255) drives movement: walk 198.5 cm / 38 frames,
  run 423.0 cm / 22 frames (1.57 and 5.77 m/s at the assumed 30 fps).
- `ddsim` (headless, N players): input -> action (wait/walk/run) ->
  root-motion movement -> ground height. `ddworld::Terrain` gives ground
  height from the cell terrain; it matches the four verified references'
  recorded feet within 1.1 cm.
- `ddview --play` / `--demo`: two local players (P1 WASD + LShift, P2
  arrows + RShift), follow camera on P1. Seen working with scripted input.

Not yet: wall/object collision (SBC), turning rate, walk/run thresholds,
blending between motions (the original blends; we switch), the other
motion banks, a dressed Arisen.

- `mtsbc` reads every collision mesh (7,362 files, 3.28 M triangles, all
  normals consistent). `--play` now stands on the cells' collision meshes
  (within 1 cm of the references) and is blocked by faces higher than a
  step (STEP_UP 60 cm, placeholder).

- Movement compared with a recording of the original (read-only memory,
  2026-10-08): walk, run and sprint move by a velocity from
  `m_pl000_00` (`rAdjustParam`), not root motion. Run velocity matches the
  recorded 10.165 cm/frame exactly; ground speed is 3-10% under the
  recorded path speed (open). Jump apex 92.6 cm vs 93.1 recorded. The
  simulation steps at 120 Hz. Town and field walk/run variants. Sprint-start
  ramp. Second jump plays the dagger roll. See MOVEMENT.md.
- Walls: players collide with steep triangles of the stage collision and
  slide along them. Only the `h` meshes are movement collision (`e` is
  `cSplitSbc::mpClsnEffMrgRsrc`, effects). Big trees block; bushes and
  shrubs have no stage collision, as in the data.

Blockers: none. Open: turn rate, run/sprint stop motions, ledge detection
from `cPlCliffHangBegin`, motion blending.

Next action: maintainer playtest of `--play` (speeds, walls, ledges); then
ledge detection from the exe.

## Batch 2026-10-09 (overnight, multi-agent)

The head coordinated about 20 agents through the board (`docs/BOARD.md`).

**Merged into main** (each checked by `scripts/merge_task.py`: no
conflict markers, tests pass):

| Area | Briefs |
|---|---|
| Readers | text (`mtgmd`), player params (`ddparam`), enemy data (`mtprp`, `ddenemy`), AI state machines (`mtfsm`), weather (`mtweather`), map and travel (`ddmap`), GUI (`mtgui`), NPC links (`ddnpc`), sound (`mtsound`), effects (`mteffect`) |
| Research | pawns and the Rift (`ddsave`, `ddpawn`, PAWNS.md), stamina rules, asset survey (SURVEY.md) |
| Core | ledges (ddsim, `Ground::ray`/`sphere`), motion blending (`mtlmt::blend`) |
| Tools | the in-process oracle (`research/ddda-oracle`, ORACLE.md), MtDTI reflected fields (`research/dti/dti_props.py`) |

**Compared with the original:**
- The clock: 1/30-s units, scaled by a per-frame `mDeltaTime` from the QPC
  timer (RECORDINGS.md; the formula was traced by codex-11).
- Sprint stamina: -1.5 per frame. Recovery: +1.625 per frame, after a
  0.59-0.61 s hold.
- Keyboard turning is instant.
- Blend lengths and countdown, through an oracle trace (ANIMATION.md).
- The offline Rift pool matches the level table, and hire costs are
  checked against a live read (PAWNS.md).

**In progress:**
- Claude lane: terrain shading (claude-3), shops (claude-6), events and
  quests (claude-7), combat (claude-8), vocations (claude-9), pawn builder
  (claude-5).
- Codex lane: rift-ui, save-pawns, enemy resource selection, weather
  clock, travel arrival.
- Live: the ledge jump/fall rule (claude-4).

**Blockers:** none. The mage save limits live checks of vocation-specific
moves such as the dagger double jump.

**Next action:** finish the M1 shading gate and the M2 gate. That means
recorded paths, speeds, jump arcs and stamina through the oracle against
`ddsim`, as an automated acceptance test.

## Batch 2026-10-09 (morning)

**M1 (look):** blended terrain layers, lighting from the game data by
time and weather, the sky's scatter shaders, the stage's tone curve and
vignette, water materials and reflections, the backdrop models, the far
ring, foliage billboards, depth of field.
- ref001 is within 3-4% of mean colour.
- The day captures are within 1-3 levels.
- Open: stars, moon, lamp lights (they need a Cassardis night capture,
  waiting on the maintainer), the Bluemoon Tower detail.

See SHADING.md and the M1 section above.

**M2 (walk):** acceptance replay (`ddaccept`) at 40 of 44 rows, up from 16.
- Done: velocity from data, the motion speed rate, stamina, the sprint
  stop, falls, landing holds, load and body classes, and the
  `cScrAdjust` capsule sweep and slide.
- Open: the drop path after frame 195. It needs a live trace, and the
  game is paused on the maintainer's fight.

**M3 (together):** `ddhost` (FRAME.md, ADR 0002), with economy, Rift and
progression wired in, and two-player save/load with stable hashes. `ddnet`
LAN co-op passes loopback tests under 10% loss and 40 ± 20 ms (prediction
error ≤ 5.7 cm). `ddview --host/--join` is in progress (claude-10).

**M4 (fight):** `ddcombat` covers hit volumes on joints, the exact shape
tests, hit history, the damage formulas and region pools; the goblin's
first swing hits exactly once, on frame 45. `ddai` runs the goblin's FSM.
The think tables are in progress (claude-9). Status effects: build-up
only.

**M5/M6 groundwork:**
- `ddstory` runs the opening (q0000 → q0002) in the traced frame order.
- The event player plays st200 ev10 with its cameras, subtitles and NPC
  outfits.
- Characters are assembled from the save: one skeleton, part masks,
  morphs checked bit for bit against the exe, weapons.
- `ddeconomy`, `ddrift` and `ddprogress` runtimes exist.

**Next actions:**
- the M3 GUI join test;
- the M4 think tables;
- play from the save in `--play`, once claude-10 releases `play.rs`;
- the live trace for the drop path.

**M3 two-window check (head, 2026-10-09 14:16):** on one PC, a host and a
guest started from the same save:

```
ddview --host 47812 --save <save> --goblin --demo
ddview --join 127.0.0.1:47812 --save <save>
```

- The guest joined within 5 s, at the host's spot (pre-join).
- Both logs report identical positions for seats 0, 1 and 4, all three
  dressed, and the host's goblin replicated to the guest (HP 0/1000,
  dead).
- The host's screenshot shows the guest's dressed Arisen beside its own
  party. The guest's shows the host's Arisen and pawn.
- Not checked: two physical machines, or a guest with a different save.

**M5 opening slice (codex-18, merged 0c291a6, 2026-10-09):** the
Cassardis opening plays through in LAN co-op with no teleports:
- st200 ev10 → control → Quina conversation;
- a collision-only, 197-waypoint walk into the village → ev15 → q0002
  boot;
- the end state matches the original's: SceNo 1400, GSF 372 and 381 set,
  q0001 bits `[18000000, 0, 3]`.

The guest shows the faces and subtitles, and the host's and guest's
cameras restore the gameplay projection after each event. Test:
`ddview` 15 release tests plus the Cassardis and story-loopback
regressions.

Open:
- the prologue fast-forward is a stub;
- guest dialogue and menus;
- the event freeze for NPC group 57 (Quina moves during shared events);
- the elder's ear and brow culling (cutscene-visuals);
- an ev15 frame-300 view of roof and sky.

**Batch 2026-10-09 evening (head + Codex lane), all on main:**
- **Movement feel (head):**
  - turning recorded with the oracle (adjust 3);
  - drawing between fixed frames (`play::Between`);
  - air attacks (rows 0x302);
  - the hard landing with fall damage.
- **Field streaming and the stage handoff (head):**
  - `ddworld::stream`: the grid comes from the exe, the 5/5/3 windows from
    0x00507440, the predicate from 0x005075d0;
  - `Streamer::enter`: door scenarios and JumpArea, with test
    `the_cassardis_gate_arrives_on_field_ground`.
- **Movies (head):** `mtmovie` decodes the WMVs with Media Foundation;
  `ddview --movie st100ev00` plays them.
- **Character edit, first version (head):** `ddview --charaedit`; the 41
  fields rebuild the body through `edit_look`; Enter fills `EditResult`.
- **Audio (codex-1):** event streams, conditional voices, motion sounds by
  surface, random selection.
- **Opening slice:**
  - prologue setup and encounter facts (codex-27);
  - q0002 records, live facts and the opening's stage handoff (codex-26);
  - dragon AI, resources, body and hover (codex-25);
  - chimera catalogue and multi-part damage (codex-28).
- **Pawn-actor brief:** saved pawn records into active fighters (codex-1).
- **Incident:** E: dropped off at 15:11 (a loose SATA cable). The repo was
  verified with `git fsck`. Bundles are now kept in `C:\ddda-backup`.

Blockers:
- the chimera fight (prologue);
- the dragon fight;
- the Rook actor (pawn-actor);
- the NPC applied controllers for q0002 (codex-26);
- the opening hookup of the movie and the editor.

Next action: q0003 trace (road to the Encampment), head.

**Batch 2026-10-09 night (head, from the maintainer's playtests):**
- **The game clock in play** advances at the original's rate (0x0043f2c0)
  and relights the stage and sky each game minute, spread over frames.
- **Performance:**
  - the clock's relighting was the 400-650 ms stutter;
  - the sun shadow now follows the stage's `uSkyStableShadow`: one map over
    60 m instead of four cascades over 1 km;
  - whole-stage parts respect their shadow-cast flag;
  - the distant ring is 6 cells.
  - Cassardis walk 46 → 12 ms; field 18 → 11 ms. `--perf` logs frame
    times and the main systems.
- **Cassardis rendering:**
  - one LOD per model, measured to the model's bounding-sphere centre
    (0x00ea5320, 0x00ea3d00; `mLODType` automatic from 0x00c25234), which
    fixed the z-fighting;
  - the opening's camera gets collision.
- **Camera collision as the original** (0x007de570): a ray, then a 1.3 x
  near-plane sphere.
- **Tools:** `--probe` (F8 crosshair, F9 logs the surfaces under it,
  drawn or hidden, with LOD masks).

Briefs queued for the Codex lane: camera-collision follow-ups,
character-shadows, skill-vfx.

**Batch 2026-10-09 late (head + Codex lane, from the maintainer's playtests):**
- **Floor shadows:** the original's two `uAmbientShadow` per player body
  (0x00B565A0), drawn straight down from joints 0x10/0x14 (ankles), 50 cm
  wide, 1-100 cm deep with both decays (vf11 0x00E7B6F0, 0x00E7AB30);
  blend state still a viewer choice (shadows.rs).
- **Camera:** sweep from the target outward to the nearest obstruction
  (0x010FDEA0, 0.1 cm response, attribute 0x10 ignored), Cassardis
  collision keeps its flags, the near-camera actor fade (0x0088BDC0
  thresholds), actor-offset shift traced (codex-30).
- **"Missing ground" in Cassardis is the original's own hole**, hidden by
  an auto-closing door (maintainer, in the original). The LOD rule was
  confirmed live on the field (codex-29: camera eye to the model's world
  sphere centre, rModel mid/low, ViewRange NORMAL = 1). New brief: doors.
- **Arisen look:** the omitted cBodyEdit controller (37 joints, sex tables,
  slender/muscle/bust) and the hand-joint scale rule (codex-1).
- **Opening route:** dragon pass C, chimera ThinkInitialization, NPC
  placement/intel lifetime/name families/Guard contracts (codex-1/28/31);
  the route harness reaches the real gate and the field (codex-29), next
  blocker the field detection-range rule 0x00443A60 (codex-1).
- **Perf:** `--perf` render diagnostics; room median 6.9 ms; field spikes
  up to 55 ms under investigation with the route harness.
- **Tools:** the probe logs collision distance per hit.

Next: doors (codex-32), field detection rule (codex-1), NPC adapter in the
opening (codex-31), route perf (codex-29).

**Batch 2026-10-10 (head + Codex on Sol 6.1 with step tasks):**
- **Process:** the Codex lane now runs on step tasks (`docs/tasks/steps/`,
  S01-S28): one moderate problem each, with known facts and a done check;
  head reviews, merges and writes the next ones from the Results.
- **Classes:** the Strider (dagger list 0x014F86B8, AirS1/AirS2/AirL, rows
  0x38-0x3A, cost 0x1BA, check 10 height 0x75; kit lvstrider + dagger
  0x195); the weapon table's choice applies the vocation's kit
  (`Host::apply_job_kit`) and starting weapons on the body. The bow is
  traced (secondary list 0x014F8198, command 7 / bit 0x8, `cPlActWpnBow`
  vtable 0x015E2F08) and queued (S25-S27).
- **Combat in the opening:** the opening host has a fight; field cells'
  gated enemy groups spawn into it; party HUD; the small-enemy gauge
  (last damaged goblin, 450 frames, 0x0067C110); hired pawns (Rook) get a
  body, a fight seat and their record's look.
- **Cutscene faces:** skinned parts were culled by rest-pose bounds (S10,
  S11: ear, hair, brows); the face's special constraints and their joint
  order (S12, 0x00812330, 0x01054540..0x01054690).
- **Opening flow:** the movie at q0000's awaited PlayEvent with the traced
  skip input (S08, S16) and its subtitles (S19); field NPC admission
  including the global (0,0) layouts (S13); the second Arisen dressed (S20).
- **Cyclops (on hold with the Encampment):** action enter, placement HP/legs,
  think driver, navigation services (S05, S06, S14, S17).
- **Blocker found:** after ev15 the story waits for NPC 1's Guard TalkEnd
  (S23, in progress: the authentic bb3290 fallback).
- **Plan:** the prologue is for later; the dragon attack is required (S24);
  the Encampment waits for the maintainer's playtest.
- **Disk:** E: fell to 27 GB; the 43 GB stale debug target was removed.

**Batch 2026-10-10, afternoon (head + Codex children 54/58/60/61/62):**
- **Route:** NPC 1's Guard TalkEnd -> GSF381 -> q0002 boots (S23); field
  goblins bind to event1's EnemyCheck and killing group 23 sets q2:20
  through the authored path; Reynard keeps his body after the admission
  LOT closes (closed spawn gate != removal, 0x0075F860 vs 0x0075FAA0,
  S32) and plays his authored motions; his thanks, reward 1058 and q2:88
  run from n304QuestCtrl (S39); the demo walks into event1's Area
  (RescueApproach) and on toward q0002_event2 (RoadWalk).
- **Enemies:** goblin RunTrace/RunEnd/RunStumble (far goblins used to
  stand still); the vf33 stat defaults (HP x2 -> 2000, attack x1.3) and
  the placements' cSetInfoPawn rates (rescue goblins attack x0.2);
  experience 100 per goblin (or mExperienceOW) through the party award,
  level-ups reaching the fighter (S37 core). Reynard as a combat body
  with source sensor selection and NoDamage/Invincible gates (S34, in
  progress).
- **Weapons and items:** the Strider's bow: mode and phases (S25), arrows
  that hit from the real bow joint (S26), in the viewer with the TPS
  camera and layered upper-body motion (S27); the new-game bag, rewards
  and quick use (S28).
- **Dragon:** host actor with its entrance (S29); landing/PressAttack and
  breath in progress (S30); LAN guests see every enemy kind, protocol 7
  (S33).
- **Viewer:** failed runs exit non-zero; `--no-vsync` (implied by perf
  runs). Field perf, uncapped: median ~22 ms, our systems ~3 ms; the rest
  is render-side (S41).
- **Next steps queued:** S35 vocation GUI, S36 pawn commands, S38 item
  menu, S40 event cameras (camera 12 = cCamExParamAnim, 3 s, blocks the
  player), S41 frame rate (180 fps target), S42 graphics settings, S43
  level-up notice.

**Batch 2026-10-10, evening (head + Codex 61..78):**
- **Rescue playable as authored:** enemy perception (S49: sensor queries,
  alert states, hate decay); Reynard's HP from his placement (x2.5 = 2500,
  0x00BBC4E0); the native gauge veto of party hits on gauged NPCs (S54,
  0x00BB7680, region-independent; the HP1/escape civilian rule is Japan-only).
  From the RescueApproach checkpoint: q2:20, only rescue goblins hurt him.
- **Tutorial UI from the install:** vocation menu (S35, uGUIJobChange),
  level-up notice (S43), item menu over host bags (S38), graphics settings
  (S42: own INI, read-only import, live VSync/DoF/shadow/sampler/FOV),
  goblin appearance and equipment by placement (S47: early goblins bare).
- **State and co-op:** host-owned bags saved with checkpoints (S51); running
  quest scripts saved/restored (S48, native STOR + optional STCX); profile
  parking on leave/rejoin (S55); LAN quest cameras, NPC combat state and
  stage (S44, protocol 9); pawn orders Go/Come/Help (S36, protocol 8).
- **Tooling:** route checkpoints and resume (S46, `DDVIEW_OPENING_FROM`),
  `DDVIEW_TIME_SCALE` for unattended runs, GPU check policy (one batched
  run, bounded, from checkpoints), board status fix.
- **Performance (S41):** named Bevy traces; compact per-submesh skin palettes
  by default: skin upload 17.8 -> 2.05 MB/frame (-88%), field median
  14.2 -> ~11 ms (GPU clocks covary; no isolated fps claim).
- **Whole route, one run (2026-10-10):** the unattended x4 route plays from the
  intro room through Quina, the gate, the field, the rescue (q2:20, Reynard
  1594/2500 HP, Arisen level 2), Reynard's talks and the road to q0002_event2:
  q2:10 and q0003 boot, exit 0 in 2 min 24 s wall. It needed one fix: the
  push-out moved the Arisen without the ground sweep (0x0076D6B0 runs before
  the scenery sweep), so a goblin push uphill buried the body 17 cm under
  the slope and froze RoadWalk (`Sim::push_player`, cbc5b85). Route-v2
  checkpoints regenerated from this run, Done included.
- **Merged on the way:** S30 static coordinate basis and primary parent
  binding, S53 death helper with atomic `Host::load` prevalidation, S41
  finite shadow caster extent (opt-in; no measured gain).
- **Pawn chatter (S52, protocol 10):** the installed situation/message
  tables, native recognition clocks from the seated cameras' views (union
  over local seats, no proximity fallback), reliable per-profile
  PawnMessage, and the opening's presenter hooks (de93e84).
- **Character creation (S56):** the installed editor flow: sex, name with
  the exe's charset/blacklist rules, 16 installed presets, confirmation;
  per-profile EDIT in the save; names on the S35/S43 GUIs. Two-profile
  capture checked.
- **Death and retry (S53, protocol 11):** grounded death phases, game over
  from an authoritative DeathReport, atomic retry (candidate host, then an
  infallible swap; live roster and connections kept), the six native vital
  fields saved per profile (VITL), epoch-tagged reliable messages.
- **Pawn voice and names (S52):** voices read from `rom/sound/pwn.pck` with
  the exe's own CRC32 path lookup; story pawn names localized like the
  original (English "Rook"); checked in an actual capture.
- **Beach scene groundwork (S31):** native Demo reactions, NPC capsules from
  the installed adjust table, damage edges, and the native editor exit to
  stage 200/start 3 with the prologue-end flag that gates it.
- **Dragon fire (S30):** coordinate basis/parent binding, generator inputs
  and initialization, each step checked against the original's instructions.
- **New steps:** S57 base vocation starting skills (codex-81), S58 equipment
  menu, S59 navigation (codex-82; the beach movement needs it), S60 the
  tutorial's numbers against the running original.
- **Vertical slice wrap-up (2026-10-10 evening):** final unattended route on
  main (after S53/S52/S31/S30/S59 merges) plays intro room -> Quina -> gate
  -> field -> rescue (q2:20) -> Reynard -> road -> q2:10 + q0003 boot, exit
  0, 2 min 30 s wall at x4. Route-v2 checkpoints regenerated from it.
  Run: `DDVIEW_DEMO_FIGHT=1 DDVIEW_TIME_SCALE=4 ddview <nativePC>
  --perf-route --unattended`; resume any step with `DDVIEW_OPENING_FROM=
  E:/ddda-re/notes/checkpoints/route-v2/<Step>.ddhs DDVIEW_OPENING_STEP=<Step>`.
  Paused for quota with handoff notes on their branches (not merged):
  S31 beach scene (`step/s31-dragon-route`, exit helper; GotoEx needs S59),
  S59 path query (`step/s59-navigation`), S30 fire RNG tables/birth adapters
  (`step/s30-dragon-land-fire`), S57 starting skills
  (`step/s57-base-skills`; Blink, Summons, Anodyne, contacts; tests unrun).
  Not started: S58 equipment menu, S60 oracle numbers, S41 frame rate.
- **Open:** dragon fire emission/damage (S30), beach scene wiring (S31,
  codex-80), navigation (S59), starting skills (S57), frame rate toward
  180 fps (S41: ~10.6 ms field frame vs 5.5 ms), Ghidra MCP down since the
  low-memory stop.
