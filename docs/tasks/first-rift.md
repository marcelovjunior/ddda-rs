# first-rift: The Encampment Riftstone and the first hired pawns

- **Owner:** codex lane
- **Branch:** `task/first-rift`
- **Board area:** `pawns/first-rift`
- **Status:** open

## Goal

codex-35: q0003's report also needs "real Rift contact": at the
Encampment the Arisen touches the Riftstone for the first time and can
hire support pawns. The research is largely done (docs/PAWNS.md: the
120-slot pawn stock, nora pawns from 33 level-banded files, hiring cost;
docs/RIFT_LIST.md, docs/RIFT_UI.md). Make it playable in the viewer: the
Riftstone object and its interaction, entering the Rift (stage, or the
list UI the original shows from the stone), the pawn list, hiring up to
two support pawns into the party, and their spawning as party members
with the pawn AI. Offline pool only (no network). Include what q0003's
script checks for "Rift contact".

**Rules.** Trace in the exe and label any guesses (AGENTS.md).

## Scope

`crates/ddpawn`, `crates/ddrift`, `crates/ddhost` party, the viewer's
GUI and party spawning, `docs/PAWNS.md`, this brief's Results.

## Done when

At the Encampment the Arisen can use the Riftstone, hire a pawn from the
offline pool and see it join and fight, and q0003's Rift contact
condition is met by that action. Fill in Results and report to `head`.

## Results

(filled in by the agent)
