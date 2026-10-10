# The opening slice

The project's current focus (maintainer, 2026-10-09). The aim is one
vertical slice of the real game, played start to finish in co-op, rather
than every system at once. Every system it needs is built in full,
driven by data, and checked against the original.

## Why a slice, and the rule that keeps it general

The opening exercises almost every core system:
- movement and animation;
- combat against several enemy families;
- a boss;
- cutscenes, movie and audio;
- character creation;
- pawns;
- quests;
- the HUD;
- field streaming.

Playing it end to end finds the integration and feel problems that
headless tests miss.

The risk of a slice is hard-coding: "if this is the dragon fight, do X".
The rule against it is simple. **A code path is specific to one event
only where the original exe is.** The original does have per-event
special cases (for example the event start code in EVENTS.md), and they
are copied with their addresses. Everything else is a general system fed
by the game's data:
- the event player plays any event;
- the quest runtime runs any quest script;
- the dragon fights through the enemy runtime and its think table.

When a step needs a system, build the system, not the step.

## The route

The order comes from QUESTS.md "Worked example: the opening" (traced),
extended past q0002.

| Step | Original | Status | Brief |
|---|---|---|---|
| 1 | New game: stage 380, SceNo 100 | traced | opening-prologue |
| 2 | Prologue, stage 380: the playable hero, fights (harpies among them), SceNo 200 → 900, event st380 ev00 | **for later** (maintainer, 2026-10-10): important, not urgent; it branches into more systems. The opening runs it as a scripted stand-in until then. Party setup, encounter facts and the chimera's think init are merged | opening-prologue |
| 3 | Prologue end → stage 800, q0000 boot, the movie `movie\event\st100ev00.wmv` (the beach and the dragon) | **plays** at q0000's awaited PlayEvent (S16), skip = Escape / pad Start (S08, 0x004367C0); subtitles open (S19) | opening-movie |
| 4 | Stage 801: character creation | data readers (chara-edit), no UI | chara-creation-ui |
| 5 | Stage 200: st200 ev00 "It's the dragon!", the scripted fight "Challenge the Dragon", ev01 (heart taken), ev05 | **required for the current goal**; the dragon is a host actor with its authored entrance (S29, merged 2026-10-10); landing/PressAttack and BreathWide (S30) and the ev00 → fight → ev01 → ev05 hookup (S31) in progress; LAN guests see it after S33 | dragon-attack, S29-S31, S33 |
| 6 | q0001 "Newly Arisen": weapon table, vocation, ev10, Quina | **playable**; the menu stub offers Fighter / Strider / Mage and the Arisen gets that vocation's fight kit and starting weapons (Host::apply_job_kit, 573e421, 50deb30); the new-game bag and quick use (S28); the Strider's bow draws and shoots arrows headless (S25, S26), viewer next (S27); the original vocation GUI replaces the stub in S35 | done; S27, S35 |
| 7 | ev15, q0002 "Upon a Pawn": the first pawn, the goblins harassing a woman | **plays to the rescue's outcome**: NPC1's Guard boots q0002 (S23); Rook joins; the field goblins spawn and bind to EnemyCheck; killing group 23 sets q2:20 through the authored event1 (head demo `DDVIEW_DEMO_FIGHT`, 2026-10-10). Open: Reynard's motions/retention and his QCT talk (S32), his HP and gauge (S34), pawn commands (S36), experience (S37) | upon-a-pawn, q0002-roadside, S32, S34, S36, S37 |
| 8 | q0003: the road out of Cassardis to the Encampment (st210 ev00 at SceNo 2000) | q0002_event2 (q2:10, boots q0003) lies up the road; the demo's RoadWalk heads there (planner extended, unverified). The cyclops and the first Rift contact belong to step 9 | road-to-encampment |
| 9 | Arrival at the Encampment, where the hydra's head is later brought | **waits** for the maintainer's playtest of steps 1-8 (road and dragon fight first); includes the cyclops (cyclops-runtime, S18) and the first Rift contact | road-to-encampment |

**Current goal (maintainer, 2026-10-10):** the tutorial through the
road to the Encampment: steps 3-8 playable, the dragon attack (5)
included. The prologue (2) is for later; the Encampment (9) waits for the
maintainer's playtest of everything before it.

Steps 7–9 have not been traced yet: who the first pawn is, where the
goblin scene happens, and which stage is the Encampment. Each brief
starts by tracing its quest's scripts and QCT, like q0000–q0002.

## Systems the slice needs

| System | For steps | State |
|---|---|---|
| Movement feel: turn rate, locomotion blending, air attacks, falls | all | done (movement-feel) |
| Enemy runtime: goblins, harpies, wolves, the dragon, prologue enemies | 2, 5, 7, 8 | goblins done; wolves and harpies in progress (enemy-families) |
| Boss and scripted-fight support (fight checks, `EnemyCheck`, `mCntState`) | 5 | traced in part |
| Event player: faces, audio, the brow and ear defects | 2, 5–9 | playable; visual defects open (cutscene-visuals) |
| Movie playback (WMV) | 3 | not started |
| Character creation UI (`uGUIEdit`), applied to the body and stats | 4 | data only |
| Pawns: first pawn meeting, recruitment, the party | 7–9 | data, AI and knowledge; no recruitment flow |
| Quest runtime and log | all | q0000–q0002 headless |
| HUD | all | merged (game-hud) |
| Skill and hit VFX (`mteffect`) | 2, 5, 7 | readers only (skill-vfx) |
| Sound and music | all | event streams, voices, motion sounds and random selection merged; mixer and BGM open |
| Field streaming between cells | 8 | built (`ddworld::stream`, `ddview` stream.rs); the opening hookup is codex-26's |
| Co-op: guest dialogue and menus, event freeze | all | host-driven only |

## Acceptance

Each step's gate is the original's state after it:
- SceNo, GSF, quest words and the active stage, compared with a save or
  oracle read taken at the same point in the original;
- a capture comparison for its look.

The whole slice passes when two players on a LAN play steps 1–9 without
fast-forwards or teleports, and the end state matches the original's.

## Steps 8-9: what the data says (head, 2026-10-09)

**Stages connect through door scenarios** (`scr\stSSS\etc\stSSS_door`,
`cScenarioArg_AreaJump` `mStageNo` / `mPosNo`; 0x00799480 jumps when the
Arisen is inside the volume; `ddstory::scenario` already emits
`Request::JumpArea`):
- Cassardis `st200_door` record 0 leads to stage 100, start position 2:
  the gate to the field. Records 1-5 lead to 700 and 610.
- The field's `st100_door` leads to 210 (positions 0 and 1), 200, 300 (both
  positions), 220, 310, 320, 700, 702 and 6.
- The Encampment's `st210_door` leads to stage 100, positions 1 and 3, and
  within 210 to positions 3 and 14.

So the walk is: Cassardis gate → field position 2 → field door into
210 → q0003. `stream::Streamer::enter` does the handoff (test
`the_cassardis_gate_arrives_on_field_ground`). The opening's host must
evaluate the current stage's door scenarios (stage 100's too) to emit
them.

**q0003 happens at the Encampment** (`quest\q0003_b00`; Japanese state
names in brackets). In order:
- the encampment check (宿営地チェック): AreaHit 3/4;
- the mysterious voice (謎の声): a message;
- the simple event that shows the Rift (リムを見せるシンプルイベント:
  camera 8, timers, SE);
- leave the tent (テントの外に出る): AreaHit 5/9/10 and SceNo;
- the NPC simple event (camera 9, layouts 210:68 and 63, NPC group 18,
  battle BGM, quest marker bit 17);
- the encampment clean-up (layouts 210:100-115 off);
- the road block (道ふさぎ: AreaHit 0/1/3);
- listening to the Rift (bit 20, camera 21, BGM control);
- the st210 checks.

"リム" is the Riftstone, so the first Rift contact is in q0003. To trace
next: its QCT, the AreaHit volumes, and how q0002 hands over to q0003.

