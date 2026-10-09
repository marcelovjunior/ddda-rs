# pawn-builder: build a pawn from its ID, and the story pawns

- **Owner:** claude lane
- **Branch:** `task/pawn-builder`
- **Board area:** `research/pawns`
- **Status:** review

## Goal

Translate the code that turns a pawn ID into a complete pawn record, so
the Rift can offer exactly the pawns the original offers. Proposals 1 and
2 in `docs/PAWNS.md`:
- **pawn-builder:** `0x00795610` and `0x00796080`, i.e. stats, skill
  slotting, equipment and inclinations from a nora template plus the
  level;
- **story-stock:** `0x00796420`, the 20 story pawns (slot 14 is Rook).

Also, if reachable: the RNG seed (so the pool can be reproduced bit for
bit), and the main pawn's party slot.

## Evidence

- `docs/PAWNS.md`.
- `crates/ddpawn` and `crates/ddsave`, both merged.
- The private save copies in `E:\ddda-re\save-backups`.
- Live reads through claude-4's oracle.

## Scope

- `crates/ddpawn/**`
- `crates/ddsave/**` (typed fields it needs)
- `docs/PAWNS.md`

## Done when

- The 8 generated pawns in the save rebuild field for field from their
  IDs and the player's level.
- Slot 14 rebuilds as the saved Rook record.
- Each translation has its source comment and a test.
- Fill in Results and report to `head`.

## Results

claude-5, branch `task/pawn-builder`. Details are in `docs/PAWNS.md` §1,
§3, §4 and §9.

**Built and tested** (`crates/ddpawn`):

- `builder::build_nora` translates `0x00795610`, with:
  - weapon-skill slotting `0x00796080`;
  - equipment `0x00786b90`;
  - built-in item skills `0x0045e130`, with seeding `0x00d006c0`;
  - inclinations `0x00ca7220`;
  - scene talk `0x00ca7320`;
  - knowledge `0x00ca7670`.
- `builder::build_story_stock` translates `0x00796420`:
  - the copy from `romPawnInfo`;
  - localised names;
  - the 20-byte appearance overrides for slots 10..16 and their pawn type;
  - NPC-ledger names and the fixed equipment of slots 14..16.
- `cmc::Cmc` is the record type. It reads from the save
  (`from_save`) and from XFS (`from_xfs`), and compares field by field
  (`fields`/`diff`).
- New readers:
  - `rSkillList` (`0x00cdec00`);
  - `AI\preset_charainfo` (PCIF);
  - `AI\preset_study` (PCST);
  - item-list slots.
- The skill vocation masks (`.rdata 0x014f3660`) and the item skill table
  (`0x01539f78`) are read at run time from the player's own `DDDA.exe`.
  Its `.rdata` is not encrypted, so no exe bytes are committed.
- Tests: 8 unit tests and 6 install/save tests.

**Compared with the original:**

- Every pawn record the save holds in full was rebuilt from its ID and
  matches field for field, apart from documented post-build fields:
  - 5 nora records (3 distinct generated pawns);
  - Rook (story slot 14) twice;
  - 4 summon-history entries.
- The post-build fields are listed in PAWNS §1: current health, stamina
  base recomputed from the body class at spawn (`0x00b81120`), knowledge
  growth, the hire's item flag 0x80, inclination status, Rook's pawn type
  and the hire time.
- `mUseSceneTalk` values match the save as unit-test constants.
- Live (claude-4, #724):
  - `0x00d006c0` called with seeds 0x1105 and 0 gives exactly the
    translation's state (unit test);
  - the main pawn is in party slot 0 and a hired pawn in slot 1;
  - `sItemManager+0x5b00` reads 0.
- All 29,700 templates build at both ends of their level band.

**Also resolved:**

- **Main pawn slot.** Party pawns are `cCmcInfo` records at
  sGameSys+0xa7ec0 + slot × 0x1660: slot 0 is the main pawn, 1-2 are hired
  (item owner ids 2 and 3 compared, slots confirmed live). The Arisen is
  at +0xa76d0.
  `cCmcInfo+0x7f4` is the pawn type; 2 keeps story pawns in the party.
- **RNG seed.** The global generator `0x018d017c` is seeded once with
  0x1105 by a static initialiser (`0x0137e5c0`). It is shared by about 400
  call sites, so the pool depends on the session's history; a bit-exact
  pool needs a live read of the state before the fill.
- **GUI filtering.** Offline filtering of the Rift list is GUI-side
  (codex-15's `0x006d4bb0`); PAWNS §4 is corrected.

**Not resolved:**

- Who writes `mInfo[k].mStatus` and who clears a story pawn's pawn type.
- The body class behind the spawned stamina base.
- What sets `sItemManager+0x5b00` (read live as 0).

**Next:** rift-sim (PAWNS proposal 3). It can now build every pawn the
offline Rift offers.
