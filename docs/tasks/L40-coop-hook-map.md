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

Done by claude-7: [COOP_ON_REBUILD.md](../COOP_ON_REBUILD.md). Research
only; nothing built or run. Evidence: Ghidra decompiles (read-only) and
static scans of the lifted sources (method in its section 1.1).

**Found [exe]:**
- "The player" is one pointer, `sPlayerManager+0x99C`
  (`*0x018FA4EC`). The party is four pointers `+0x99C..+0x9A8`, with the
  index accessor `0x0047EC30`.
- The stage set-up `0x00501020` creates the Arisen and the pawns, and
  stores each pawn's index (`+0x5854`) and master (`+0x585C`). The
  per-frame party upkeep is `0x0043D2B0`.
- The Arisen's input is a per-unit path (`uPlayerBase::vf200`
  `0x00B6AEA0`). It merges all four local pads and the keyboard, and
  rotates the stick by the global camera (`0x00B51EE0`). Pawns fill the
  same fields from their AI pad.
- Pawn follow (`0x00585D00`) reads slot 0, not the master pointer.
- Enemies request camera work through `0x004219A0` (59 callers).

**Counts [scan]:**

| Global | Readers |
|---|---:|
| `sPlayerManager` | 606 (284 read the Arisen; 7 name the last slot) |
| `sCamera` | 380 |
| `sQlocInput` | 131 |
| `sMouse` | 104 |
| `sKeyboard` | 22 |
| `sSave` | 218 |
| the Arisen's record `sGameSys+0xA76D0`, by fixed offset | 175 (GUI 96) |

Each is broken down by system in the doc. At least 38 functions
compare a unit with the Arisen (identity).

**Decisions (proposals, recorded in the doc):**
- Each machine sees its own Arisen in slot 0. P2 is a `uPlayer` in slot
  3 on the host (the slot COOP_DESIGN gives P2); the host's Arisen is in
  slot 3 on the client. So camera, HUD, menus, input and the save record
  stay single per machine.
- The host's simulation gets a seat-relative slot 0 through one lifter
  rule over the 284 readers. The current unit is a thread-local set by
  the pass walkers.
- A remote pad goes through `vf200`. The snapshot is taken at step 54
  (`sNetworkExt`).
- Host-authoritative only: unit jobs, the shared `MtRandom` at
  `0x018D017C` and the QPC timing rule out lockstep.
- Stage 1 (two Arisen, one calm area): about 6 adapters, all cold
  except `vf200`.

**Open:**
- the setter of `uPlayerBase+0x3348`;
- the size of the Arisen record;
- whether a replica's pose updates with its move suppressed;
- whether `uPlayer` is as large as `uCmc`;
- the per-site review of the 128 unattributed readers.
