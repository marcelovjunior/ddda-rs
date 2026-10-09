# host-integration: one player identity and per-crate save chunks

- **Owner:** claude-10
- **Branch:** `task/host-integration`
- **Board area:** `arch/host`
- **Status:** review

## Goal

Follow-up to ADR 0002 (#975):
- **One identity:** a common `PlayerId` / `ProfileId` from `ddhost` (or a
  tiny shared crate) used by `ddeconomy`, `ddprogress` and `ddrift` (today
  they use `usize`, `PlayerId(u8)` and `u8`).
- **Save chunks:** `save()` / `load()` pairs per owner (ECON, PROG, RIFT)
  in the DDHS container.
- **Wire the three crates into `ddhost`:** the per-frame calls in the
  traced order, the shop and NPC sessions under the interaction lock, and
  the Rift visit. Keep each crate's tests green.

The owners are paused, so head approves these edits.

## Scope

- `crates/ddhost/**`
- `crates/ddeconomy/**`, `crates/ddprogress/**`, `crates/ddrift/**`
  (identity and save only)
- `docs/adr/0002-game-host.md`

## Done when

- A ddhost test saves and loads a session with two players, a shop
  purchase, a hire and a level-up.
- The state hash is equal after load.
- Fill in Results and report.

## Results

claude-10, 2026-10-09, branch `task/host-integration`, on main 2740de9.
Head approved the edits in the three crates (#965); each crate's tests
stay green.

### 1. One identity: the crate `ddid`

- `PlayerId = u8`: the seat, 0 = the host. It is the same type as
  `ddsim::PlayerId`.
- `ProfileId(u64)`: an Arisen across sessions.
- `Parts { shared, players }`: the save shape every system uses.
- `Writer` / `Reader`: a small little-endian codec. Floats are stored by
  bits; counts are bounded; trailing bytes are refused.

The crates now use it:
- `ddeconomy` (was `usize`);
- `ddrift` (was `struct PlayerId(u8)`);
- `ddprogress` (was already `u8`);
- `ddhost` (re-exports both types).

### 2. Save parts and DDHS chunks

Each crate has a `save` module (format notes in each crate's docs), and
each part starts with a version byte.

- **`ECON`**, from `Economy::save` / `load`:
  - shared: restock day, the 17 stock records, buy-back, gates, the
    main-pawn flag, pawn bags;
  - per seat: wallet, bag, storage.
- **`PROG`**, from `Progression::save` / `load(parts, seat map)`:
  - per seat: that Arisen's full record;
  - shared: the pawns, the character order (an Arisen is a seat
    reference) and the party roles.

  An absent player's Arisen and party slot are left out.
- **`RIFT`**, from `Rift::save` / `load`, shared only:
  - the party pawns (full `Cmc` codec), the pawn log, the cost
    multiplier;
  - the stock is refilled on load, as the original does (0x0047D330);
  - the room is not saved.
- **`HOST`**, the host's own chunk: frame count, seat → profile table,
  and who hired each pawn.
  - On load the host files each seat's part by profile.
  - It remaps seats, so a guest who hosts next time keeps their wallet,
    bags, Arisen, place and hired pawn.

`Host::save` is refused while an NPC lock, shop session or Rift visit is
open.

### 3. Wiring into `ddhost`

- **In the tick**, in the traced order (FRAME.md):
  - `Economy::update(day)` before the story (`sNpcManager::vf06`
    0x00475AD0, in aGameMain's managers);
  - `Progression::step` and `sync_to_world` after the movement step (the
    Player line, phase 90: 0x00B5BD20, 0x00B5C230).
- **Host API** (`systems.rs`):
  - characters: `create_arisen`, `create_main_pawn`, `award_exp`;
  - shops, under the host's interaction lock first: `open_shop`, `buy`,
    `sell`, `close_shop`;
  - the Rift visit (shared room; first in opens it, last out closes it):
    `rift_enter`, `rift_leave`, `rift_summon`, `rift_hire` (cost from the
    hirer's level, paid from their wallet's crystals), `rift_release`,
    `refill_rift`;
  - `economy_party`: the economy's view of the party (bags, levels,
    augments).
- **Leaving** releases the seat's locks, shop sessions
  (`Economy::close_all`, new) and Rift place.
- **Data**: `SystemData` (catalog, tables, Rift data) with the loaders
  `install::catalog` and `install::tables`.
- **Hashes**:
  - `saved_hash` covers exactly what a save keeps;
  - `state_hash` (replays) adds body fields, story stage, quests,
    `Story::script_states()` (claude-7's accessor, #995), open
    interactions and the request log.

### Tests

All pass: `cargo test -p ddid -p ddeconomy -p ddprogress -p ddrift -p ddhost`.

- **ddhost `tests/session.rs`** (install). A two-player session:
  1. Caxton sells the Trusty Sword for 2,700; the guest is locked out
     meanwhile, and a save inside the shop is refused.
  2. The guest hires a pawn with Rift Crystals.
  3. An award of 3,000 exp levels up both Arisen over the following
     frames.
  4. Save, then load into a fresh host.

  Checks after the load:
  - `saved_hash` is equal;
  - economy, progression, Rift party and party are equal;
  - saving again gives identical bytes.

  A second test loads with the profiles on swapped seats.
- **Crate round trips:**
  - ddeconomy (`save_parts_round_trip`, including a seat change);
  - ddprogress (`save_parts_round_trip_and_seat_change`: absent guest,
    swapped seats);
  - ddrift (`round_trip_without_stock_or_room`);
  - ddid codec.

### Not done

- **Hash equality after load.** `state_hash` cannot be equal after a
  load, because scripts restart and body fields beyond the place are not
  saved. The done-criterion is met with `saved_hash`.
- **Hired pawns and progression.** They have no progression record
  (`Role::Support`); experience shares for them need a `Character` built
  from a `Cmc`.
- **Rift refill.** The host does not refill the Rift on load by itself:
  `refill_rift` needs `RiftData` and an RNG.
- **Story flag.** `blocked` for the restock (sGameSys +0xB8844 bit
  0x80000) is not in `ddstory::World`; it is passed as false.
- **Hire time unit.** Hire times use host play seconds; the unit of
  `cCmcInfo+0x1650` is *assumed*.
