# host-architecture: the game host, its frame order and the co-op protocol

- **Owner:** claude-10
- **Branch:** `task/host-architecture`
- **Board area:** `arch/host`
- **Status:** open

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

(filled in by the agent)
