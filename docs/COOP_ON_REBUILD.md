# Co-op on the 1:1 rebuild: where the original assumes one player

The rebuild runs the original's own code (lifted 1:1). Two-player co-op
([COOP_DESIGN.md](COOP_DESIGN.md)) therefore has to be added as small,
deliberate changes on top of verified functions. This document maps
where the original assumes a single player, proposes the smallest
change for each, splits the work between host and client, and lays out
a staged plan. Brief: [L40](tasks/L40-coop-hook-map.md).

**Labels.**
- **[exe]**: read in the decompile of DDDA.exe (2017-12-13 Steam).
- **[scan]**: a static count over the lifted sources (method in 1.1).
- **[doc]**: taken from the topic document named.
- ***Proposal*** and ***inferred*** mark everything that is not a
  reading of the exe.

## Summary

- "The player" is one pointer: **`sPlayerManager+0x99C`**
  (`sPlayerManager` = `*0x018FA4EC`). 284 lifted functions read it
  directly; enemies, AI, the HUD and the camera all do.
- The party is **four pointers** `+0x99C..+0x9A8`: the Arisen and three
  `uCmc` pawns. Only 7 functions name the last slot (`+0x9A8`), and
  COOP_DESIGN already gives that slot to P2. So P2 fits into the
  original's party array.
- **The player's input is already a per-unit path.** `uPlayerBase::vf200`
  fills the unit's own command fields; pawns fill the same fields from
  their AI's virtual pad. A remote player is a third source of the same
  fields. It needs one wrapped function.
- **Camera, HUD, menus and the save's Arisen record are single.** On the
  client they serve the client's own Arisen if every machine sees its
  own Arisen in slot 0. That is the main proposal: a seat-relative
  slot 0, with P2 in slot 3 on the host and the host's Arisen in slot 3
  on the client.
- **The original is not deterministic across machines.** Units run as
  jobs on worker threads, one global `MtRandom` is shared, and the step
  comes from QPC. Co-op must be host-authoritative with snapshots, not
  lockstep.
- **Stage 1:** P2 as a `uPlayer` in slot 3, a remote pad in `vf200`,
  and a snapshot of the two Arisen at the frame's network step (step 54).
  That is about 10 cold functions changed plus new transport code.

## 1. The player and the party: `sPlayerManager`

### 1.1 Method of the counts [scan]

`scan_global.py` walks the lifted sources (`crates/ddlift/*`). For each
function, it finds every load of the global's pointer into a register
and records the offsets that register is then read or written at within
the same lifted block. "ptr" means the pointer was loaded and only
passed on or compared.

Each function is attributed to a system:
- by its own DTI label (a virtual slot of a class);
- else by the classes of its nearest labelled callers, up to 4 call
  levels (majority system).

Limits:
- Pointer arithmetic (a base `+0x9A0` stepped by 4) counts once, at the
  first offset.
- A use in a later block (after a branch) is not seen. So the use
  classes of 1.4 are lower bounds.
- Functions not yet lifted are absent.

The scripts and per-function lists are private scratch (not committed).

### 1.2 Structure [exe]

`sPlayerManager` is a `sUnitSearchManager` (ctor `0x0047DF20`, vtable
`0x0155F8B8`). It holds the party and the shared player parameters:

| Field | Content | Evidence |
|---|---|---|
| `+0x868`, `+0x86C..+0x8CC` | shared player parameters: `rPlExp`, `rPlLevel` and `rPlJobLevel` per vocation, 9 `rAIPlayerActionParameter`, `PlDefendParam`, `ShakeDamage`, `PlNeckPos`, `PlAbilityParam`, `mag_item_act_param` | loaded by `0x0047E2B0` (from `0x004FB3E0`) |
| **`+0x99C`** | **the Arisen** (`uPlayer`, vtable `0x015E90D0`) | ORACLE.md; written by `0x00501020` |
| `+0x9A0`, `+0x9A4`, `+0x9A8` | pawn slots 0-2 (`uCmc`) | `0x00501020`, `0x0043D2B0` |

The accessors and loops:

| Function | What |
|---|---|
| `0x0047EC30` | party member by index (in `eax`): 0 → `+0x99C`, 1-3 → `+0x9A0 + 4(i-1)`, else 0. 13 callers: the stage event code (`0x0050BE20`, `0x0050C7A0`, `0x0050DCD0`, `0x00511770`, `0x00512A00`), `uEm5200`, `uNpc`, `uOmObj8505` and others |
| `0x0047EBA0` | clears every slot whose unit state (`unit+4 & 7`) is neither 1 nor 2 (the unit is gone) |
| `0x0047EB50` | deletes all four (from `vf05`, `vf10`) |
| `vf19` `0x0047EA40`, `vf20` `0x0047EA90` | call slot `+0x2E8` / `+0x2EC` on each of the four |
| `0x0047EAE0` | creates a pawn in slot `edi` (unit creator `0x00421FE0`) and stores the index at `uCmc+0x5854` |
| `0x0047EB30` | deletes pawn slot `edi` |

### 1.3 Who creates the party [exe]

**The stage set-up `0x00501020`** (19 callers, the stage modes) creates
the party when a stage starts (except stages 0x320-0x324):
1. The Arisen: unit creator `0x00421FE0` → `sPlayerManager+0x99C`,
   placed at the stage's entry (`param+0x780..`), or higher ground when
   `0x007B0580` finds it.
2. Then, for `i` = 0..2, when `0x0043E300` allows it (not on stages
   0xF0, 0x1F5, 0x1F6, 0x263, 0x267, 0x320-0x324 and some others; and
   the record's byte `sGameSys+0xA8216 + 0x1660i` is clear):
   - a unit into `+0x9A0 + 4i`;
   - **`uCmc+0x5854 = i`** (the pawn index);
   - **`uCmc+0x585C` = the Arisen pointer** (the pawn's master);
   - placed beside the Arisen, at (x + i, z + 2 − i).

**The party upkeep `0x0043D2B0`** runs every frame from
`sGameSys::vf06` (step 18). It spawns and removes pawns from per-slot
requests (*inferred:* hire, dismiss, return). Its inputs:
- the request block at `sGameSys+0xAC330 + 0x30i`;
- the pawn record at `sGameSys+0xA7EC0 + 0x1660i`.

It also sets `+0x5854` and `+0x585C` the same way.

*Inferred:* slot `+0x9A0` is the main pawn. The save's `mCmc[0]` is the
main pawn (PAWNS.md), and the records are indexed by the same `i`.

### 1.4 Who reads it [scan]

606 lifted functions load `sPlayerManager`. Fields per function:

| Field | Functions |
|---|---:|
| pointer only | 379 |
| `+0x99C` (the Arisen) | 284 |
| `+0x9A0` | 40 |
| `+0x9A4` | 11 |
| `+0x9A8` | 7 |
| `+0x8C8`, `+0x8CC`, `+0x868` (parameters) | 3, 2, 2 |

By system:

| System | Functions | `+0x99C` | pawn slots |
|---|---:|---:|---:|
| enemies | 138 | 72 | 15 |
| unattributed (no labelled caller within 4 levels) | 128 | 62 | 3 |
| GUI / HUD | 72 | 31 | 4 |
| player (`uPlayerBase`, `cPlAct*`) | 62 | 22 | 7 |
| AI (`sAI*`, `cAI*`) | 43 | 20 | 9 |
| NPCs and humans | 30 | 18 | 2 |
| other classes | 29 | 13 | 1 |
| objects (`uOm*`) | 23 | 4 | 1 |
| pawns (`uCmc`, `sPawnManager`) | 17 | 11 | 0 |
| camera | 17 | 13 | 0 |
| other managers | 14 | 5 | 0 |
| stage, sky, weather | 12 | 1 | 0 |
| shells | 8 | 6 | 1 |
| sound | 7 | 4 | 0 |
| events, quests, scripts | 3 | 1 | 0 |
| effects, items | 3 | 1 | 0 |

**How the Arisen pointer is used** (463 load sites in lifted blocks,
strongest use per function; lower bounds, see 1.1):

| Use | Functions | Meaning for co-op |
|---|---:|---|
| identity: compared with another pointer | ≥ 38 | "is this unit the player?" |
| dereferenced (position `+0xE0`, fields) | ≥ 44 | distance to, or state of, the Arisen |
| passed on | ≥ 47 | a target or an argument |
| null test only in the block | 183 | mostly followed by a dereference in the next block |
| not classified | 151 | |

Typical sites [exe]:
- `0x00853DC0` (`uEnemy`): the update rate by distance to the Arisen
  (`+0x298C` against `+0x2984`).
- `0x008F06F0` (`uEmWolfBase`): the Arisen as the default target
  (`+0x2B98`) and its position.
- `0x0093B170`: an action only when an Arisen exists.

The 7 functions that name `+0x9A8` all walk the whole party: dragon,
wight, zombie, `uEm7000`/`7001`, `uEm5400`.

## 2. The other single-player assumptions

### 2.1 Input [exe]

| Object | Global | Readers [scan] |
|---|---|---|
| `sQlocInput` (game buttons) | `0x018D8BB0` (written in `0x004083A0`) | 131 functions (GUI 47) |
| `sKeyboard` | `0x018D0FAC` (ctor `0x00DF43E0`) | 22 |
| `sMouse` | `0x018D103C` (written in `0x00E05120`) | 104 (GUI 85) |

- `sQlocInput` holds four pad blocks (stride 0x400 from `+0x48`;
  analog values at `+0x17B0 + 0x2E8·pad`).
- **The Arisen merges every local device** (`uPlayerBase::vf200`
  `0x00B6AEA0`; slot 200 inside phase 90 of pass A, FRAME.md):
  1. Eight analog values, ids 0xD5..0xDC, are read through the object at
     `uPlayerBase+0x3348` (vf `+0x3C`) into `+0x331C..+0x3338`.
  2. `0x00B51EE0` takes the stick from the first of the four pads past
     its dead zone, or from the keyboard keys listed at
     `sQlocInput+0x1400`. It rotates the stick by **the global camera**
     (`sCamera+0x34`: eye `+0x40`, target `+0x60`).
  3. `0x00B52880` maps the buttons to commands (the key table
     `*0x018D9264`).
- **Pawns use the same fields.** `uCmc::vf200` `0x00B537D0` copies its
  AI's virtual pad (`cAICtrl+0x40`) into them (FRAME.md). So the
  per-unit command fields are the natural injection point for a remote
  player.

*Not found:* who sets `uPlayerBase+0x3348`. The ctor `0x00B574D0` zeroes
it as part of a `cAdjustParam` at `+0x3344`.

### 2.2 Camera [exe]

`sCamera` is `*0x018D0B6C`; `sCameraExt::vf06` runs at step 51
(FRAME.md). Fields:
- `+0x34`: the current camera;
- `+0xCE0`: the player camera's controller (CAMERA.md);
- `+0xD09`: a byte read by 53 GUI functions.

380 lifted functions load it [scan]:
- GUI 83;
- enemies 78. Mostly the camera-work request `0x004219A0(sCamera, n,
  unit, unit)`, 59 callers, e.g. a grab such as
  `cEm7000ActRimSummonCaughtPl`;
- player 59 (`+0xCE0` 18);
- camera classes 29.

The player camera follows the Arisen through `sPlayerManager+0x99C`
(13 camera functions read it).

### 2.3 GUI and HUD [doc, scan]

- The HUD units run on lines 26/27 and the menus on 28-32 (FRAME.md).
  `sIdMessage` and `sIdCockpit` run at step 45.
- The GUI reads the Arisen two ways:
  - through `sPlayerManager+0x99C` (31 functions);
  - **directly in the save record `sGameSys+0xA76D0`**: 96 of the 175
    functions that read that record by fixed offset.
- It reads input through `sQlocInput` and `sMouse` (132 functions).

The GUI is entirely "the one local player".

### 2.4 The Arisen's data and the save [exe, doc]

`sGameSys` (`*0x018FA4BC`) holds the party's records:
- **the Arisen's** at `+0xA76D0`: level `+0x700`, name `+0x70C`
  (ORACLE.md). 0x7F0 bytes up to the first pawn record, size
  *inferred*.
- **the three pawns'** at `+0xA7EC0 + 0x1660i`.

Each character unit points to its record at `uPlayerBase+0x3DEC`
(ORACLE.md).

Readers by fixed offset [scan]:
- 175 functions read the Arisen record directly (GUI 96,
  unattributed 60, NPCs 6, enemies 3, sound 2, events 1, objects 1);
- 31 read the pawn records.

The save file mirrors this (PAWNS.md, CHARACTERS.md): `mPlayerDataManual`
holds `mPl` (one Arisen: `mEdit` and `mParam`), `mCmc[3]` and the party
positions (`mPlPos`, `mCmcPos`). `sSave` (`*0x018FA4F8`) is read by 218
functions [scan].

### 2.5 Events and cutscenes [doc, exe]

- `sEventManager` (`*0x018FA4B8`) plays one event at a time
  (EVENTS.md).
- Actor `mUnitType` 0 binds **the player**; it copies the player's look
  (`uActorModelPl::vf05` `0x0083E010`). Type 1 binds pawn slot
  `mUnitNo`, through `0x0047EC30` [exe].
- Events and menus freeze gameplay by clearing group bit 57
  (`0x0043F6C0`, 21 callers; the mask writer `0x00DB5A60`, 12 callers;
  FRAME.md).
- Rows are refused while the player is busy (`0x00516E00`).
- Skip: any of the four pads (`0x004367C0`).

### 2.6 Pawn AI and the Arisen [exe, doc]

- **Follow** (`0x00585D00`, Wait_Follow-01, PAWN_AI.md) reads
  `sPlayerManager+0x99C` directly, not the pawn's master `+0x585C`.
- **Orders**: `0x00B768E0` (from `uPlayerBase` `0x00B6D510`) sends 9
  Up, 10 Down and 11 Left/Right to every pawn slot without an owner
  check (PAWNS.md). They are delivered in pass C (`0x00887360`).
- `sAICharacterInfo` (`0x00411030..0x00411F10`) and `sAIMessageCtrl`,
  `sAIStudyCtrl` read slot 0 and `+0x9A0`. They are the AI boards'
  view of the party: 43 AI functions read `sPlayerManager`, 20 of them
  slot 0.
- `sPawnManager` (`*0x018FA4E8`): 23 readers, none of them
  player-specific in the scan.

### 2.7 Other world-wide singletons

The following are world state. The host owns them, and the client
receives what it shows:
- `sGameSys` flags and clocks;
- `sArea` (the area stack, `*0x018D099C`);
- `sWeatherManager`;
- `sQuestManager`, `sScenario`.

The shared player parameters (`sPlayerManager+0x868..+0x8CC`) are
read-only, so they stay single.

## 3. Change proposals per assumption (*Proposal*)

All changes use the runtime's existing mechanism: a binding per address
(RUNTIME.md). A changed function is a readable adapter, bound in place
of (or around) the lifted body. It carries a source address and a test.
**With co-op off, every adapter must reduce to the original call, and the
one-seat comparison gate stays bit-identical.**

The central decision, **each machine sees its own Arisen in slot 0**:
- **Host:** slot 0 is P1 and **P2 is a `uPlayer` in slot 3** (`+0x9A8`,
  the second hired-pawn slot COOP_DESIGN gives P2).
- **Client:** slot 0 is the client's own Arisen, and the host's Arisen
  is in slot 3.

So the camera, HUD, menus, input and the save record work unchanged on
each machine for its local player. That covers the readers of 2.1-2.4.
Only the host's *simulation* needs a seat-aware view of slot 0.

| # | Assumption | Change | Functions | Hot/cold |
|---|---|---|---:|---|
| 1 | Party creation (`0x00501020`, `0x0043D2B0`) makes pawns in slots 1-3 | **Parameterise by seat:** an adapter after `0x00501020` creates P2's `uPlayer` in slot 3 (on the client: the host's Arisen); both pawn loops skip slot 3 in co-op | 2 + 1 new | cold (stage start, hire) |
| 2 | Slot 3 is assumed to be a `uCmc` (`+0x5854`, `+0x585C`, pawn vfuncs) | **Audit:** the pawn-slot readers and the accessor's callers; guard the pawn-only ones by vtable (e.g. the order broadcast `0x00B768E0`) | 41 + 13 to review, ~10 to change *(estimate)* | mostly cold; the 7 enemy party walks are hot but use `uCharacterBase` fields only *(to check)* |
| 3 | P2's data (`uPlayerBase+0x3DEC` → `sGameSys+0xA76D0`) | **Second instance:** a P2 record in host guest memory (the client's own Arisen record, sent at join), pointed to by P2's `+0x3DEC` | 1 new | cold |
| 4 | Input: the Arisen reads every local device and the global camera (`0x00B6AEA0`, `0x00B51EE0`) | **Host-side wrapper:** for a remote seat, `vf200` fills `+0x331C..+0x3338` and the pad state at `+0x32B4` from that seat's network pad, like `uCmc::vf200` does from the AI pad. The stick is rotated by the client's camera (sent with the pad) | 1 wrapped (`0x00B6AEA0`), 1 parameterised (`0x00B51EE0`) | hot (per Arisen per frame), cheap |
| 5 | Simulation reads of slot 0 (enemies, AI, NPCs, objects, shells: about 240 of the 284, outside the GUI and camera) | **Seat-relative slot 0.** The lifter rewrites the static loads of `[0x018FA4EC]+0x99C` into a runtime call that returns: the unit itself, if the current unit is an Arisen; the owner's Arisen, for a pawn (`+0x585C`); else the nearest living Arisen to the current unit. The current unit is a thread-local set where passes A/B/C call a unit (`0x00DB7350`, `0x00DB6890`, `0x00DB67B0`, jobs `0x00DBE560`). Outside a unit (managers, GUI): the local seat | 284 rewritten by rule; 3-4 pass walkers wrapped | hot; one TLS read and two distances per load |
| 6 | Identity tests ("is this unit the player?") | **Review per site:** replace `x == slot0` with "x is an Arisen" where the site means any player (damage reactions, pickups, talk) | ≥ 38 *(lower bound)* | mixed |
| 7 | Camera-work requests from the simulation (`0x004219A0`) | **Host-side wrapper:** a request whose target is a remote seat's Arisen is sent to that client as a camera event | 1 wrapped | cold (grabs, throws) |
| 8 | Pawn orders (`0x00B768E0`) | **Parameterise by seat**, per COOP_DESIGN: the shared main pawn obeys both (ascending seat order), a hired pawn its owner | 1 | cold (on a button) |
| 9 | Pawn follow (`0x00585D00`) | Covered by #5: a pawn's slot 0 is its owner's Arisen | 0 extra | hot |
| 10 | Events (`mUnitType` 0, `0x0083E010`; freeze `0x0043F6C0`) | **Leave single** on the host: the event runs once and freezes both. P2 is hidden, or bound where the event asks for pawn slot 3 (it is P2's slot). The client plays the event from the host's state | 1-2 | cold |
| 11 | Menus freeze the world (bit 57) | **Leave single, but local:** P2's menus run on the client and freeze only the client's replica. Pause on the host stays the host's (COOP_DESIGN: personal menus freeze nothing; the host's own pause needs a rule) | 21 callers to classify | cold |
| 12 | Inventory and the Arisen record read by fixed offset (175 functions, GUI 96) | **Leave as is on each machine** (the local seat's record). The host-side simulation readers (19 attributed outside the GUI, plus the 60 unattributed) are audited for P2's actions: use item, equip | 20-80 to review | cold |
| 13 | The save (`mPl`, `mCmc[3]`) | **Host-side wrapper** at save time: P2's record and edit go to the client's save as its chunk (ADR 0002). The world chunks stay the host's | 2-3 | cold |
| 14 | Online pawn sharing (`sNetworkExt`, `sDDCaProto`, Steam) | **Leave single**: host only. LAN co-op uses its own transport | 0 | — |

**Totals *(estimate)*:**
- about 25 hand-written adapters and wrappers, nearly all cold;
- one lifter rule (#5) over 284 functions;
- a review of about 100-200 sites (#2, #6, #12).

Nothing on the render path changes.

## 4. Host and client on the 1:1 runtime

### 4.1 Determinism constraints [exe, doc]

- **Jobs.** Unit lines run as jobs on `sMain`'s worker threads (line
  flags 0x1FD live; FRAME.md 3). Within a line, the order of side
  effects depends on thread timing.
- **One random generator.** `MtRandom` is the global `0x018D017C`
  (xorshift128, MATH.md), and every system draws from it. Its sequence
  depends on the job interleaving.
- **Timing.** `mDeltaTime` comes from QPC (`sMain::vf06` `0x00DBFA90`).
  Several systems tie behaviour to real time (FRAME_RATE.md: input
  repeat times, `sRecognition` intervals).

**Conclusion (*Proposal*):** no lockstep and no client-side
re-simulation of the world. The host simulates, the clients replicate
its state. The runtime may pin the host's step to 1/30 s
(COOP_DESIGN), but the result is still not reproducible on another
machine.

### 4.2 Sync points in the frame (FRAME.md order)

| Where | Host | Client |
|---|---|---|
| step 23 (`sQlocInput::vf06`), before the units | apply each remote seat's latest pad (read later by `vf200` in pass A) | read the local devices as usual; send the pad, the button events and the camera (eye, target) to the host |
| pass A/B/C (steps 36-42) | simulate everything, P2 included | run the local Arisen's prediction (stage 2); replicas do not move themselves |
| **step 54** (`sNetworkExt::vf06`, the original's own network step: after the cameras and sound, before rendering) | capture the snapshot and send it | apply the latest snapshot to the replicas, before rendering (steps 56-58) |
| events, area changes | send as reliable messages (event row, area request `sArea::vf10` `0x00DB4C10`) | apply at the next step 20 (`sAreaExt::vf06`) |

### 4.3 What the client runs locally (*Proposal*)

The client needs its own guest world in any case: the original renderer
draws from units, not from pictures. Its units are driven from the
host's state:
- **local:**
  - the camera (step 51, slot 0 = its own Arisen);
  - the HUD and the menus;
  - sound mixing (fed by replicated cues);
  - effects (spawned from replicated events);
  - the local Arisen's input and, from stage 2, its prediction.
- **replicated, per character** (`uCharacterBase`):
  - position `+0xE0` and rotation;
  - the current action;
  - the motion layers (motion id, frame, interpolation; ANIMATION.md
    layer fields);
  - HP, stamina and status.
- **replicated, world:**
  - the area and stage;
  - the game clock and the weather;
  - quest and scenario flags;
  - spawns and despawns, with a unit id map.

**Open, to measure:** whether a replica's pose updates when its own
move is suppressed (by its Move flag 0x400, per unit, not by the group
mask, so that the client's camera and HUD keep running). If the pose
is evaluated inside the move, the replica runs a reduced move instead.

### 4.4 RUNTIME.md's caution

RUNTIME.md says that a global-player swap or two independent guest
worlds is "not an established solution". This proposal is a seat-
relative slot 0 inside one authoritative world (the host's), plus
**replica** worlds on the clients, which make no decisions. Stage 1
tests exactly that before anything else depends on it.

## 5. Staged plan (*Proposal*)

**Stage 0: groundwork, no behaviour change.**
- A seat table in the runtime: seat → Arisen unit, profile, transport.
- A thread-local "current unit" at the three pass walkers and the job
  runner.
- Lifter support for rewriting a statically identified load into a
  runtime call (off: the same code as today).
- A LAN transport crate with pad, snapshot and reliable-event messages.
- Gate: the one-seat comparison unchanged.

**Stage 1: two Arisen in one area; the client sees and moves its own
character.**
- Host:
  - #1: P2's `uPlayer` in slot 3; the pawn loops skip slot 3;
  - #3: P2's record from the client's Arisen record;
  - #4: the remote pad in `vf200`;
  - the snapshot of both Arisen at step 54.
- Client:
  - joins the host's area (stage index → stage loader `0x004FFBA0`);
  - the host's Arisen as a replica in slot 3;
  - its own Arisen in slot 0, driven by the snapshot (no prediction
    yet; LAN latency);
  - its own camera and HUD.
- Test area: a calm stage, with enemies and pawns not spawned on either
  side for this stage.
- Acceptance: each player walks, runs and jumps, and the other sees it
  within two frames. The one-seat gate is unchanged with co-op off.
- About 6 adapters and wrappers, all cold except `vf200`.

**Stage 2: the shared world.**
- All characters replicated; pawns and enemies run on the host only.
- The seat-relative slot 0 on (#5); the identity sites reviewed (#6).
- Camera-work events (#7), effect and sound cues, damage and death on
  P2.
- Prediction of the local Arisen, with correction.

**Stage 3: interaction.**
- Pawn orders by seat (#8); P2's hired pawn.
- The NPC interaction lock (COOP_DESIGN).
- Personal menus (#11); items and equipment for P2 (#12).

**Stage 4: events and saves.**
- Events run on the host, P2 hidden or placed (#10); skip by any seat.
- Shared game over (COOP_DESIGN).
- P2's chunks written to the client's save (#13).

## 6. Not established

- The setter of `uPlayerBase+0x3348` (the input object).
- Whether pawn AI reads its master `+0x585C` anywhere. Follow reads slot
  0 directly.
- The size and full layout of the Arisen record at `sGameSys+0xA76D0`.
- Whether a replica's pose updates with its move suppressed (4.3).
- The 128 unattributed `sPlayerManager` readers and the 151
  unclassified slot-0 uses: both need the per-site review of stage 2.
- Whether `uPlayer` is at least as large as `uCmc`. That matters for
  the reads of `+0x5854` / `+0x585C` through slot 3 (#2).
