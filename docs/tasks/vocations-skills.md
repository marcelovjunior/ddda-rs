# vocations-skills: vocations, skills, augments and level growth

- **Owner:** claude lane
- **Branch:** `task/vocations-skills`
- **Board area:** `research/vocations`
- **Status:** review

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

claude-9, branch `task/vocations-skills`. Full findings: `docs/VOCATIONS.md`.

**Found** (static analysis of DDDA.exe + install data; **live** where
marked, from claude-4's read-only oracle, board #722):

- Vocation ids 1..9 = `job_1`..`job_9` labels (Fighter, Strider, Mage,
  Mystic Knight, Assassin, Magick Archer, Warrior, Ranger, Sorcerer).
  Pawns: 1, 2, 3, 7, 8, 9 only (0x01517a3c, 0x01517a54).
- `rSkillList` (`etc\skill\skill`, `skl\0` v0x200, loader 0x00cdec00):
  18 core skills, 131 weapon skills x 2 levels, 78 augments with discipline
  cost and rank. Field order from the consumers (0x006fd940, 0x006fd7e0,
  0x006fe050), not from the DTI offsets.
- Exe tables: skill records 0x014f3658 (430 x 0x20: action id, vocation
  mask, weapon mask, type, text index), text table 0x01520278 (name /
  command message per level; level 3 present or not), augment-name table
  0x015214a0, weapon-category tables 0x014f6c18/4c/80/b4, stamina path table
  0x0163e62c, motion banks 0x0163df8c, vocation weapon lists 0x0163e784.
- Level-up 0x00b5bd20 (growth of the vocation held at that moment, HP cap
  9999; **live**: level-7 Mage Arisen and Fighter pawn stats match),
  next-exp 0x00b5c080 and total 0x00b5c150 (**live**: 17 values match,
  x87 at double precision; the level-200 constant 0x022f0778 is 786 above
  the curve's sum), exp award 0x00782d10, exp to discipline 0x00782f30
  (bands 0x014f24a0; **live**: 7 values match), rank-up 0x00b5c230
  (**live**: thresholds match), party
  distribution 0x00443470 / 0x004437d0 (pawn catch-up, support pawns get
  10 % as +0x34c).
- Vocation change `uGUIJobChange`: offered list 0x00675780 (3 basic below
  level 10 or on the free first choice; 9 / 6 from level 10), cost 1000 /
  1500 discipline once per vocation per character (0x01517a7c, flags
  540..554), apply 0x00675da0 (clears active skills, filters skill sets,
  keeps level/stats/ranks/augments/learned skills), starting items on the
  first choice (0x00673740).
- Learn rules, augment value lookup 0x00783340 (equipped: mFVal0, else
  mFVal1), weapon-skill stamina rows 0x00b60e20 / 0x00b60f70 with
  Proficiency, level-3 skills granted by equipment (0x00442af0 /
  0x00442d50, item flag bit 29).

**Built:** `crates/ddskill` (reader + translated rules, each with its
address). `cargo test -p ddskill`: 14 unit + 5 install tests pass (both
rSkillList copies re-encode byte-identically; all nine vocations to level
200; live party stats and curve values; names from text); clippy clean.
Commits on `task/vocations-skills` (see `git log`). `Cargo.lock` changed for
the new crate.

**Unknown / next:** live checks that change game state (level-up order,
party shares, pawn catch-up, vocation change, learning); unlock of augments
100..114 (save bytes sGameSys+0xbe1b4);
which items grant level-3 skills or scale discipline (0x361, 0x47f);
parameter objects at sGameSys+0xb8754 / +0xb875c; action id to
`cPlActWpn*` class and motion; magick and bow skill stamina. PARAMS.md's
"guessed" rows for `rPlLevel`, `rPlExp`, `rPlJobLevel` are now traced
(VOCATIONS.md); also ANIMATION.md's "m0004_co = dagger" guess is
contradicted by 0x0163df8c (m0004 = unarmed, m0005 = dagger). Proposed
briefs are at the end of VOCATIONS.md.
