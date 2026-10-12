# L40: co-op on the 1:1 rebuild (where the original's code has to change)

- **Owner:** claude-7
- **Branch:** `task/L40-coop-map` (worktree `E:\ddda-rs-wt\L40`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md). Research only, CPU-light.

## Why

The project's goal is two-player LAN co-op (`docs/COOP_DESIGN.md`):
- two full Arisen, a shared main pawn plus one hired pawn;
- host-authoritative simulation, NPC interaction locks, shared game over.

Those decisions predate the static-recompilation rebuild. The rebuild runs
the original's own single-player code 1:1. Co-op must therefore be added
as small, deliberate changes on top of verified functions, and the
places where the original assumes one player decide how big that is.

## Task

1. **Find the single-player assumptions** in the original. Use Ghidra,
   the lifted sources, FRAME.md and the `docs/` topic files (PAWNS,
   PAWN_AI, CAMERA, SAVE_PAWNS, ...). Look for:
   - `sPlayerManager` and every "the player" accessor, and who calls them
     (how many call sites, by system);
   - the party structure (four slots: Arisen, main pawn, two hired) and
     how code finds the Arisen (slot 0? a flag?);
   - the camera, input (`sPad`/`sKeyboard`) and GUI ownership: one of
     each, global;
   - events and cutscenes that take control of "the" player;
   - the save: one Arisen's data, and how the main pawn and hired pawns
     serialise;
   - pawn AI that reads the Arisen (follow, orders, knowledge).
2. **For each assumption,** propose the smallest change that gives two
   players:
   - add a second instance;
   - parameterise by seat;
   - route through a host-side wrapper;
   - or leave it single (for example, cutscenes control both).
   Estimate the number of functions touched and whether they are hot or
   cold.
3. **The host/client split on the 1:1 runtime:** which state the client
   needs replicated (units, effects, GUI), where the natural sync points
   are (the frame order in FRAME.md, sUnit's lines and passes), and what
   the client runs locally (camera, GUI, audio, effects?). Note the
   determinism constraints: job threads, timing, random number
   generators (L24's MtRandom).
4. **Write `docs/COOP_ON_REBUILD.md`:** the assumption inventory with
   addresses, the change proposal per item, and a staged plan. Make the
   first stage the smallest playable co-op: two Arisen in the same area,
   host-simulated, client seeing and moving its own character.
   Distinguish facts from proposals.

## Results

(open)
