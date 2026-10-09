# combat-rules: damage, resistances, stagger and hit detection

- **Owner:** claude lane
- **Branch:** `task/combat-rules`
- **Board area:** `research/combat`
- **Status:** review (claude-8, 2026-10-09)

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

Full write-up: `docs/COMBAT.md`. Live: claude-4 confirmed the player
attack inputs (P+0x08 = mBasicAttack, defaults of P+0x48/+0x4C; #758);
the goblin hit measurement is queued until a goblin is present (#698).

**Found (exe addresses, 2017-12-13 Steam build):**
- Hit pipeline: per-frame collision update `0x0076B7E0` (pairs attack and
  hurt volumes, builds `DamageInfo` `0x00774ED0`, merges same-frame hits
  into one `HitInfo` per victim with a hit count at +0xD0, filters
  `mIsSameFrameNoHit00..03`), then `0x0076DB10` (mitigation callback,
  region scaling, HP/DP/BP/status application). `uObjModel::vf05`
  `0x00885C10` wires the callbacks to vtable slots (vf68 = mitigation,
  vf72/vf75 = pool emptied). `cObjCollision` sits at actor+0x10E0.
- Attacker value: enemies `0x0088B530` (stat x `mAttackRate` /
  `mMgcAttackRate`, status multipliers); players/pawns `0x0088AA20`
  (`mAttackRate`*base + weapon*`mWpPhyAtkRate`*crit + `mAttack`, and a
  magick part from the weapon's magick on every hit; critical roll from
  `param\dlc`[0x19..0x1B]); dispatcher `0x0088A910`.
- Enemy mitigation `uEnemy::vf68` `0x00AAA490`: linear,
  `max(0, atk - hits*def)`, element resist on the magick part,
  slash/strike split by `mSwordRate`/`mHitRate` x region resists;
  `mIsNoCalcEnemyDefence` skips defence. Player mitigation
  `uPlayerBase::vf68` `0x00B7AD90` + `0x00B83EE0`: hyperbolic,
  `x*(1 - c*D/(D+100)) - d`, `D = 0.5*equipDef + baseDef`
  (`param\game`[5], [6]).
- Region pools (`cObjCollision::RegionStatus`, ctor `0x00776BA0`, filled
  by `0x00770910` from RST and `0x00770FD0` from the PRP resist array at
  actor+0x59E8): HP `0x00776F50`, DP stagger / BP knockdown `0x00777110`,
  per-frame recovery and weak-point class `0x007781A0`, `mAttr` bits,
  `param\game` class multipliers 1.25/1/0.75/0.4/0.
- Status build-up `0x007772B0` (element statuses from `mElementRate` x
  element resist, others from the attack's `mPoison`.. values; random
  chance, thresholds from the PRP `耐..` values, >= 10000 immune).
- `cCharParamEnemy` (`vf03` `0x0074A410`) is the common PRP class; its
  stats live at actor+0x59B0 (attack +0x59BC .. resists +0x59E8).

**Built and tested:** `crates/ddcombat`: `rObjCollision` reader (groups,
nodes, 83-value attacks with names/offsets, 57 flags, sequence index),
3 synthetic tests + install tests: all 8,629 occurrences (415 paths) parse
to the last byte. Examples `ocl_dump`, `ocl_list`, `rst_dump`, `adj_dump`.

**Worked example:** level-1 Fighter, Iron Sword (50), sword attack 1
(`mAttack` 50) on a goblin (def 75, HP 1000): 180 - 75 = 105 HP, flinch;
goblin attack 0 on an unarmoured Arisen with base defence 80: 111.1 x e,
where e (1, 1.1 or 1.2) is not resolved.

**Unknown:** motion-event activation of hit groups, shape tests,
collision mode +0xD14, crit-capable flag P+0x144, `cStatus` type names
and status durations (`rStatusParam` consumer), stamina-damage consumer,
guard / friendly fire / experience. No live measurement yet.

**Next:** the five briefs proposed in COMBAT.md section 10
(combat-hit-volumes, combat-damage, combat-status, combat-oracle-checks,
player-attack-params).
