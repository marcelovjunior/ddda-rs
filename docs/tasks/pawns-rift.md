# pawns-rift: Riftstones, the Rift and recruiting pawns

- **Owner:** claude lane
- **Branch:** `task/pawns-rift`
- **Board area:** `research/pawns`
- **Status:** review

## Goal

Find out exactly how pawn recruitment works, so the rewrite can do it 1:1
and co-op can share the main pawn (`docs/COOP_DESIGN.md`):
- what a Riftstone does;
- how the Rift lists pawns, and where they come from (online server, the
  built-in offline list, friends, the user's own data);
- what hiring costs (Rift Crystals, the level rule);
- how a hired pawn is built from its stored data (appearance, vocation,
  skills, gear, inclinations, knowledge);
- dismissing, returning, rating and gifts;
- what is saved.

This is research. The output is documentation and readers, not gameplay
code.

## Starting points

- **DTI classes** (`E:\ddda-re\notes\dti-labels.csv`): search for `Pawn`,
  `Rift`, `Rim` (the game's word for Rift Crystals may differ),
  `sPawnManager`, `uPawn`, and network/session classes (`sNet`,
  `cNetwork`, `Session`, `Server`).
- **Exe strings:** server URLs and API paths (WININET is imported). Do NOT
  contact any server. Document what the client sends and expects from the
  code only.
- **Data:**
  - pawn-related resources in `nativePC` (offline pawns, rift UI, pawn
    parameters: search `paths_of_type` over `mtarc::RESOURCE_CLASSES`,
    and paths containing `pawn`/`rift`);
  - game text through `mtgmd` (`docs/TEXT.md`; e.g. Rift menu strings,
    inclination names).
- **Save file** (read-only copy only, never the original): back up first,
  then work on the copy in
  `E:\ddda-re\save-backups\<latest>\367500\remote\DDDA.sav`. Find the
  pawn records in it. Its format may be documented in
  `E:\ddda-re\refsrc` (check).
- **Live checks:** only through the oracle agent (board area `live/game`;
  ask its owner, or head, on the board). Never attach to the game
  yourself.
- The Codex lane's `asset-survey` covers items, quests and NPCs broadly.
  Read `docs/SURVEY.md` on its branch when it exists, and coordinate with
  `codex-1` on the board to avoid duplicating work.

## Scope

- `docs/PAWNS.md` (new)
- `crates/ddsave/**` (new, optional): a read-only save-file reader, if
  you get that far, with tests against a copy

## Deliverables

`docs/PAWNS.md`:
- each rule with its exe address, marked traced or guessed;
- the data structures (pawn record layout in memory and in the save);
- the network protocol as far as the client code shows it;
- the offline fallback;
- a proposed implementation plan split into briefs (data readers, rules,
  UI) with "done when" for each.

## Done when

PAWNS.md answers the goal's questions, or says precisely what is still
unknown and how to find it (e.g. "needs a live check of X through the
oracle"). Fill in Results and report to `head`.

## Results

claude-5, branch `task/pawns-rift`. Full write-up: `docs/PAWNS.md`.

**Found** (addresses in PAWNS.md):

- **Offline pawn data.** Everything is in `rom\etc\romNoraPawnData.arc`,
  loaded by `cRomPawnManager` (`0x00796160`):
  - 33 `rRomNoraPawn` level-band files × 900 templates. The bands are at
    `0x0150a384`. Vocations 1/2/3/7/8/9.
  - `rRomPawnInfo`: 100 complete pawns. Records 0..19 are the story stock,
    and slot 14 is Rook.
  - `rRomPawnName`: 440 Latin names, chosen by `0x00cdbf00`.
- **Unique IDs** (`0x00794f10`, `0x00795010`):
  - `0x1LLLIIII`: nora template I of level L's band;
  - `0x2000IIII`: story stock slot or romPawnInfo record;
  - `-1`: network pawn.
- **The Rift pool** is sGameSys `mCmcStock` slots 20..119. It is not saved
  and is refilled on every load.
  - Online: network pawns first. Offline: favourites with built-in IDs
    first.
  - Then `0x007951c0` adds nora pawns at the player's level +
    {0×25, +2×16, −2×2, +4×16, ...} (table `0x0150a358`).
- **Rift menus.** Category enum mapped to the `pawn_search` menu items, the
  list builder `0x0047a550`, row flags, a 5-pawn summon cap and the
  2-hire limit (`0x0073c2c0`).
- **Hiring cost** (`0x0043eec0`): 0 at or below the player's level and for
  friends; otherwise `trunc(m*((L+P)(L-P)^2+18))`, with m = 1.0 offline,
  clamped to 99,999,999.
- **Rift Crystals** are sGameSys+0xa7a1c, saved as `OnlinePoint`.
- **Dismissal and death** (`0x0043e8b0`, `0x0043e9b0` → `0x0043ec60`)
  update past summons (FIFO 100), the close friends (top 4 by time
  together) and the summon history.
- **Ratings** average to 1..5. Gifts, RC and ratings come back only through
  the network (inn, `0x00737900`).
- **Network.** Steam Cloud files shared as UGC plus Steam leaderboards
  (`leader_%d`); no HTTP server. The pawn file is DTI-serialised, zlib,
  SHA-1 and Blowfish. The offline fallback is the nora pool.
- **Save.** A 32-byte header (`0x0040f1a0`): checksum = CRC-32 without the
  final inversion. Then zlib and an XML dump of the reflected save classes.
  The pawn fields are mapped in PAWNS.md §8.

**Built and tested:**

- `crates/ddsave`: a read-only save reader. It covers the header and
  checksum, bounded inflate, a borrowed XML tree, and typed pawn records
  (party, favourites, past summons, lent, close friends, history).
  - 7 unit tests, including synthetic corrupt-input and truncation tests.
  - A private-save test. It skips without a copy and prints no personal
    data.
- `crates/ddpawn`: readers for `rRomNoraPawn`, `rRomPawnInfo` and
  `rRomPawnName`, plus translated rules with tests:
  - hire cost `0x0043eec0`;
  - xorshift128 `0x00d00690`;
  - pool fill `0x007951c0`;
  - level-search draw `0x00794f10`;
  - name lookup `0x00cdbf00`.

**Compared with the original:**

- All 8 nora pawns in a real save equal their templates (appearance,
  vocation, English name), confirming the ID encoding, the bands and the
  name lookup.
- A live oracle read (claude-4, #358) of the 100 pool IDs matches the
  generator's level table for P = 7 exactly, with 1 favourite placed first
  (`live_pool_level_multiset`). The same read confirmed the offline
  multiplier of 1.0 and that RC equals `OnlinePoint`.

**Unknown** (details and how to resolve them in PAWNS.md, Open questions):

- The RNG seed, so a bit-exact pool cannot be reproduced yet.
- The full pawn builder `0x00795610` (stats and skill slotting), which is
  only outlined.
- The gift-refusal rule.
- Where evaluate ratings go.
- The main pawn's party slot.
- The save-version constant.
- Live: the hire cost via "Base Cost", and a new-game pool (load mode 0).

**Next:** the briefs proposed at the end of PAWNS.md. Start with
pawn-builder (rebuild the save's 8 nora pawns field for field), then
rift-sim.
