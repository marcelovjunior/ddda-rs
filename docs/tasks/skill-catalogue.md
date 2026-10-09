# skill-catalogue: skills and augments read from the installed exe

- **Owner:** claude lane
- **Branch:** `task/skill-catalogue`
- **Board area:** `research/vocations`
- **Status:** review

## Goal

Proposal 2 in `docs/VOCATIONS.md`: a complete catalogue of skills and
augments in `ddskill`. It joins three sources:
- `etc\skill\skill` (costs and ranks);
- the exe's skill table (0x014f3658: vocation and weapon masks, action
  ids);
- the name tables (0x01520278, 0x015214a0) with the game's text.

Following the constants policy in AGENTS.md, the exe tables are read **at
run time from the player's installed DDDA.exe**, like `ddquest::events`
(PE timestamp check, VA-to-file mapping). They are never committed as
constants.

Also map the action ids to the `cPlActWpn*` classes and their motions,
where the exe allows it. claude-8 asked for this.

## Scope

- `crates/ddskill/**`
- `docs/VOCATIONS.md`

## Done when

- Every skill and augment resolves to its name (rebuilt from game text),
  vocations, weapon, cost, rank and action id.
- Install tests skip without the game.
- Fill in Results and report.

## Results

claude-9, branch `task/skill-catalogue`. Details: `docs/VOCATIONS.md`,
"Action ids, action classes and motions" and "Implementation in this repo".

**Built** (`crates/ddskill`):
- `exe::Exe`: read-only PE access to the player's DDDA.exe (PE timestamp
  1513180209 required, VA to file mapping), class names from the `MtDTI`
  constructor calls (`push name; mov ecx, dti; call 0x00cf5780`). Every
  table read is byte-identical in the installed Steam exe and the unpacked
  analysis copy. No exe bytes are committed; only addresses and table
  shapes (with their source addresses) are.
- `catalogue::ExeTables::from_exe`: skill records 0x014f3658 (430),
  text table 0x01520278 (149), augment names 0x015214a0 (115), the action
  tables per weapon category, the per-skill motion lists 0x0163dfc0.
- `catalogue::Catalogue::build`: joins those with `etc\skill\skill` and
  the text (`skillName`, `skill_command`, `abilityName`, `abilityInfo`, any
  language) into 149 skills (levels with names, commands, cost, rank;
  vocations; weapons; action id, class, parameter; motion list) and 78
  augments (name, description, vocation, cost, rank).
- Example `catalogue_dump`.

**Found (traced):** action ids are `group << 24 | weapon << 16 | index`;
group 1 / 2 tables are registered per weapon category by 0x00b610f0 /
0x00b611e0 (records of 0x44 bytes: `MtDTI*` + 16 words). For weapon skills
word 0 is the skill id, read by the class's vf05 (e.g. 0x00b23d50), which
plays the skill's motion list (0x00ac65a0, layers 7 / 9). Exceptions:
Gouge (58) uses `cPlActEnemyClimb`, Arc of Might (109)
`cPlActWpnGSwordChargeAtckWait`, bows share `cPlActWpnBow` /
`cPlActWpnMagicBow`. Motion lists: `uPlayer`+0x4c2c -> 0x0163dfc0,
loaded per active skill by 0x00b5b400.

**Tested:** `cargo test -p ddskill` 16 unit + 9 install tests pass,
clippy clean. Install tests skip without the game or exe. All 149 skills
and 78 augments resolve; names rebuilt from text; every motion list path
exists; 58 weapon skills have a level 3.

**Not done / next:** which motion inside a list each class plays and when
(claude-8); meaning of record word 0 for core-skill and normal-attack
records (looks like a motion id, guessed); the generic `Exe` reader could
be shared with `ddquest::events` and `ddpawn::tables` in a small common
crate (head's call).
