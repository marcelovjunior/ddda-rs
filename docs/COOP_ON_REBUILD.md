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

**How the Arisen pointer is used** (superseded by the decompile-based classes of 6.7; 463 load sites in lifted blocks,
strongest use per function; lower bounds, see 1.1):

| Use | Functions | Meaning for co-op |
|---|---:|---|
| identity: compared with another pointer | ≥ 38 (**corrected in 6.7: 32**) | "is this unit the player?" |
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
  1. Eight tuning values, ids 0xD5..0xDC, are copied from the shared
     parameter table `m_pl000_00` (`uPlayerBase+0x3348`, vf `+0x3C`)
     into `+0x331C..+0x3338`. These are parameters, not input (6.1).
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

The pad itself (the object at `uPlayerBase+0x32B4`) and how a remote
player drives it: 7.1.

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
- **the Arisen's** at `+0xA76D0`: a `cPlayerInfo`, 0x7F0 bytes
  (6.2);
- **the three pawns'** at `+0xA7EC0 + 0x1660i`: `cCmcInfo`, which
  derives from `cPlayerInfo`.

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
| 3 | P2's data (`uPlayerBase+0x3DEC` → `sGameSys+0xA76D0`) | **Second instance:** a P2 `cPlayerInfo` (0x7F0 bytes) in host guest memory, filled from the client's own record at join. A wrapper on the record binder `0x00B5A820` points a remote seat's unit at it (6.2) | 1 wrapped + 1 new | cold |
| 4 | Input: the Arisen reads every local device and the global camera (`0x00B6AEA0`, `0x00B51EE0`) | **Host-side wrapper:** a remote seat's Arisen uses the pad's virtual path, the one pawns use: its pad flag `+0x60` is cleared, and its 36 input bytes are written from the network before `vf200`. The client's own pad code has already rotated the stick by the client's camera (7.1) | 1 wrapped (`0x00B6AEA0`); `0x00B51EE0` and `0x00B52880` unchanged | hot (per Arisen per frame), cheap |
| 5 | Simulation reads of slot 0 (enemies, AI, NPCs, objects, shells: about 240 of the 284, outside the GUI and camera) | **Seat-relative slot 0.** The lifter rewrites the static loads of `[0x018FA4EC]+0x99C` into a runtime call that returns: the unit itself, if the current unit is an Arisen; the owner's Arisen, for a pawn (`+0x585C`); else the nearest living Arisen to the current unit. The current unit is a thread-local set where passes A/B/C call a unit (`0x00DB7350`, `0x00DB6890`, `0x00DB67B0`, jobs `0x00DBE560`). Outside a unit (managers, GUI): the local seat | 284 rewritten by rule; 3-4 pass walkers wrapped | hot; one TLS read and two distances per load |
| 6 | Identity tests ("is this unit the player?") | **Review per site:** replace `x == slot0` with "x is an Arisen" where the site means any player (damage reactions, pickups, talk) | 32 (6.7) | mixed |
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

**Settled in L40b (7.3):** the pose is evaluated inside the move
(phases 92 and 93 of slot 8). A replica therefore runs a reduced move:
the motion advance and the commit, with the host's state, and no AI,
actions, integration or hit volumes.

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
- 7 guest functions wrapped plus two runtime hooks and the
  transport; the list and the message formats are in 7.6 and 7.5.

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

## 6. The open items of L40, closed (L40b, L40c)

### 6.1 `uPlayerBase+0x3348` is a parameter table, not input [exe]

`+0x3344` is an array of ten parameter slots, 8 bytes each
(`cAdjustParam`-style: a vtable and an `rAdjustParam` pointer). They are
set by `0x00B5B950`, called from the unit's init (`uPlayerBase::vf05`
`0x00B58F50`). It copies the ten `rAdjustParam` pointers that
`sPlayerManager`'s loader `0x0047E2B0` keeps at `sPlayerManager+0x970..
+0x994`, with reference counts (`0x00DE0B80` / `0x00DE0B90`):
- `param\pl\other\m_pl000_00`, the general player table, into
  `+0x3348`;
- the nine per-weapon tables (`m_pl000_Sword`, `_Shield`, `_GSword`,
  `_Unarm`, `_Dagger`, `_Bow`, `_Wand`, `_BowL`, `_ShieldMG`; string
  table `0x0163E660`) into `+0x3350..`.

`0x00B5B9A0` maps a weapon category to a slot. vf `+0x3C(id)` returns
the float with that id. 257 lifted functions read `+0x3348` (e.g. id
0xB9 in `0x005AA900`, 0xD3 in `cEm0600ActPlayerMove::vf08`).

For co-op the tables are shared and read-only, so P2 binds the same
ones through the same init.

### 6.2 The records and the sizes [exe, DTI]

**How a character finds its record.** `0x00B5A820` sets
`uPlayerBase+0x3DEC` from the character's kind byte `+0x206D`:

| `+0x206D` bit | Record |
|---|---|
| 0x2 (the Arisen) | `sGameSys+0xA76D0` |
| 0x4 (a party pawn) | `sGameSys+0xA7EC0 + 0x1660 × uCmc+0x5854` |
| 0x8 | the unit's own `+0x5870` |

It also clears the record's `+0x354`. Its 6 callers:
- the party code: `0x00501020`, `0x0043D2B0`, `0x0047EAE0`;
- the uPlayer ctor `0x00B55900`, `uPlayerBase::vf05` `0x00B58F50`, and
  `0x00BABB00` (in the `uNpc` code, beside its ctor `0x00BAB3A0`).

**The record types** (MtDTI reflection, `dti-props`):
- **`cPlayerInfo`** (parent `cBaseInfo`), **0x7F0 bytes**. That is
  exactly the gap from `+0xA76D0` to the first pawn record. Fields:
  - look and equipment ids (`mJob +0x10`, weapons, armour, colours);
  - skills;
  - `mHpMax +0x2A0`, `mHpMaxWhite +0x2A4`, `mStamina +0x2A8`;
  - `mExp +0x2C4`, `mGold +0x348`, `OnlinePoint +0x34C`,
    `mIsCmcLost +0x354`;
  - conditions (`mCStatus*` from `+0x35C`).

  `mHp` is reflected by getter only. *Inferred* at `+0x29C`:
  `0x0043D2B0` reads that float for a pawn.
- **`cCmcInfo`** derives from `cPlayerInfo`, **0x1660 bytes**. It adds:
  - `mStatus +0x7F0`, `mPawnType +0x7F4`;
  - the knowledge `mStudyFlag +0x7F8` / `mLocalStudyFlag +0xFB4`;
  - `mInfo[]` inclinations from `+0x14BC`.

**The vitals live in the record, not in the unit.** P2's HP and stamina
are therefore in P2's `cPlayerInfo` (proposal #3).

**Unit sizes** (DTI registry, the allocation in each `newInstance`):

| Class | Size | Parent |
|---|---:|---|
| `uCharacterBase` | 0x2F90 | `uObjModel` (0x2530) |
| `uPlayerBase` | 0x5850 | `uCharacterBase` |
| `uCmc` | 0x58E0 | `uPlayerBase` (adds `mCmcId`, **`mpPlayer +0x585C`**, `mTargetPos +0x5860`) |
| `uPlayer` | 0x5A10 | `uPlayerBase` |
| `uNpc` | 0x7170 | `uPlayerBase` |

So a `uPlayer` is larger than a `uCmc`. Reading the pawn fields
`+0x5854..+0x58DF` through slot 3 stays inside P2's object but reads
`uPlayer`'s own fields there. The audit of #2 stands: it is a
correctness issue, not a memory one.

**P2 passes every kind test as an Arisen.** The uPlayer ctor gives
bit 0x2, which about 939 functions test (`+0x206D`, any class, an upper
bound) [scan]. That is right for co-op: damage reactions, pick-ups and
so on treat P2 as an Arisen. The one exception is the record binding:
with bit 0x2, `0x00B5A820` would point P2 at P1's record. Proposal #3
becomes a wrapper on `0x00B5A820`: for a remote seat's unit, the
record is that seat's `cPlayerInfo`. That is 1 function, cold.

### 6.3 The 128 unattributed `sPlayerManager` readers [scan]

They have no labelled caller within 4 levels. Attributed now by their
labelled address neighbours (within 0x4000 bytes; MSVC lays out a
source file's functions together) or by the address blocks already
known:

| Group | Functions | Where | Co-op rule |
|---|---:|---|---|
| stage modes (`aStageNNN`, `aGameMain` code) | 22 | 0x004F..0x0053 | host only, seat 0 (placement, stage start) |
| stage event code | 22 | `0x00504B40..0x00516E00`: event modes `0x0050BE20`/`0x0050C7A0`/`0x0050DCD0`, group-bit writers `0x00514A90`/`0x00516460`, busy test `0x00516E00` (EVENTS.md) | #10 |
| pawn AI think conditions | 13 | `0x00589C10..0x0058FC10`, beside the follow conditions of PAWN_AI.md | #5 (owner's Arisen), hot |
| AI (`sAI*`, `cThinkFSM`) | 15 | | #5 |
| quests and scenario arguments | 15 | `cScenarioArg_*` and quest code | host only, seat 0; review for "the player is near" tests |
| GUI | 13 | | local seat |
| `uInn` | 7 | | #12 (personal) |
| other | 18 | links, shops, sound, items, enemies, managers | review |
| none nearby | 3 | `0x00442580`, `0x00648510`, `0x00648AF0` | review |

So about half are stage, event and quest code: cold, run on the host,
where slot 0 meaning P1 is correct. About a fifth are AI and pawn AI:
hot, covered by the seat-relative rule.

### 6.4 What L40c settled

- **Pawn AI and `mpPlayer`:** 6.5.
- **The effect entry points:** 6.6.
- **The slot-0 uses:** 6.7.
- **The pose test:** 8, a spec to run once the boot reaches gameplay.

Still open after L40c:
- the provider's motion-link evaluator, which co-op no longer needs
  (6.6);
- the per-site review of the identity tests (6.7);
- the 35 unresolved slot-0 uses (6.7).

### 6.5 Pawn AI and `mpPlayer` (L40c) [exe, scan]

`uCmc+0x585C` (`mpPlayer`, reflected) is read by **two functions**, and
neither is AI:
- `uCmc::vf16` `0x00B53710` clears it when the unit it points to is
  gone (state not 1 or 2);
- `uCmc::vf03` `0x00B53490` is the reflection listing (`"mpPlayer"`).

It is written only by the two party spawners, `0x00501020` and
`0x0043D2B0`, which store slot 0. (`0x006A1820`, NPC talk code, writes
`+0x585C` of a kind-0x8 unit: another class at the same offset.)

These are all the lifted sites that name the offset. So **no pawn
behaviour reads its master.** Pawn AI finds "its" Arisen only through
`sPlayerManager+0x99C`: follow `0x00585D00`, the 13 think conditions
`0x00589C10..0x0058FC10` (6.3), and `sAICharacterInfo`.

**For P2 (*Proposal*):**
- A hired pawn spawned for P2 gets `mpPlayer` = P1 from the original
  spawners, since they store slot 0. The seat adapter of #1 sets it to
  P2's Arisen instead.
- Nothing in the original reads `mpPlayer`, so the field is free to
  serve as the owner record. The seat-relative slot 0 of #5 reads it:
  inside a pawn's update, slot 0 returns that pawn's `mpPlayer`. The
  shared main pawn keeps P1 (seat 0), which matches COOP_DESIGN's rule
  that the main pawn follows the host while both command it.
- `uCmc::vf16` keeps clearing it when that Arisen is gone (P2
  disconnects). The rule then falls back to the nearest Arisen.

### 6.6 The effect request entry points (L40c) [exe]

Every effect from an `rEffectProvider` (EPV, EFFECTS.md) goes through
one request and two creators:
- **`0x00429680(element, provider, owner, index, …)`**: 345 callers.
  - It bounds-checks `index` against the provider's index table
    (`+0x68`, count `+0x6C`, 12 bytes per index) and `element` against
    that index's elements (0xE0 bytes each in memory: the loaded form
    of the elements of EFFECTS.md).
  - It reads the camera eye (`sCamera+0x34`, `+0x40`) for the creator.
  - It dispatches on the element's type at `+0x28`: **0 →
    `0x004298B0`** (15.5 KB; *inferred* to build the effect units: it
    reaches the `uEfCam` constructor through `0x0042DED0`);
    **1 → `0x0042D560`**.
- `0x00429790`: every element of an index whose id (`+0x2C`) matches;
  19 callers. Among them are `uPlayerBase::vf93`, the stage set-up
  `0x00501020` and enemy death actions.

Callers of `0x00429680` by class: 147 unlabelled, `cPlAct*` 28,
dragon actions (`cEm580*`) 20, objects (`uOmObj*`) 17, shells 6;
most of the rest are enemy classes. Hit effects come through `uObjModel::vf53`
`0x008876A0` → `0x0042EFC0` / `0x0042FA60` / `0x0042FE90` (*inferred*
from its hit-info argument). Other routes into the two creators:
- `0x00429520`: stage and light code, 8 callers;
- `0x0042FB60`: `uShlArrow::vf116`;
- a few enemy `vf33`/`vf05` directly.

**For replication (*Proposal*, refines 7.4):**
- The host wraps the **two creators** `0x004298B0` and `0x0042D560`
  (8 callers each). They catch every provider effect, whoever asked:
  actions, hits, motion links, stage.
- Each call is sent as an event: provider resource path, index,
  element, owner net id, and the creator's position and flags
  arguments.
- The client calls the same creator with the owner mapped to its
  replica. Effects attached to an owner's joints then follow the
  replica's pose.
- Replicas never request effects themselves (the tail of
  `uCharacterBase::vf92` stays off, 7.3). So nothing spawns twice.
- *Not found:* the evaluator of the provider's motion links (`+0x70`
  records, count `+0x74`). The wrap at the creators makes it
  unnecessary for co-op.

### 6.7 The slot-0 uses, classified (L40c) [exe]

L40's use classes (1.4) came from single lifted blocks. That method
missed uses after a branch and misread null tests as identity tests.
L40c reclassifies all 463 functions with a slot-0 load from Ghidra's
decompiles, which follow the value across blocks. The rules:
- The value is the load `*(…)(DAT_018fa4ec + 0x99c)`, directly or
  through a local assigned from it, or from a local holding
  `DAT_018fa4ec`.
- The local is followed to the end of the function. That is an upper
  bound, since Ghidra reuses local names.
- A comparison with an integer literal is not an identity test.
- Each function is counted under its strongest use, in the order of
  the table.

| Use | Functions | Meaning for co-op |
|---|---:|---|
| identity: compared with another pointer | **32** | "is this unit the player?" (#6) |
| passed on: a call argument, or stored | 108 | the Arisen as a target or parameter: AI targets, camera work, effect owners |
| reads its state (fields, vfuncs, the record) | 131 | status, kind bytes, action, `+0x3DEC` record |
| reads its position only (`mPos +0x40`, world row `+0xE0`) | 75 | distance and direction: LOD and update rates (e.g. `0x00853DC0`), aggro ranges, sound and effect anchors (`0x00432370`: the Arisen's position, else the camera's) |
| existence test only | 82 | "is there a player": gates |
| unresolved (a pattern the rules do not cover) | 35 | review |

By system:

| System | identity | passed | state | position | existence | unresolved |
|---|---:|---:|---:|---:|---:|---:|
| enemies | 20 | 34 | 27 | 15 | 14 | 6 |
| unattributed (6.3) | 1 | 32 | 26 | 14 | 22 | 13 |
| GUI / HUD | 0 | 13 | 24 | 7 | 12 | 1 |
| AI | 0 | 1 | 9 | 5 | 5 | 7 |
| NPCs and humans | 1 | 3 | 2 | 9 | 8 | 2 |
| player | 4 | 5 | 6 | 1 | 2 | 2 |
| other classes | 0 | 4 | 11 | 3 | 1 | 1 |
| objects | 0 | 2 | 4 | 9 | 2 | 0 |
| camera | 1 | 1 | 9 | 2 | 2 | 2 |
| pawns | 1 | 3 | 3 | 3 | 5 | 0 |
| stage, sky, weather | 0 | 4 | 5 | 2 | 1 | 0 |
| other managers | 1 | 3 | 1 | 0 | 4 | 1 |
| shells | 3 | 2 | 0 | 1 | 1 | 0 |
| sound | 0 | 0 | 2 | 2 | 2 | 0 |
| events, effects, items | 0 | 1 | 2 | 2 | 1 | 0 |

L40's 151 "not classified" functions split into:
- 52 state;
- 45 passed;
- 19 existence;
- 13 identity;
- 4 position;
- 18 unresolved.

**Correction to 1.4 and #6:** the identity tests are **32 functions**,
not "≥ 38". Of the 38 lifted-block hits, 34 are null tests followed by a
dereference in the decompile. 4 are real identity tests; the other 28
were invisible to the block-local scan.

**The 32 identity tests** (#6):
- **enemies (20):** `0x004D8150` (`uEm7001`), `0x004E06E0` (`uEm9100`),
  `0x008D2930` (`uEm0100`), `0x008F3830` (wolf), `0x008FB6C0`
  (`uEm0204`), `0x0092C8C0` (zombie), `0x00958BE0` (`uEm0900`),
  `0x00A40F70` (`uEmDragonBase::vf196`), `0x00A68400` /
  `0x00A76580` / `0x00A7B3C0` (dragon `cEm5800` actions), `0x00A88720`
  (wight), and `0x00A97260`, `0x00A986C0`, `0x00A9A7D0`, `0x00A9C7D0`,
  `0x00A9ED60`, `0x00AA05C0`, `0x00AA36C0`, `0x00AA4AC0` (`uEm8000` to
  `uEm9000`);
- **player (4):** `0x00B45370`, `0x00B4B6F0` (staff actions),
  `0x00B855C0`, `0x00BB5840` (`uPlayerBase` code);
- **shells (3):** `0x00C04180` (`uShlHealingSpot::vf108`),
  `0x00C145E0`, `0x00C14E60` (`uShlThunderSpark`);
- **one each:**
  - `sSetManager::vf06` `0x004A40D0`;
  - `cCmcUseItem` `0x00599DB0`;
  - `uCameraGame::vf23` `0x007DCF30`;
  - `uNpc::vf90` `0x00BAFB00`;
  - `0x00964720`.

**Co-op reading (*Proposal*):**
- **Identity (32):** reviewed one by one. Most enemy sites compare a
  target or grabbed unit with the Arisen (a grab, a hit reaction), so
  they mean "an Arisen". The rule of #5 does not fit them; they need
  `is_arisen(x)` (kind bit 0x2, which P2 has). The shells (a healing
  spot, a spark chain) compare their owner or touch target with the
  player: the same.
- **Position only (75) and existence (82):** satisfied by the
  seat-relative slot 0 of #5 as it stands (the nearest Arisen, or the
  owner for a pawn).
- **Passed (108) and state (131):** also #5. Where a GUI or camera
  function is among them, it runs for the local seat only (section 3).

## 7. Stage 1 replication design (L40b, *Proposal* on [exe] facts)

Stage 1: two Arisen in one area. The host simulates; the client sees
both and moves its own. This section is concrete enough to implement.
The facts it rests on are marked as before. Byte counts are raw, before
any compression.

### 7.1 Input upload: the pawn's virtual-pad path [exe]

Every character has a pad object at `uPlayerBase+0x32B4` (0x90 bytes,
up to the parameter slots at `+0x3344`). Its owner is at `+8`. Two
paths fill it:

| Pad field | Local path (`+0x60` = 1, the Arisen) | Virtual path (`+0x60` ≠ 1, pawns) |
|---|---|---|
| `+0x0C..+0x2B` sticks out (`+0x10` the stick's world angle, `+0x14`/`+0x18` the stick) | `0x00B51EE0` reads the 4 pads, the keyboard and the mouse, and rotates by `sCamera+0x34` | `0x00B51EE0` copies `+0x40..+0x5F` → `+0x0C..+0x2B`, then clears `+0x40..+0x54` |
| `+0x30` held, `+0x2C` pressed, `+0x34` released, `+0x38` last held | `0x00B52880` from the key table (`0x00B52A00` ×3) | `0x00B52880`: held = `+0x3C`, edges against `+0x38` |
| `+0x3C` buttons in, `+0x40..+0x5F` sticks in | unused | written by `uCmc::vf200` from the AI pad (`cAICtrl+0x40`, bytes `+0x10..+0x33`) |

Both paths apply the same gate. For an Arisen (kind bit 0x2), the
sticks and buttons are zeroed while the game is in a menu or event
(the `sGameSys+0xB883C/+0xB8848/+0xB8850` flags, or the area's
`+0x3834` object).

**Proposal:**
- **Client:**
  - its own Arisen's pad runs the local path, as in the original
    (7.3 step 1). That is the client's own devices, rotated by its own
    camera;
  - after `0x00B52880` it sends the 32 bytes of sticks out
    (`+0x0C..+0x2B`) and the held word (`+0x30`).
- **Host:**
  - P2's pad has `+0x60 = 0`, set once at spawn;
  - each frame before pass A, the host writes the received sticks to
    `+0x40..+0x5F` and the held word to `+0x3C`;
  - `vf200`, `0x00B51EE0` and `0x00B52880` then run unchanged: the
    original's own code derives the edges and applies the gate.

So the wrapper of proposal #4 is a write of 36 bytes before
`uPlayerBase::vf200`. No camera has to be sent. Both worlds use the
same axes; only the floating origin differs (7.2).

**Message (client → host):**
- header 8 bytes: seat, client frame;
- 36 bytes per frame;
- each packet repeats the last 3 frames against UDP loss.

That is 116 bytes per packet; at 30 Hz, about 3.5 KB/s up.

*To verify by test:* the local path writes `+0x3C` for the next frame,
so the held word may lag one frame there.

### 7.2 Per-character state (host → client) [exe, DTI]

| Field | Where | Bytes |
|---|---|---:|
| net id | assigned by the host at creation (7.5) | 2 |
| position | `uCoord.mPos` `+0x40` (vector3, cm), **plus `sWorldOffset` `mWorldAbsoluteOffset`** (the positions are in a floating-origin frame; ACCEPTANCE.md, ORACLE.md) | 12 |
| rotation | `uCoord.mQuat` `+0x50` | 16 (8 quantised) |
| animation step | `+0x2EEC`: the final step `uCharacterBase::vf92` computed this frame (status and slow factors included) | 4 |
| motion layers | `uModel::mMotion` `+0x380`, 0x140 each, count `+0x374`. Per active layer: `mMotionNo +0x04`, `mState +0x06`, `mAttr +0x0C`, `mInterFrame +0x40`, `mInterCount +0x44`, `mBlend +0x48`, `mFrame +0x4C`, `mSpeed +0x5C` | 28 × layers |
| kind and status | `+0x206C..+0x206F` (kind bytes), `+0x26A0` (status bits read by phase 92 and `vf200`) | 8 |
| vitals (party) | the record: `mHp` (+0x29C, *inferred*), `mHpMaxWhite +0x2A4`, `mStamina +0x2A8`, `mCStatusNum +0x35C` | 16 |
| action | the current action of the action manager `+0x2DB8` (FRAME.md 5.2). *Not traced:* the offset of its id | 2 |

The frozen blend pose (`uModel+0xDD0`) is **not** sent. The replica
freezes its own current pose on a motion change, as the host does. With
an identical history the two are identical (7.3).

**Per character**, with 2 active layers (*assumed; to measure*): about
120 bytes.

**A typical scene.** The field at night, from FRAME.md section 4:
- line 10: the Arisen and 3 pawns. In co-op that is 2 Arisen and
  2 pawns;
- line 15: 5 enemies and 3 NPCs.

| Scene | Characters | Per frame | At 30 Hz |
|---|---:|---:|---:|
| stage 1 (the two Arisen) | 2 | ~0.25 KB | ~7 KB/s |
| stage 2, the field scene | 12 | ~1.5 KB | ~45 KB/s (~0.36 Mbit/s) |

Both fit LAN with room to spare, before deltas.

**Not sent per frame:**
- the attached units: `uWeaponPl` (16 in the census, on lines 11/16)
  and soft bodies (cloth, hair). They follow the owner's joints and
  simulate locally. *To verify:* the weapon's sheathed or drawn state
  should follow from the owner's action;
- the 97 swinging plants `uFmSwingBase` (local);
- the HUD (local, from the replicated vitals and its own seat);
- the camera (local).

### 7.3 The replica's move on the client [exe]

A character that the client does not simulate is a **replica**. For a
replica, the class's move slots are bound to adapters. The adapters
check a runtime "is replica" table and otherwise call the original, so
co-op off changes nothing. The slots involved:
- `uPlayer::vf08` `0x00B55F20`, `uCmc::vf08` `0x00B53650`; in stage 2
  also `uEnemy::vf08` `0x00AA94D0` and `uNpc::vf08` `0x00BAF4F0`;
- slots 9 and 10 (`uPlayer` `0x00B56390`/`0x00B56340`, `uCmc`
  `0x00B54050`/`0x00B53FC0`, `uCharacterBase` `0x0084C720`/`0x0084C6A0`)
  return at once for a replica.

**The replica's slot 8, in order:**
1. **Only the client's own Arisen:** run its pad,
   `0x00B51EE0(+0x32B4)` and `0x00B52880(+0x32B4)`, where `vf200`
   would run them. Send the result (7.1).
2. Set the motion layers to the snapshot's state as of the **start** of
   the host's frame. Set `cUnit.mDeltaTime` (`+0x14`) to the host's
   animation step `+0x2EEC`.
3. **Motion advance:** `uObjModel::vf92` `0x00886210` → `0x010629F0`.
   That is the pose freeze on a change `0x01061DD0`, the root motion
   `0x0105B740`, and the frame and blend advance `0x01060730`
   (FRAME.md 5.2, ANIMATION.md).
4. Overwrite `mPos` and `mQuat` with the snapshot's end-of-frame
   values. This discards the root motion just added, so the host stays
   authoritative.
5. **Commit:** `uObjModel::vf93` `0x00886280`: the world matrix
   `uCoord::vf20` `0x00DEABC0`, the joint pose `0x01058A70`,
   constraints. The replica's step mask `+0x2398` has **bit 4 cleared**,
   so collision activation `0x0076B470` does not run and the replica
   never creates hit volumes. The mask's other bits (0x40, 0x80, 0x100,
   0x200, 0x400, 0x800, 0x1000, 0x2000) select further sub-steps of
   `vf93` and `vf92`; *to review* one by one.

**Not run on a replica:**
- phase 90: status, AI think, stamina, level-up;
- phase 91: actions and integration;
- the tail of `uCharacterBase::vf92`: motion-flag camera work
  `0x004219A0`, shake `0x00CADE50`, the attached sub-units
  `0x0081F280`, `0x0080B450`. Stage 2 adds them back after review
  (7.4);
- pass B (sensors), pass C (push-out, hit volumes);
- the hit pass (step 33) and the character push-out (step 38).

The replica must not appear in the latter two. *To check:* whether the
replica's collision objects are registered at all once bit 4 stays
clear.

**Why the pose should match bit for bit** (*inferred; the test of
6.4*):
- Steps 3 and 5 are per-unit computations. Same code, same install data
  and the same inputs (layer state, step, transform) give the same
  result.
- On a motion change the freeze copies the replica's previous pose. By
  induction that equals the host's, as long as no snapshot was missed.
- After a loss the blend source differs only until the blend ends
  (`mInterFrame`: up to 8 frames for player locomotion, ANIMATION.md),
  then the pose converges again.

The test: one host, one client, the same recorded input, compare the
joint matrices of both Arisen per frame.

### 7.4 Effects and sounds

- **Motion-driven:** the motion flags (`+0x494`, previous `+0x4A4`)
  feed camera work, shakes and the sub-units in the tail of
  `uCharacterBase::vf92`. The effect provider's motion links
  (`rEffectProvider`: 144,146 links, EFFECTS.md) tie effects to motion
  frames.
  - If the evaluator runs from the motion state (where is not traced,
    6.4), a replica that runs the tail reproduces these effects and
    sounds locally, in step with its pose.
  - Stage 1 leaves the tail out.
- **Action-driven** (phase 91, host only; the entry points are in 6.6): spells, arrows and other
  shells (`uShl*`, line 18). Proposal for stage 2:
  - shells become replicated units (spawn event with class and owner,
    then transforms);
  - one-shot effects spawned by actions are sent as events from a
    wrapper on the effect request. *To find:* the entry point.
- **Hit results** (step 33: damage, sparks, reactions) are host events.
- **Camera work** for a remote seat's Arisen is a camera event to that
  client (proposal #7).

### 7.5 Transport, and why not `sNetworkExt` [doc: L32]

L32's census (`task/L32-network` `fee078b`) shows what the guest has:

- **One socket class** (vtable `0x01428BD8`, factory `0x00D8B690`):
  - TCP or UDP, non-blocking;
  - `SO_BROADCAST` on UDP;
  - `sendto` at `0x00DA23F3`, `recvfrom` at `0x00DA2912`.
- **Its users:**
  - `sDDCaProto`: Capcom's TLS login, `dune.dragonsdogma.com:12501`;
  - `sNetwork` sessions (`0x011562C0` via `0x00D8B5F0`), only in
    network states 3/4 (online and logged on to Steam);
  - `sNetworkExt`'s storage tasks (`cNetStorage*`), which require an
    online session (`sNetwork+0x220`).
- **No LAN discovery, and no game-state protocol.** The messages are
  the online services' (pawn sharing, rewards).
- The runtime keeps Winsock "not initialised", as measured (L32).

**Decision (*Proposal*):**
- Nothing at the message level is worth reusing. The socket class is a
  thin wrapper of WS2_32. Driving it would need a live Winsock in the
  guest plus new guest-side code, and nothing in it is specific to
  game state.
- The co-op transport is therefore host-side Rust in the runtime
  process, outside the guest. Input injection (7.1), snapshot capture
  and apply (7.2-7.3) are runtime adapters that read and write guest
  memory at the frame's sync points.
- `sNetworkExt::vf06` stays at step 54 and stays offline, as L32
  implements it. The snapshot hook runs right after it (4.2).

**Messages:**

| Message | Direction | Content |
|---|---|---|
| Join | client → host | profile, the client's `cPlayerInfo` (0x7F0 bytes) and edit data |
| Area | host → client | stage index (stage loader `0x004FFBA0`), spawn point, the seat table |
| Input | client → host | 7.1, unreliable, ×3 redundancy |
| Snapshot | host → client | host frame, `sWorldOffset`, per character 7.2, unreliable |
| Events | host → client, reliable | spawn and despawn (class, net id, owner), event start and end, camera work, effect spawns (stage 2) |

**Net ids:** units are created on worker threads in no fixed order, so
the host assigns an id at creation. In stage 2 that means wrapping the
creators named in FRAME.md 3:
- `0x00421FE0` characters;
- `0x00433E00` enemies;
- `0x0049DA10` objects;
- `0x004BA540` by DTI.

Stage 1 needs only two fixed ids: seat 0's and seat 1's Arisen.

### 7.6 Stage 1 change list

| Side | Function | Change | Hot/cold |
|---|---|---|---|
| both | `0x00501020` (stage set-up) | after it: create the other seat's `uPlayer` in slot 3; its pawn loop skips slot 3 in co-op | cold |
| host | `0x0043D2B0` (party upkeep) | skip slot 3 in co-op | per frame, trivial |
| both | `0x00B5A820` (record binder) | the remote seat's unit → its own `cPlayerInfo` | cold |
| host | `0x00B6AEA0` (`uPlayerBase::vf200`) | P2: write the 36 received bytes into the pad first | hot, cheap |
| client | `0x00B55F20` / `0x00B56390` / `0x00B56340` (`uPlayer` slots 8/9/10) | the replica move of 7.3 | hot, cheaper than the original move |
| host | after step 54 | capture the snapshot of both Arisen | runtime |
| client | after step 54 | apply the snapshot (the start-of-frame layer state for the next move; end-of-frame transforms) | runtime |
| both | transport | 7.5 | runtime |

**In total:** 7 guest functions wrapped, two runtime hooks and the
transport.

**Acceptance:**
- each player walks, runs and jumps;
- the other machine shows both Arisen within two frames, with
  bit-identical joint matrices in steady state (the 7.3 test);
- the one-seat gate is unchanged with co-op off.

## 8. The first co-op test: replica pose bit-exactness (L40c, spec)

This is the first test to run once the boot reaches gameplay. It checks
the claim of 7.3: a replica that runs only the motion advance and the
commit, from the host's state, shows **exactly** the host's pose. If it
fails, the whole replica design is wrong, so it comes before any
gameplay work.

### 8.1 Set-up

- **Two guest instances on one machine**, from the same install:
  - a **host** with co-op on (seat 0, and seat 1 remote);
  - a **client** (seat 1).

  The runtime may give each its own process (`ddplay` host and client
  on loopback) or two guest memories in one process, if it supports
  that. Their records (8.3) go to two files, compared by the test.
- **Saves:** the host continues the maintainer's private save (the
  Continue path, GAMEPLAY_PATH.md). The client joins with its own
  Arisen, a second private save or a copy of the same one; only its
  `cPlayerInfo` and edit data are sent.
- **Area:** the host's Continue area. With co-op on, the host disables
  enemy and NPC spawns for this test, so only the party exists. The
  test needs the two Arisen only, and stage 1 has no other replicas.
- **The host's step is pinned to 1/30 s** (`mDeltaTime`, COOP_DESIGN).
  That makes runs repeatable. The comparison itself does not depend on
  it.
- **Transport:** loopback, in two variants: lossless; and "lossy",
  dropping 5% of snapshots with a fixed seed.

### 8.2 Inputs: scripted pads

No live devices are used. Each seat's pad is a script of 36-byte
records (7.1: sticks out `+0x0C..+0x2B`, held word `+0x30`), one per
frame:
- the **host** writes seat 0's script into seat 0's pad, with `+0x60`
  set to 0 for the test, through the same virtual path P2 uses;
- the **client** feeds seat 1's script in place of its devices, then
  uploads it as in 7.1.

Script (600 frames per seat; the two seats are offset by 45 frames, so
their motion changes do not coincide):

| Frames | Input | Covers |
|---|---|---|
| 0-29 | none | idle |
| 30-119 | stick forward, half tilt | walk, the locomotion phase match (ANIMATION.md) |
| 120-209 | full tilt, turning 90° over 30 frames | run, turn blend |
| 210-299 | sprint held | sprint, stamina |
| 300-389 | three jumps (press, 1 frame) at 300, 340, 380 | takeoff, airborne, landing, interpolation 3/0/2 |
| 390-479 | stop, then walk backwards | stop blends, direction change |
| 480-599 | none | idle again; all blends must have ended |

Stage 2 extends the script with weapon draws, attacks and a skill, once
the motion-flag tail (7.4) is in.

### 8.3 What is recorded and compared

For each frame N and each Arisen (seat 0 and seat 1), the record holds:

| Field | Host: when | Client: when |
|---|---|---|
| `uCoord` `mPos +0x40`, `mQuat +0x50` | after the unit's `uObjModel::vf93` in pass A | after the replica's commit (7.3 step 5) |
| the world matrix (`uCoord` row 3 at `+0xE0..`, the whole `+0xB0..+0xEF` *inferred* as `mWMat`) | same | same |
| per motion layer: `mMotionNo`, `mFrame`, `mBlend`, `mInterCount` | same | same |
| per joint (`uModel` `mJoint`, 0xA0 bytes each): `mQuat +0x60`, `mScale +0x70`, `mTrans +0x80`, and the joint's world matrix (*to locate*: not reflected) | same | same |
| motion flags `+0x494` | same | same (computed by the advance, not sent) |

- **Equality is bitwise** (the f32 bit patterns), never a tolerance.
- **Alignment:** the client's frame for host frame N is the one that
  applied snapshot N. The record carries the host frame number, so the
  test aligns by that, not by arrival time.
- A difference is reported as: frame, seat, field, joint index, host
  bits, client bits.

### 8.4 Pass and fail

- **Lossless:** every field equal on every frame, from the first frame
  both Arisen exist plus 2 (snapshot latency) to the end. Both seats
  are checked: seat 0 is a replica on the client, seat 1 on the host.
- **Lossy:** after a dropped snapshot, differences are allowed only
  from that frame until the active blends end (the layer's
  `mInterFrame`: at most 8 frames for player locomotion, ANIMATION.md)
  plus 2. After that, every field is equal again. Transforms (`mPos`,
  `mQuat`) are equal on every frame that applied a snapshot.
- **Negative control:** the same run with the client's received
  `mFrame` of seat 0 changed by one ULP at frame 150 must fail at frame
  150. That proves the comparison sees a real difference.

### 8.5 Where it runs

- An install-gated integration test of the runtime: the test skips
  without the game install, like the other install tests. It runs in
  the integration build through `heavy.py` (AGENT-RULES), never in the
  light crates.
- **Prerequisites:**
  - the boot reaches gameplay through Continue (GAMEPLAY_PATH.md);
  - the stage-1 adapters of 7.6;
  - the record hooks;
  - the scripted pad.
- The record hooks are runtime-side and write nothing into guest
  memory.
- **One-seat gate:** with co-op off, the same build passes the existing
  comparison unchanged. That check runs in the same job.

### 8.6 Likely first failures (to look at first)

- **Constraints and IK in `vf93`** (`uCns*`; uPlayerBase holds
  `uCnsLift`, `uCnsEff`, `uConstraint`): they read the ground and other
  units (look-at, foot placement). On the client, the ground is the
  same stage data and the other units are replicas at the host's
  transforms. But a constraint that reads a unit *not* replicated, or
  state from phase 90/91 the replica skipped, will differ. The step
  mask `+0x2398` bits that select these sub-steps (7.3) are where to
  look.
- **The animation step:** `+0x2EEC` must be the step the host's advance
  actually used (written just before `uObjModel::vf92`, FRAME.md 5.2).
- **The floating origin:** `sWorldOffset` may differ between the two
  worlds. `mPos` is compared after removing each world's own offset;
  joint world matrices likewise (translation only).
- **Jobs:** the host moves line 10 as parallel jobs. One unit's advance
  reads only its own state, so this does not matter unless a
  constraint reads another party member mid-move. That is a host-side
  race the original has too, and it would show up as an occasional
  single-frame difference.
