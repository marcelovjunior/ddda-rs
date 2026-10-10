# cyclops-runtime: The Encampment cyclops fight (q0003)

- **Owner:** codex lane
- **Branch:** `task/cyclops-runtime`
- **Board area:** `fight/cyclops`
- **Status:** open

## Goal

codex-35 (road-to-encampment): q0003 at the Encampment is not just an
arrival. Event st210 ev00 is the *post-cyclops report*: its script waits
for SceNo 1800, the event ends at 2000, and bit 12 leads to 2100. Its
EnemyCheck targets kind 2 / group 20 / index 0, the Encampment's cyclops.
So the opening slice's last step needs the cyclops: its spawn (GPL
group 20 at stage 210 / the field cell), its AI and attacks through the
generic enemy runtime (ddenemyrt, like the goblins and the chimera), its
death reporting to the EnemyCheck, the guards and pawns fighting it, and
the clinging/weak-point rules the original applies to cyclopes.

Build the system, not the step: the cyclops family as a runtime enemy
usable anywhere it appears.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`. Test runs use
`--unattended`.

## Scope

`crates/ddenemyrt` (new cyclops module), `crates/ddai` if its think
tables need it, `crates/ddhost` enemy feed, the viewer's enemy drawing,
`docs/ENEMIES.md`, this brief's Results.

## Done when

In an `--opening` run at the Encampment the cyclops spawns where the
original puts it, fights the party with its original moves, and its
death drives q0003's EnemyCheck to SceNo 1800 and st210 ev00. Fill in
Results and report to `head`.

## Results

(filled in by the agent)
