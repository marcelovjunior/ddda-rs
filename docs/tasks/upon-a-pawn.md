# upon-a-pawn: q0002 "Upon a Pawn": the first pawn and the goblins

- **Owner:** codex lane
- **Branch:** `task/upon-a-pawn`
- **Board area:** `story/q0002`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Trace and run q0002 from its boot (GSF 381): its scripts, QCT, events and NPCs. That covers the first pawn, the goblins harassing a woman, and recruitment into the party. The goblins use the existing runtime. The maintainer reports goblins "looked weird" in play: compare their look (shading, skinning, materials) against an original capture at the same time of day, and fix any defect or record why it differs.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddstory/**`
- `crates/ddhost/**`
- `crates/ddpawn/**`
- `crates/ddview/src/opening.rs` (claim)
- `docs/QUESTS.md`
- `docs/PAWNS.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

q0002 plays from its boot to its clear in `--opening`, the first pawn joins the party, and the state matches the original's. Fill in Results and report to `head`.

## Results

(filled in by the agent)

### Research checkpoint (2026-10-09)

- Installed `quest\q0002_b00` checks stage-100 enemy group 23,
  indices 0, 1 and 2, and NPC group 4 index 0. Its cleanup disables
  stage-100 layout flags 3, 4 and 7, removes NPC group 4 and enemy
  groups 23/24, clears q0002 bit 51 and raises SceNo to 1450.
- The QCT has seven rows. NPC 304 death cleanup uses judgment 18,
  selector 0/1, and only disables layout flag 100:7 after leaving
  stage 100. Implemented generic NPC death predicates 18/19 from
  `0x0078ece0`, getters `0x00435710`/`0x00435740`; two tests pass,
  including the installed q0002 table. No fake enemy deaths are supplied.
- Rook recruitment traced to event preparation `0x0050c290`, row 23,
  calling `0x00440190` before st200 ev15. Details in PAWNS.md.
- Still open: real encounter integration, field traversal, first-pawn
  dialogue, pawn party/body setup and original goblin visual comparison.
  The brief is not complete and q0002 is not yet playable through clear.
