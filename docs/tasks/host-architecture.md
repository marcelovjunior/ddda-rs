# host-architecture: the game host, its frame order and the co-op protocol

- **Owner:** claude-10
- **Branch:** `task/host-architecture`
- **Board area:** `arch/host`
- **Status:** review

## Goal

The headless systems now exist as separate crates:
- `ddsim`: movement;
- `ddstory`: world state, quests, events;
- `ddrift`, `ddeconomy`, `ddprogress`: in progress;
- `ddcombat`: damage;
- `ddworld`: collision.

Design and start the host that composes them, as the original does, so
that M3 (LAN co-op) and M4 can build on it:

1. **Trace the original's frame order:** what runs each frame and in which
   order. Cover the main loop (0x00DF0D20 → sBbsRpgMain vf06 0x0041DD60),
   `sUnit` move lines and groups (`cUnit.mUnitGroup`), the system
   managers' update order, and where input, AI, actions, physics,
   collision, damage, story and rendering sit. Record it in
   `docs/FRAME.md`, with addresses. Ask claude-4 for live confirmation
   (call order through the oracle's tracing, read-only).
2. **Design the host** in `docs/adr/0002-game-host.md`:
   - the crate `ddhost` that owns the world, players, party and systems,
     with a fixed-step tick (`ddsim`'s 1/120 s step within the original's
     1/30-s units; `RECORDINGS.md`, the clock);
   - the system order taken from FRAME.md;
   - player identity for N players (`docs/COOP_DESIGN.md`);
   - the interaction lock;
   - save/load composition across crates.
3. **The co-op protocol (LAN)**, in the same ADR:
   - host-authoritative simulation;
   - client input upload;
   - snapshot and delta replication;
   - what each client renders locally;
   - latency handling;
   - what is per-player (wallet, inventory, camera) and what is shared
     (world, story, main pawn).
4. **Start `ddhost`:** compose the existing `ddsim` and `ddstory` behind
   the tick, with a deterministic-replay test (same inputs, same state
   hash). Do not change the other crates; request API changes from their
   owners on the board.

## Scope

- `crates/ddhost/**` (new)
- `docs/FRAME.md` (new)
- `docs/adr/0002-game-host.md` (new)
- `docs/COOP_DESIGN.md` (updates)

## Done when

- FRAME.md has the traced order with addresses.
- The ADR is written.
- `ddhost` ticks `ddsim` and `ddstory` together, with the replay test.
- Fill in Results and report.

## Results

claude-10, 2026-10-09, branch `task/host-architecture`.

### 1. The frame order: `docs/FRAME.md`

Traced statically in the exe. The singleton fields and the move lines are
confirmed live (claude-4, #872). The call order itself is not yet
confirmed live (section 8).

- **Main loop** `0x00DF0D20`: one `sBbsRpgMain::vf06` (`0x0041DD60`) per
  iteration, with no fixed rate. The clock is in `sMain::vf06`
  (`0x00DBFA90`).
- **vf06** is 65 steps, each resolved to its singleton through the ctor
  `0x0041B400`. The 66 fields were checked live by their vtables.
  - In order: clock, save, world origin, `sGameSys`, the area stack
    (managers, quests, events, scenarios), input, `sAI`, hits, the unit
    passes, HUD, effects, cameras, sound, weather, network, render, draw
    jobs.
- **`sUnit`** has 37 named move lines (`0x018753A8`) and three passes:
  - A: slot 8, the move;
  - B: slot 10, AI senses;
  - C: slot 9, push-out and hit volumes.

  Within a pass:
  - each unit's step is line rate × `mDeltaTime`;
  - a line's units run as concurrent jobs with a barrier per line (flags
    0x1FD on every line, live);
  - units are gated by the group mask `sUnit+0xD30`. Events and menus
    clear bit 57 (`0x0043F6C0`); `mPause` is only a capture pause.
- **Line contents, live:**
  - line 0: story scripts (uFSMTask, uFSMQuest);
  - line 10: uPlayer and 3 uCmc;
  - line 15: enemies and uMultiNpc NPCs;
  - line 21: camera;
  - then effects, lights and HUD.

  The table of which class goes on which line is from the creators
  (`0x00DB5800`, `0x00421FE0`, ...).
- **A character's frame** (uPlayer, uCmc, uEm0100):
  1. hits from the previous frame's volumes (`0x004792C0`, before pass A);
  2. phase 90: status, AI think, input (slot 200; a pawn's AI fills the
     same virtual pad), stamina;
  3. phase 91: action change and update, then integration `0x0084CCC0`;
  4. phase 92: motion advance and root motion `0x0105B740`;
  5. phase 93: pose commit;
  6. push-out between characters;
  7. pass B: AI senses;
  8. pass C: wall push-out, hit volumes on.
- **The story**, in order:
  1. `sQuestManager` QCT, in `aGameMain`'s managers;
  2. `aStage`'s mode switch (events);
  3. `sScenario`;
  4. input;
  5. scripts on line 0.

  ddstory runs scenarios first today. A reorder was requested from
  claude-7 (#961).
- **Facts new to the docs:**
  - the player's turn rate +0x2584 = m_pl id 3 × π/180 (written in
    `uPlayerBase::vf90`);
  - integration `0x0084CCC0`;
  - root motion `0x0105B740`;
  - the 1.15 unit-step rescale in `uObjModel::vf90`.

### 2. and 3. The host and the co-op protocol: `docs/adr/0002-game-host.md`

The ADR covers:
- the host's fields;
- a fixed step of 1/30 s, with ddsim's 1/120-s ticks inside; one
  `Sim::step` per host frame;
- the system order table, mapped to FRAME.md;
- seats (`PlayerId` u8) against profiles (`ProfileId` u64), and the
  party layout;
- one host-owned interaction-lock table;
- the DDHS chunked save, with per-crate chunk tags;
- the determinism rules;
- the LAN protocol: UDP with reliable and unreliable channels, the join
  handshake with build and data checks, redundant held-state input
  upload with a per-seat queue, delta snapshots against acked baselines,
  reliable events, prediction of one's own Arisen with reconciliation,
  100 ms interpolation of everyone else, latency, and the per-player
  versus shared table.

Every networking choice is labelled a decision. COOP_DESIGN.md gained a
host section and proposals for the open questions on saves and pause.

### 4. `crates/ddhost`

- `Host<D: Data, W: StoryWorld>` owns roster, party, `ddsim::Sim`,
  `ddstory::Story`, the world adapter (`StoryWorld`: ddstory's `Host`
  queries plus `handle` and `events`), ground and locks.
- `Host::tick(&FrameInputs)` runs:
  1. the world's completions;
  2. the story, which sees the last frame's positions; a scenario press
     arrives one frame late, the lower seat first;
  3. the input latch;
  4. one `Sim::step`;
  5. the requests, handed to the world.

  The boot `JumpArea` is sent automatically.
- `state_hash()`: FNV-1a over the `Debug` text (bit-exact floats) of
  every Arisen, the story world, quests, scripts and the request log.
- Also: `save` (DDHS: `WRLD` and per-profile `PLCE`, unknown chunks
  kept), `lock`, `player` (Roster, Party), `clock`, and `NoStoryData`.
- `install::sim(&Vfs)`: one Sim builder for host, ddview and ddaccept
  (head #876). It loads every slot of banks 0 and 6 (149 motions), the
  jump rows and the adjust table.

**Tests:** `cargo test -p ddhost`: 15 tests, all pass.
- `replay.rs`: a two-player scripted session run twice gives an equal
  hash every frame (more than 800 distinct states in 900 frames); one
  changed input diverges exactly from its frame; plus save and load,
  leave releasing locks, the unknown seat refused, and the interact
  latency and seat priority.
- `opening.rs` (install): the real opening through the host twice
  reaches the same state, and the same SceNo ladder as ddstory's own
  test.
- `install.rs`: the install Sim equals ddsim's shipped constants.

### Not done, and next

- Live call-order confirmation (needs a probe command; asked of claude-4,
  #963). The worker-thread count; the pause-menu mask read.
- Composing ddeconomy, ddrift, ddprogress and ddcombat, and their save
  chunks and `PlayerId` types (#962).
- The network layer itself (M3): the ADR's messages, input queue and
  snapshots.
- Pawns in the sim. The story's `pawns` and `enemies` inputs are empty
  so far.
