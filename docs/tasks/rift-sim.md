# rift-sim: The Rift as a host-owned system

- **Owner:** claude-5
- **Branch:** `task/rift-sim`
- **Board area:** `pawns/rift`
- **Status:** review

## Goal

Proposal 3 in `docs/PAWNS.md`: a headless Rift on top of `ddpawn` and the `ddstory` world state, covering pool refill on load, the list builder (with the projection rules in RIFT_LIST/RIFT_UI), summoning (5 in the room), hiring (cost, Rift Crystals, slot choice, the 2-hire limit) and the dismissal bookkeeping (past summons, close friends, history). Every call is scoped by player for co-op (PAWNS.md section 10).

**Shared world state.** `crates/ddstory` (merged) owns the world state and
its save/load. **claude-7 owns `ddstory`**. Build your system in your own
crate on top of it; if you need a ddstory API change, ask claude-7 on the
board instead of editing it. Design for N players, host-owned
(`docs/COOP_DESIGN.md`).

## Scope

- `crates/ddrift/**`
- `crates/ddpawn/**`
- `docs/PAWNS.md`
- this brief's Results

## Done when

Tests drive a full session: load, list, summon, hire at the correct cost, dismiss. Both single-player and two-player cases are covered. Live checks of cost and pool are queued with claude-4 where possible. Fill in Results and report to `head`.

## Results

claude-5, branch `task/rift-sim`. Details are in `docs/PAWNS.md` §11.

**Built.** `crates/ddrift` is a new crate. The host owns one `Rift` per
session: the stock, the shared party slots, the `PawnLog`, and the room
during a visit. Each call names the acting player and takes that player's
`Arisen` (level and Rift Crystals). `RiftData` loads the `ddpawn` tables,
the exe tables through `ddexe`, and the quest-knowledge table
(`0x01521e90`).

Translated, offline semantics:

| Area | Exe source |
|---|---|
| Refill | `0x0047d330`: favourites first, hired favourites skipped, then `0x007951c0`, then the story stock `0x00796420` |
| Lists | `0x0047a550`: by level with the 100-draw duplicate rule; past summons and favourites; network categories return status 6; 0x800 for pawns in the room; the knowledge sort `0x0047af50` / `0x005d0760` against `ddstory::World::active_quest` |
| Room | at most 5 present (`0x0073bae0`) |
| Hire | `0x0073c2c0`: code order 5 / 2 / 1, slot reuse, spend with floor |
| Release and death | `0x0073c480`, `0x0043e9b0` → `0x0043ec60` with `0x00448da0`, `0x00448130`, `0x00448470`, `0x00448500`, `0x00449380` and the history entry `0x0077bf10` |

Co-op:

- Hired slot 2 is reserved for the second Arisen, so a co-op party hires
  one pawn.
- The pool is built at the host's level.
- Costs and payment use the acting player's level and crystals.
- A talk lock keeps a room pawn to one player at a time.

**Tested** (`cargo test -p ddrift`; clippy clean):

- 8 unit tests, including the live cost values.
- 3 session tests against the install:
  - **From the private save:**
    - the refill reproduces the live pool's level multiset;
    - list at level 10 (100 unique rows, cost 171 each);
    - summon 5, with the sixth refused;
    - hire refused for crystals, then for a full party;
    - release Eloyse (favourite totals, past summons, history);
    - hire into the freed slot.
  - **Two players:** per-viewer costs, the lock, the one shared hired
    slot, payment by the hiring player, release then re-hire.
  - **The knowledge sort.**

**Compared with the original:**

- The pool refill against the live pool (#358): the same level multiset,
  with the favourite placed first.
- The hire cost: `0x0043eec0` called live for five (P, L) pairs gives
  exactly the translation's values (claude-4, #879).

**Not modelled / open:**

- Stage placement of stock pawns (`0x0047ddf0`).
- The room's eviction path when all seven slots are full (`0x0073ef10`).
- Taking back a released pawn's items (inventory).
- A save format for the log (it is imported from DDDA.sav); this belongs
  with the host save design.
- Two guessed details of the history entry: which six skills it lists and
  their levels.
- Live and still queued with claude-4: the GUI "Base Cost" in the Rift and
  a new-game pool (load mode 0).

**Next:** rift-ui on top of `ddrift::Row` (codex's per-viewer
projection), and saving the log with the host save.
