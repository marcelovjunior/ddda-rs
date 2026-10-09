# combat-rules: damage, resistances, stagger and hit detection

- **Owner:** claude lane
- **Branch:** `task/combat-rules`
- **Board area:** `research/combat`
- **Status:** open

## Goal

Find the rules of a hit, from the swing to the number, so M4 (Fight) can
be built 1:1:
- how an attack's hit volumes are placed per motion frame (`rObjCollision`
  attack shapes, which bones and frames);
- how a hit is resolved: the damage formula from attack, defence,
  elements and resistances; magick versus physical; the enemy body part
  (`rRegionStatus` regions); criticals and weak points;
- stagger, knockdown and the stamina damage of a hit;
- how status effects are applied (the `rStatusParam` consumers).

This is research. The output is documentation and, optionally, typed
readers. No gameplay code.

## Evidence

- `docs/ENEMIES.md` and `docs/PARAMS.md` (merged readers: `mtprp`,
  `ddenemy`, `ddparam`).
- `docs/SURVEY.md` (status effects, effects).
- Riftstone notes on `rObjCollision` (`E:\ddda-re\refsrc\riftstone_formats.md`,
  MIT; verify everything).
- Reflected field names: `research/dti/dti_props.py show <Class>` (e.g.
  `rObjCollision::Attack`, which carries `mStaminaAttack`).
- **Exe:** DTI classes for damage, attack, hit, region, status (`dti-labels.csv`).
- **Live confirmation:** through claude-4's oracle on the board (for
  example "hit a goblin with X, read the HP before and after"). Never
  touch the game yourself.

## Scope

- `docs/COMBAT.md` (new)
- `crates/ddcombat/**` (new, optional: readers only, with install tests)

## Done when

COMBAT.md gives the damage formula and the hit pipeline with exe
addresses, or names precisely what is still unknown. It includes one
worked example (a specific attack on a goblin) with the numbers, and a
proposed implementation plan split into briefs. Fill in Results and
report to `head`.

## Results

(filled in by the agent)
