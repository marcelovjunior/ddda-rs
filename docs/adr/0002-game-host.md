# ADR 0002: The game host and the LAN co-op protocol

- **Status:** proposed (claude-10, 2026-10-09)
- **Brief:** [host-architecture](../tasks/host-architecture.md)
- **Evidence:** [FRAME.md](../FRAME.md) (the original's frame order),
  [RECORDINGS.md](../RECORDINGS.md) (the clock),
  [COOP_DESIGN.md](../COOP_DESIGN.md) (the maintainer's decisions)
- **Code:** `crates/ddhost`

There is no ADR 0001 in the repo; the number follows the brief.

Two kinds of statement appear below:
- **Traced:** taken from the exe or a recording, with its address or
  session.
- **Decision:** a design choice with no counterpart in the original, which
  has no LAN co-op. Each one says why.

## Context

The game's rules live in headless crates:
- `ddsim`: movement, stamina;
- `ddstory`: story world, scripts, quests, scenarios, events;
- `ddeconomy`: wallets, bags, shops;
- `ddrift`: the Rift;
- `ddprogress`: experience, levels, skills;
- `ddcombat`: damage rules;
- `ddworld`: collision.

Each is written for N players and keeps its own state. Something has to:
- own one session's state;
- step the systems in the right order at the right rate;
- know who the players are;
- save and load the whole session;
- be the authority that LAN clients talk to.

That is the host.

## 1. `ddhost` owns the session

`ddhost::Host` holds:

| Field | What |
|---|---|
| `frame`, `clock` | frames run, the fixed step (section 2) |
| `roster`, `party` | the seated Arisen and the four party slots (section 4) |
| `sim` | `ddsim::Sim`, every Arisen's body |
| `story` | `ddstory::Story`, the session's one story |
| `world` | a `StoryWorld`: actors and presentation the story talks to |
| `ground` | `ddsim::Ground` (stage collision from `ddworld`) |
| `locks` | the NPC interaction locks (section 5) |
| `economy`, `progression`, `rift` | `ddeconomy::Economy`, `ddprogress::Progression`, `ddrift::Rift` |
| `data` | their game data (`SystemData`: catalog, tables, Rift data) |

`Host::tick(&FrameInputs)` runs one frame. It is the only place that knows
the system order. The systems never call each other: the host passes
outputs of one as inputs of the next (story requests to the world, world
events to the story, inputs to the sim).

**Decision:** one host type for every mode.
- Single-player: one local seat, no network.
- LAN host: a local seat plus remote seats fed by the network.
- Tests: scripted inputs.

`ddhost::install::sim(&Vfs)` builds `ddsim::Sim` from the install for every
user (host, `ddview --play`, `ddaccept`), so their simulations cannot
differ. It loads whole motion banks rather than a hand list of motion ids.

## 2. Time: a fixed step in the original's units

**Traced** (RECORDINGS.md; FRAME.md section 3):
- The original measures time in 1/30-s frames (`sMain.mFps` = 30).
- It scales every per-frame rule by a variable step, `sMain.mDeltaTime`
  (+0x70), computed from the QPC timer by `sMain::vf06` (0x00DBFA90).
- The step is capped by `mDeltaTimeLimite` (2.0, written each frame by
  `sBbsRpgMain::vf06`, 0x0041DD60).
- Each unit's `cUnit.mDeltaTime` (+0x14) is that step times its move
  line's rate (0x00DB7384). The rate was 1.0 on all lines in a live read.

**Decision:** the host steps a constant `delta` original frames per host
frame:
- default 1.0, so 30 host frames per second;
- `FrameClock::new(0.5)` gives 60 Hz.

Why:
- The rules are per 1/30-s frame times delta, so any constant delta is the
  original's model.
- Rules that count frames rather than time (script state counters, AI
  timers) run at their design rate at 1.0.
- A constant step makes the host deterministic (section 7), which replay
  tests and client prediction need.

`ddsim` keeps its own 1/120-s step (`TICK` = 0.25 frames) inside each host
frame. `FrameClock::seconds() * MOTION_FPS` equals `delta` exactly in f32
(unit test), so a 1.0 frame is exactly four ticks and never drifts.

The views render at any rate and interpolate between host frames. That
is presentation, not simulation.

## 3. System order

The host runs the original's order (FRAME.md), cut down to the systems
that exist. The table maps each host phase to the original step it
stands for.

| Host phase | Original (FRAME.md section 2 step) | Crate |
|---|---|---|
| 1. step | 6 `sMain::vf06` (clock) | `FrameClock` |
| 2. world completions | 18-20 (stage and event modes) | `StoryWorld::events` |
| 3. story: QCT, events, scenarios, scripts | 20 (`aGameMain`, `aStage`, `sScenario`), 36 line 0 | `ddstory` |
| 4. restock once per new day | 20 `sNpcManager::vf06` 0x00475AD0, in aGameMain's managers before `sQuestManager` | `ddeconomy::Economy::update` |
| 5. input latch | 22-25 `sQlocInput`, `sKeyboard`, `sMouse` | `input` |
| 6. AI objects | 30 `sAI` | AI runtime (next) |
| 7. hits from the last frame's volumes | 33 `0x004792C0` | `ddcombat` (next) |
| 8. Arisen and pawns move; level-up and rank-up (phase 90) | 36 line 10 "Player" (0x00B5BD20, 0x00B5C230) | `ddsim` (Arisen done), `ddprogress::Progression::step` |
| 9. enemies and NPCs move | 36 line 15 "Enemy" | next |
| 10. push-out between characters | 38 `0x004797E0` | next |
| 11. AI senses | 40 pass B | next |
| 12. wall push-out, hit volumes on | 42 pass C | `ddsim`, `ddcombat` (next) |
| 13. requests performed, cameras, sound | 50-53 | `StoryWorld::handle`, views |

**Traced** consequences the host keeps:
- The story sees the positions where the last frame left the Arisen.
- Hits are resolved from the volumes switched on in the previous frame,
  before anything moves.
- A scenario press reaches the story one frame after it was made (the
  prompt is set during the player's move; *inferred*, FRAME.md 6.2).

One `Sim::step` is one engine frame (ddsim's rate queue,
`Player::unit_rate`), so the host calls it exactly once per host frame.

The story's internal order is traced as QCT → event mode → scenarios →
scripts. ddstory runs scenarios first; changing that is ddstory's owner's
call (requested on the board).

**Decision:** within one move line, units run sequentially in a fixed
order: seat order for the Arisen, then creation order.

**Traced:**
- The original queues each line's units as jobs (line flag bit 0; all 37
  lines had it in a live read).
- It runs them on `sMain`'s worker threads, then waits at a barrier
  before the next line (0x00DB7350, 0x00478560, 0x00DBE560).

So no original rule can depend on the order of units within a line;
such a rule would be a data race there. Any fixed order is therefore
faithful, and a fixed one is deterministic. *Inferred, not proven: a
line whose job count is 1, or a machine with one worker, runs its units
inline.*

## 4. Player identity

**Decision:** two identities.

- `PlayerId` (u8) is the **seat** in this session. Seat 0 is the host's
  Arisen. It is the id in `ddsim`, and the id every per-player system
  should take. The story receives players in seat order (`ddstory::Inputs`
  says "host first"), so the story's player index is the seat's rank.
- `ProfileId` (u64) names an **Arisen across sessions**: whose character
  and save chunks these are. A guest can sit on a different seat next time.
- `Roster`:
  - seats at most `MAX_ARISEN` = 2 (COOP_DESIGN.md);
  - refuses a profile seated twice;
  - reuses the lowest free seat.
- `Party`: four slots in the original's order (`sPlayerManager` +0x99C:
  the Arisen, then three pawns; ORACLE.md):
  - host Arisen;
  - main pawn (shared);
  - one hired pawn;
  - guest Arisen in the last hired slot (COOP_DESIGN.md).

**One identity** (host-integration): the crate `ddid` defines
`PlayerId = u8` (the seat; the same type as `ddsim::PlayerId`) and
`ProfileId(u64)`. `ddeconomy`, `ddprogress`, `ddrift` and `ddhost` all use
it. Before, they used `usize`, `u8` and `struct PlayerId(u8)`.

System crates key their state by seat. Only the host knows profiles: it
files each seat's save part under that seat's profile (section 6).

## 5. The interaction lock

COOP_DESIGN.md: an NPC that one Arisen talks to, trades with or otherwise
uses is locked for the other. The original has nothing like this.

**Decision:** one host-owned table, `ddhost::lock::InteractionLocks`.
- It maps an NPC id (npc +0x5858, the id `ddstory::NpcUnit` carries) to
  the seat that holds it.
- A system that opens an interaction acquires the NPC through the host
  first, and releases it at the end. Such systems include talk, a shop
  session (ddeconomy), the Rift at a Riftstone (ddrift), and the inn.
- The interact prompt is not offered for an NPC another seat holds.
- Leaving the session releases all of the seat's locks.

How the systems use it (host-integration):
- **Shops.** `Host::open_shop` takes the host lock first, then opens the
  economy session. If the economy refuses (closed shop, no shop), the
  lock is released. `Host::close_shop` releases both.
  `ddeconomy::Economy` keeps its own lock map; with the host acquiring
  first, the two can only agree.
- **The Rift is not an NPC lock.**
  - Both Arisen may stand at a Riftstone together (`Host::rift_enter`).
    The first opens the room (uRim) and the last one out closes it.
  - A room pawn being talked to is locked for the other Arisen through
    ddrift's own talk lock (`RoomSlot::talking`).
  - A hire is paid from the hiring seat's wallet, at the cost from that
    Arisen's level (`Host::rift_hire`).
- **Leaving.** A seat that leaves loses its NPC locks, its shop sessions
  (`Economy::close_all`) and its place at the Riftstone.

The story's scenario prompts (`sScenario` +0x108) take one player per
frame. When both press on the same frame, the host gives the press to the
lower seat (`Host::tick`).

## 6. Save and load

The original saves one Arisen (`sSave`, cSAVE_DATA_*).

**Decision:** the host writes its own container (`ddhost::save`, "DDHS"
version 1). It is a list of chunks, each owned by:
- the shared world; or
- one Arisen's `ProfileId`.

Each crate serialises its own state into its chunks; the host only frames
them. The host keeps unknown chunks as they are, so a save survives a
build that lacks a crate.

| Tag | Owner | Content | Status |
|---|---|---|---|
| `HOST` | shared | frame count, the seats with their profiles, who hired each pawn | done |
| `WRLD` | shared | `ddstory::World::save` ("DDST" v1) | done |
| `PLCE` | profile | the Arisen's position and yaw | done |
| `ECON` | profile; shared | wallet, bag, storage; shop stock, restock day, buy-back, gates, pawn bags (`Economy::save`) | done |
| `PROG` | profile; shared | the seat's Arisen record; pawns, character order, party roles (`Progression::save`) | done |
| `RIFT` | shared | party pawns, pawn log, cost multiplier (`Rift::save`) | done |

Each crate writes its parts with `ddid::Writer` as `ddid::Parts { shared,
players }`, keyed by seat. Each part starts with a version byte. Floats
are stored by bits, so a load gives back the exact state.

**Loading on other seats.** The host saves the seat → profile table in
`HOST`. On load:
- it hands each crate the parts of the profiles seated now, under their
  new seats;
- it gives `Progression::load` a seat map (old → new), because its
  shared part names the Arisen by seat;
- a profile that is not seated now is left out, with its party slot.

**Not saved, by design:**
- **Open interactions.** `Host::save` refuses while any NPC lock, shop
  session or Rift visit is open, as the original cannot save in a shop
  or at a Riftstone.
- **The Rift's stock.** It is rebuilt on every load, as the original does
  (`Rift::refill`, 0x0047D330; `Host::refill_rift` with the Rift data and
  an RNG).
- **Running scripts**, as before.

**Two hashes.**
- `saved_hash` covers exactly what a save keeps:
  - frame, seats, party, hirers;
  - each Arisen's place;
  - the story world, economy, progression;
  - the Rift without its stock.

  It is equal before a save and after its load (`tests/session.rs`).
- `state_hash` adds what a load cannot restore and replays must compare:
  - every body field;
  - the story's stage, quests and script states
    (`Story::script_states`);
  - open interactions, pending presses and the request log.

Policy (**decision**; COOP_DESIGN.md left it open):
- The host's save holds everything.
- When a session ends, each guest receives its own profile's chunks
  (`SaveFile::profile_part`), so every player keeps their character.
- When a guest joins, it brings its profile chunks; the host uses them
  for that seat. LAN co-op between friends needs no anti-cheat.

Running scripts are not saved yet; `Story::from_world` restarts them from
the world. The original saves non-`fix` scripts (0x00495150; QUESTS.md).

## 7. Determinism

The host is deterministic on one machine: the same inputs from the same
start give the same state, frame by frame.
- `tests/replay.rs` runs a scripted two-player session twice. It compares
  `state_hash()` every frame, and shows that a one-frame input change
  diverges exactly from that frame.
- `tests/opening.rs` runs the real opening (new game to the boot of
  q0002) twice through the host. Both runs reach the same state, and the
  story climbs the same SceNo ladder as ddstory's own driver.

`state_hash` hashes, with FNV-1a, the `Debug` text of:
- every Arisen (all fields, private ones too; floats print in round-trip
  form, so the text is bit-exact);
- the story world, quests, scripts, and every request emitted.

Rules for system crates:
- No iteration over `HashMap` or `HashSet` in simulation paths (their
  order is random per process). Use `Vec` or `BTreeMap`.
- No wall clock and no thread timing in rules.

Cross-machine determinism is **not** required, and is not promised:
`sin`, `cos` and `atan2` come from the platform. Only the host simulates
authoritatively. A client's prediction may differ slightly; the next
snapshot corrects it (section 8).

## 8. The LAN co-op protocol

Everything in this section is a **decision**: the original has no LAN
co-op. Its online code (`sNetworkExt`, `sDDCaProto`) shares pawns
through Capcom's servers and is not a model for this.

### 8.1 Topology

- **Host-authoritative client/server** (COOP_DESIGN.md). The host machine
  runs `Host` plus its own view.
- A client runs only views and its own prediction. Its rules come from
  the same crates.
- Each machine reads the game data from its own install, so no assets
  cross the network.

### 8.2 Transport

- UDP, one socket, default port 47810.
- LAN discovery is a broadcast on 47811 ("who hosts?"; hosts answer with
  name, seats free and build).
- Every packet starts with a header:
  - magic `DDCO`;
  - protocol version;
  - connection id;
  - sequence u32;
  - the newest sequence received, plus 32 ack bits for the ones before it.
- Two channels in the same packets:
  - **unreliable, newest-wins**: inputs, snapshots;
  - **reliable, ordered**: everything that must arrive once and in
    order. Messages are resent until acked and fragmented above 1,200
    bytes.
- No encryption or NAT traversal: LAN only.

### 8.3 Joining

1. The client sends `Hello`:
   - protocol version;
   - the exe build (PE timestamp 1513180209, through `ddexe`);
   - a fingerprint of the install's data (archive list with sizes);
   - its `ProfileId`;
   - its character's chunks (section 6).
2. The host refuses with a reason if any of these hold:
   - the build or data differ;
   - the roster is full;
   - the profile is already seated.
3. Otherwise the host answers `Welcome`:
   - the seat;
   - the frame clock;
   - the current host frame;
   - a full snapshot.

A leave or a timeout (2 s without packets) unseats the client. Its locks
are released, and its body leaves the world.

### 8.4 Input upload

- The client samples its devices every host frame and builds a
  `PlayerInput` (`ddsim::Input` plus the buttons the host interprets). It
  stamps each one with the host frame it is meant for.
- Each packet carries the newest 8 commands, so one lost packet costs
  nothing.
- The host keeps a queue per seat, keyed by frame:
  - At frame F it takes that seat's command for F.
  - If the command is missing, it repeats the last one. Held buttons
    stay held.
  - It drops commands for frames already run.
- Buttons are sent as **held states**, never as presses. The host derives
  presses from edges itself (`input::Edges`, ddsim's jump), so a repeated
  or resent command never presses twice.
- Each snapshot tells the client how early or late its input for that
  frame arrived. The client keeps its lead at about one frame plus
  jitter.

### 8.5 Snapshots and deltas

The host sends each client one snapshot per host frame. A snapshot has:
- the frame number;
- the last input frame it consumed from that client;
- entity states, delta-encoded against the newest snapshot the client
  acked. A full snapshot goes out when there is no usable baseline.
  - **Entities:** the Arisen, pawns, enemies, NPCs and moving objects.
    Each has position, yaw, velocity, action state, motion and frame,
    HP and stamina.
  - **Shared scalars:** clock, weather, SceNo and the story flags that
    views show.
  - **A private block for the owner only:** wallet, inventory changes,
    menu state.

Reliable events carry what is not state:
- story requests the views perform (`PlayEvent`, `Camera`, `Message`,
  `Tutorial`, `QuestBanner`, `PlaceName`, `Weather`, `Order` sounds and
  effects);
- per-player menus (`OpenMenu { player }`);
- lock grants and refusals;
- rewards.

Each event carries the host frame it belongs to, so the views apply it
together with that frame's snapshot.

### 8.6 What each client renders locally

| Local to each client | From the host |
|---|---|
| its camera (one per player), HUD, menus | every entity's state (snapshots) |
| its own Arisen: **predicted** with `ddsim` from its own inputs | story progress and events |
| effects and sounds, triggered by replicated state and events | damage results, item and money changes |
| cutscenes, played from local data when the event arrives | |
| other entities: **interpolated** 3 host frames (100 ms) behind | |

Reconciliation works like this: when a snapshot arrives, the client:
1. resets its own Arisen to the host's state at the acked input frame;
2. replays its unacked inputs with `ddsim`;
3. smooths any visible correction over a few frames.

Because ddsim is the same code on both machines, corrections are rare and
small.

### 8.7 Latency

- LAN round trips are under 5 ms. The design stays playable to about
  150 ms.
- Own movement is predicted.
- Results only the host can decide wait for the host's confirmation:
  hits and damage, pickups, purchases, story, NPC locks. At 30 Hz that is
  at most two frames on a LAN.
- No lag compensation for hits in the first version. Revisit at M4 with
  measurements.
- The host's own player has zero latency. The asymmetry is accepted.

### 8.8 Per-player and shared

| Per player | Shared (host-owned) |
|---|---|
| the Arisen: character, stats, vocation, skills, equipment | world: stage, time of day, weather |
| inventory, storage, wallet (gold, Rift Crystals) | story: SceNo, GSF, quest bits, layout flags, events |
| camera, HUD, menus, lock-on target | NPCs: schedules, states, affinity (per Arisen? open) |
| interaction locks held | enemies, AI, spawns |
| | the main pawn (the host's; either Arisen commands it) |
| | the hired pawn, the Rift pool |
| | shop stock and restock |

## Alternatives considered

- **Deterministic lockstep:** every machine simulates; only inputs are
  sent. Rejected:
  - it needs bit-identical floats across machines, which platform `sin`,
    `cos` and `atan2` do not give;
  - it needs every system deterministic from day one;
  - it stalls both players on the slower one.
- **Client-authoritative own Arisen:** rejected. Grabs, climbing on
  monsters, carrying and collisions between the two Arisen need one
  authority.
- **Rollback:** unnecessary at LAN latencies.

## Consequences

- Every system crate exposes:
  - a step the host calls, with explicit inputs (no globals);
  - state the host can hash, save and replicate;
  - per-player state keyed by `PlayerId` or `ProfileId`.
- The order of systems lives in one function (`Host::tick`) with
  FRAME.md citations. Changing it needs evidence.
- A client never runs story, AI or combat rules. It only predicts its own
  movement.

## Open questions

- Pause: `sMain.mPause` is only a capture/benchmark pause. Gameplay
  freezes through unit group bit 57, which `0x0043F6C0` clears during
  events and menus (FRAME.md section 3).
  - Proposal: a personal menu freezes nothing in co-op, because the world
    is shared.
  - Events (cutscenes) freeze the shared world for both Arisen, as bit 57
    does in the original.
- Who answers story prompts (dialogue choices, quest acceptance) when
  both press: today the lower seat. COOP_DESIGN.md leaves story ownership
  open.
- Script state in saves (above), and the per-frame `ddstory::Host`
  queries the world must answer in co-op (`player_damaged` asks about
  "the Arisen").
