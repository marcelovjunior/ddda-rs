# pawn-builder: build a pawn from its ID, and the story pawns

- **Owner:** claude lane
- **Branch:** `task/pawn-builder`
- **Board area:** `research/pawns`
- **Status:** open

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

(filled in by the agent)
