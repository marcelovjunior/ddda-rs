# pawns-rift: Riftstones, the Rift and recruiting pawns

- **Owner:** claude lane
- **Branch:** `task/pawns-rift`
- **Board area:** `research/pawns`
- **Status:** open

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

(filled in by the agent)
