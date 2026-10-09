# vocations-skills: vocations, skills, augments and level growth

- **Owner:** claude lane
- **Branch:** `task/vocations-skills`
- **Board area:** `research/vocations`
- **Status:** open

## Goal

Document the class system 1:1:
- the nine vocations and their weapons;
- core and weapon skills, with their motions (banks `m0001`..`m0010`),
  costs, stamina use and learning costs (discipline);
- augments;
- stat growth per level per vocation (`rPlLevel` / `rPlJobLevel`);
- how changing vocation works (where, what changes, what carries over).

Pawns are restricted to some vocations: note which.

## Evidence

- `docs/PARAMS.md` (`ddparam` reads the level and job-level tables).
- Motion banks and weapon groups: `docs/ANIMATION.md`, `docs/MOVEMENT.md`.
- Skill and augment names and descriptions come from the game's text
  through `mtgmd` (`docs/TEXT.md`); search the message files.
- Skill data resources: find them with `ddtool survey` and
  `paths_of_type`.
- Reflected field names: `research/dti/dti_props.py show <Class>`.
- Exe classes (`dti-labels.csv`): `Skill`, `Job`, `Ability`, `Augment`.
- **Live confirmation:** through claude-4's oracle on the board. Never
  touch the game yourself.

## Scope

- `docs/VOCATIONS.md` (new)
- `crates/ddskill/**` (new, optional: readers with install tests)

## Done when

VOCATIONS.md lists every vocation, skill and augment from the game's own
data and text, with costs and learning rules, the stat growth tables, and
the vocation-change rules with exe addresses. Plus a proposed
implementation plan. Fill in Results and report to `head`.

## Results

(filled in by the agent)
