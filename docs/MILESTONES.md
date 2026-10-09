# Milestones

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
