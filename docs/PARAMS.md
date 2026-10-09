# Player parameters

Evidence: Steam executable 2017-12-13 (identity in METHODOLOGY.md), read-only
Ghidra decompilation **and disassembly**, and the installed resource bytes.
No live comparison was performed for this brief.

## Implementation and verification

`ddparam` provides `Stamina`, `Weight`, `Level`, `Exp`, `JobLevel`,
`Ability`, `Defend`, and `NeckPos`, each with `from_bytes`, typed ordered
`entries`, and the resource `quality` flag. `Parameter::from_bytes` dispatches
by resource class. Wrong classes, field types, missing/extra/duplicate fields
and null array elements are errors. Original XFS structural validation is
delegated to `mtxfs`. Float values and all four Vector3 storage lanes are
preserved; semantic ranges are not guessed.

`cargo test -p ddparam -- --nocapture`: 12 passing tests, including one
value-checked install test per type and three synthetic malformed/schema tests.
Install tests use `DDDA_NATIVE_PC`, defaulting to the maintainer's nativePC.
They return with a SKIP diagnostic when that directory is absent; an existing
but broken install fails. The exhaustive test reads every archive copy.

| Resource | Distinct paths | Archive entries parsed |
|---|---:|---:|
| rPlStamina | 33 | 987 |
| rPlWeight | 6 | 6 |
| rPlLevel | 9 | 15 |
| rPlExp | 1 | 1 |
| rPlJobLevel | 9 | 9 |
| rPlAbilityParam | 1 | 1 |
| rPlDefendParam | 1 | 1 |
| rPlNeckPos | 1 | 1 |
| Total | 61 | 1021 |

Zero parse failures. The brief's 31 stamina paths was an undercount.
The typed CLI is `ddtool param-dump <nativePC> <resource-path>`.
It reads the first copy in sorted archive order, matching VFS policy.
Engine precedence among differing duplicate copies remains unresolved.

## Field dictionary

All names/storage types below are **observed in data**. Meaning is explicitly
**traced** or **guessed/unresolved**, independently of successful parsing.

| Record / field | Storage | Meaning and evidence |
|---|---|---|
| Every root: mQuality | u32 | Observed 2; resource quality metadata, exact semantics unresolved; do not use as gameplay tier |
| Every root: mArray | ClassRef array | Ordered child records; loader 0x0047e2b0 and consumers below use indexed pointer arrays |
| cPlStamina.mStamina | f32 | **Traced:** amount term; normal recovery numerator at 0x00b8186c; continuous drain numerator at 0x00b81b60 / 0x00b81c22 |
| cPlStamina.mFrame | f32 | **Traced:** rate divisor at 0x00b81867/71, 0x00b81b79; not a countdown implementation |
| cPlStamina.mMotSpd | f32 | **Traced:** action motion-speed multiplier copied to player+0xee4 by 0x00b81920 |
| cPlWeight.mWeight | f32 | **Traced:** body category boundaries / encumbrance table values at 0x00b81120 and 0x00b81270; kg naming remains an inference from customization |
| cPlLevel.mLevel | u16 | **Guessed:** level-band start; entries 0,1,2,11,101,201; consuming interpolation/growth rule untraced |
| cPlLevel.mHPMax, mStaminaMax | u16 each | **Guessed:** initial/max-health and stamina growth terms; initial versus increment semantics must be traced before use |
| cPlLevel.mAttack, mDefend, mMgcAttack, mMgcDefend | u16 each | **Guessed:** physical/magic attack/defense growth terms; formulas untraced |
| cPlLevel.mNextExp | u32 | **Guessed:** experience term; observed zero throughout the first-copy tables |
| cPlExp.mLv | u32 | **Unresolved:** experience curve control; first entries 200,220 precede 2,6,...; cannot treat as sorted level thresholds |
| cPlExp.mRate | f32 | **Guessed:** experience rate; consumer untraced |
| cPlJobLevel.mLevel | u32 | **Guessed:** vocation rank; observed 0..10 |
| cPlJobLevel.mNextExp | u32 | **Guessed:** experience required for next rank, cumulative versus incremental untraced |
| cPlAbilityParam.mFVal0, mFVal1 | f32 each | **Unresolved:** ability-indexed numeric slots; keep neutral names |
| cPlAbilityParam.mSVal0, mSVal1 | s32 each | **Unresolved:** signed ability slots, including -1 sentinel |
| cPlDefendParam.mShrinkMax, mShrinkSpeed | f32 each | **Guessed:** stagger threshold/recovery; untraced |
| cPlDefendParam.mBlowMax, mBlowSpeed | f32 each | **Guessed:** knockdown threshold/recovery; untraced |
| cPlNeckPos.mPos | Vector3: four f32 lanes | **Guessed:** neck offset; coordinate frame/units and row identities untraced |

## Loader map and vocation IDs

**Traced** 0x0047e2b0 loads resources into the manager at 0x018fa4ec.
0x00b5bc40 retains the weight/stamina resources on each player independently.
No global-player assumption is needed by the readers or future simulation.

| Manager offset | Player offset | Resource |
|---|---|---|
| +0x8f8 | +0x34d0 | PlEditWeight |
| +0x8fc..+0x90c | +0x34d4..+0x34e4 | PlTotalWeight SS,S,M,L,LL |
| +0x910 | +0x34e8 | PlStaminaMax |
| +0x914..+0x920 | +0x34ec..+0x34f8 | RecoverNormal, RecoverLiftS/M/L |
| +0x924..+0x96c | +0x34fc..+0x3544 | 19 action tables, in order below |

Action table order: Walk, Run, Dash, EnemyClimb, EnemyClimbAttack,
EnemyClimbJump, LiftS, LiftM, LiftL, LiftSWait, LiftMWait, LiftLWait,
Hagaijime, DashJump, WalkAssassin, RunAssassin, DashAssassin,
LiftMAssassin, LiftLAssassin. This order is established by the loader's
disassembly; its decompiler aliases overlapping stack locals.

Level/job arrays use IDs 0..9: Fighter, Fighter, Strider, MagicUser,
Paladin, Assassin, Trickster, Battler, Hunter, Wizard. **Traced loader
ordering** at 0x0047e2b0, not a claim about localized vocation names.

## Body size and encumbrance

**Traced** 0x00b81120 stores:
body weight at player+0x34c0; total weight at +0x34c4; body class at +0x34c8;
encumbrance class at +0x34cc. Body weight comes from player+0x3dec vfunc+0x18,
with 76.0 fallback. Total weight comes from player vfunc+0x1ac
(0x00b7e560): body plus inventory and an additional helper contribution
(0x00b7e810, meaning unresolved).

Body class compares against PlEditWeight rows 1..4: equality belongs to
the higher class, yielding 0..4 (SS..LL). Missing table falls back to M.
The shipped boundaries are 50,70,90,110, with row 0 equal to 24.

**Traced** 0x00b81270 selects PlTotalWeight[body class]. For each boundary
row k=1..4, the effective threshold is:

`table[k].weight + capacity_adjustment + (body_weight - PlEditWeight[body_class].weight)`

The capacity adjustment is the SSE return of 0x00b7e710, which consults
ability data and adjust-parameter IDs 0x1eb/0x1ec; its complete semantic
mapping is unresolved. The selected class is 1 below the first boundary,
2/3/4 at successive boundaries, and 5 at or above row 4. Missing table
returns 3. Row 5 is **not used** by this classifier; class 0 can be reached
by later action-specific adjustments. UI names such as Very Light are
not assigned by this reader.

Weight affects stamina costs, recovery denominator and action motion speed
through row indices. It does not by itself explain player+0xda8.

## Stamina rates and action rules

**Traced** 0x00b81450 calls recovery (0x00b81810) and continuous drain
(0x00b81920), then applies recovery first and drain second through
0x00b81e30. Both rates are multiplied by:

`*(0x018fa524)+0x24 times *(0x018fa524)+0x20 times *(0x018d0b28)+0x70`

Here each expression denotes a field of the object pointed to by that global.
This is a **scaled-update rate**, not evidence for subtracting once per render
frame. The exact conversion of this product to seconds is **unresolved**.
The existing movement work's assumed 30-Hz frame unit does not verify
stamina's wall-clock rate. No per-second numbers are asserted here.

Normal recovery uses RecoverNormal only in 0x00b81810:

`recovery = rows[body_class].stamina / (rows[body_class].frame + rows[4 + encumbrance_class].frame) * player[0x2b44]`

Thus rows 0..4 carry body-class values; rows 5..9 carry encumbrance
adjustments. It is not six parallel recovery rates. A flags-OR at offsets
0x854,0x5d4,0x494 with mask 0x20000000 suppresses recovery. Actions
0x02060000..0x02060002 with +0x38b0 != -1 also suppress it.
Actions 5/0x7d can bypass those suppressors under virtual-result bit 0x20
or manager+0xb8844 bit 0x400000. Pending recovery debt at +0x4ce4
suppresses it unless +0x4cec is nonzero. These flag meanings are unresolved.

**Traced** continuous costs and motion-speed lookup, 0x00b81920:

| Action ID | Table / behavior |
|---|---|
| 1 | Walk (WalkAssassin when vocation ID=5): speed only |
| 2 | Run (RunAssassin when ID=5): speed only |
| 5, 0x7d | Dash / DashAssassin: drain = stamina/frame, speed = mMotSpd |
| 0x73 | EnemyClimb cost can already have been accumulated from player+0x273c bit 0x80; action object +0xd0 can suppress it |
| 0x41..0x48 | LiftL / LiftLAssassin |
| 0x53,0x54,0x55,0x57..0x5a | LiftM / LiftMAssassin |
| 0x65,0x66,0x67,0x69..0x6c | LiftS |
| 0x3d / 0x4f / 0x61 | LiftLWait / LiftMWait / LiftSWait |
| 0xa9,0xe3 | Hagaijime |
| Other IDs | No additional cost in this switch (independent one-shot consumers may exist) |

Lift/Hagaijime costs use stamina/frame times the multiplier from
0x00b80a10(0). Lift rows add 6 when ability ID 0x67 is present; those tables
contain two six-row blocks. Walk/run/dash row selection starts at encumbrance,
adds integer-truncated 0x00b80a10(10), and subtracts one (clamped at zero)
for player+0x26a4 bit 0x80. Vocation 5 selects Assassin variants.
Dash additionally multiplies by adjust parameter 0x1db when ability 0x6c
is active. It has zero cost when manager+0xac720 bit 0x10000000 or
player+0x206d bit 4 is set. The former is consistent with the town flag in
MOVEMENT.md; the precise latter role remains unresolved.
Flag 0x20 from vfunc+0x2f8 or manager+0xb8844 bit 0x400000 suppresses
the cost on drain branches.

EnemyClimb's base cost uses the EnemyClimb row, stamina/frame, a
0x00783340(0xb) factor, and vocation-2 adjust parameter 0x1d7. Its exact
action-state gating remains only partially traced.

**Not traced:** one-shot consumers of DashJump, EnemyClimbJump,
EnemyClimbAttack and weapon stamina tables; actual skill-index names;
RecoverLiftS/M/L consumers; meaning of every status/augment modifier.
Their shipped values below are data evidence, not complete action rules.

0x00b81e30 adds the signed result to character-parameter+0x2a8,
clamps to zero/max, sets exhausted at player+0x4ce1 on crossing zero,
and uses a 0.6 max factor for player+0x26a0 bit 0x200.
Positive normal recovery is blocked while exhausted. 0x00b82060 modifies
negative deltas using status/ability and adjust-parameter values; these
modifiers need a separate full trace before a faithful stamina simulation.
0x00b815b0 sets player+0x4c14 when stamina <= adjust parameter 0xf3.
No gameplay simulation was translated or added by this reader brief.

## The +0xDA8 writer

The premise that the exe contains no direct stores to +0xda8 is false.
**Traced disassembly** includes MOVSS stores at 0x00b6b3d6,
0x00b6b477 and 0x00b6b4d1, and writes in 0x0081ba40 / 0x0081c290.
0x00b6b350 reaches the destination through
`player+0x2e54 -> object+4 -> owner`; its default action branch enables
flags +0xf94/+0xf95 and writes the vector from 0x0081c170 to owner+0xda0.

0x0081c170 uses two records selected by signed indices +0x4e and +0x50
in a 0xd0-stride array at +0xc, subject to disabled-record bits.
With +0xf94 enabled, horizontal components (x,z) equal:

`record[index_0x50].float_0x84 * record[index_0x4e].float_0x84 * owner.float_0x64`

The y component starts at 1 and can be divided by owner+0x64 when +0xf95
is enabled. The two horizontal lanes are therefore the same scale term.
The destination's ownership chain and scale formula are traced, but exact
skeletal/customization identities of those records and reproduction of the
recorded 0.8471090 are **not verified**. Special action branches disable
flags and normalize by owner+0x60/+0x64. Do not replace +0xda8 with a
weight-tier constant.

## Shipped values

The following are normalized numeric research tables, not serialized game
resources. They describe the first VFS copy; all copies were parsed above.
Every root has mQuality=2. Index ranges mean consecutive identical records.
For stamina and weight, each cell is an ordered array (zero-based rows);
`value x count` compresses an entirely constant column. Other tables show
each distinct consecutive row explicitly. Values are f32 decimal displays.

### rPlStamina

| File suffix | Rows | mStamina | mFrame | mMotSpd |
|---|---:|---|---|---|
| plstamina_bow | 18 | 0 x 18 | 1 x 18 | 1 x 18 |
| plstamina_bowl | 9 | 0 x 9 | 1 x 9 | 1 x 9 |
| plstamina_dagger | 20 | 180, 140, 120, 140, 100, 140, 140, 80, 100, 120, 100, 180, 120, 140, 100, 140, 180, 100, 400, 160 | 1 x 20 | 1 x 20 |
| plstamina_gsword | 10 | 150, 50, 300, 100, 100, 0, 150, 50, 0, 400 | 1 x 10 | 1 x 10 |
| plstamina_shield | 9 | 100, 0, 0, 50, 100, 0, 50, 100, 150 | 1 x 9 | 1 x 9 |
| plstamina_shieldmg | 15 | 0 x 15 | 1 x 15 | 1 x 15 |
| plstamina_sword | 20 | 100, 80, 300, 120, 140, 180, 200, 160, 250, 400, 140, 120, 180, 100, 250, 120, 180, 140, 180, 160 | 1 x 20 | 1 x 20 |
| plstamina_unarm | 1 | 0 x 1 | 1 x 1 | 1 x 1 |
| plstamina_wand | 30 | 0 x 30 | 1 x 30 | 1 x 30 |
| plstaminadash | 6 | 1.25, 1.5, 1.75, 2, 2.5, 3 | 1 x 6 | 1.2, 1.15, 1.1, 1, 0.9, 0.8 |
| plstaminadashassassin | 6 | 1.25, 1.5, 1.75, 2, 2.5, 3 | 1 x 6 | 1.25, 1.2, 1.15, 1.05, 1, 0.95 |
| plstaminadashjump | 6 | 80, 80, 90, 100, 110, 120 | 1 x 6 | 1 x 6 |
| plstaminaenemyclimb | 6 | 0.4, 0.4, 0.6, 0.8, 1, 1.2 | 2, 1, 1, 1, 1, 1 | 1 x 6 |
| plstaminaenemyclimbattack | 6 | 0 x 6 | 1 x 6 | 1 x 6 |
| plstaminaenemyclimbjump | 6 | 80, 80, 90, 100, 110, 120 | 1 x 6 | 1 x 6 |
| plstaminahagaijime | 6 | 0.4, 1.2, 1, 0.8, 0.6, 0.4 | 2, 1, 1, 1, 1, 1 | 1 x 6 |
| plstaminaliftl | 12 | 0.4, 0.4, 0.6, 0.8, 1, 1.2, 0.4, 0.4, 0.6, 0.8, 1, 1.2 | 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1 | 1.09, 1.06, 1.03, 1, 0.97, 0.94, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2 |
| plstaminaliftlassassin | 12 | 0.4, 0.4, 0.6, 0.8, 1, 1.2, 0.4, 0.4, 0.6, 0.8, 1, 1.2 | 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1 | 1.12, 1.09, 1.06, 1.03, 1, 0.97, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2 |
| plstaminaliftlwait | 12 | 0.2, 0.2, 0.3, 0.4, 0.5, 0.6, 0.2, 0.2, 0.3, 0.4, 0.5, 0.6 | 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1 | 1 x 12 |
| plstaminaliftm | 12 | 0.2, 0.2, 0.3, 0.4, 0.5, 0.6, 0.2, 0.2, 0.3, 0.4, 0.5, 0.6 | 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1 | 1.09, 1.06, 1.03, 1, 0.97, 0.94, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2 |
| plstaminaliftmassassin | 12 | 0.2, 0.2, 0.3, 0.4, 0.5, 0.6, 0.2, 0.2, 0.3, 0.4, 0.5, 0.6 | 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1 | 1.12, 1.09, 1.06, 1.03, 1, 0.97, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2 |
| plstaminaliftmwait | 12 | 0.1, 0.1, 0.15, 0.2, 0.25, 0.3, 0.1, 0.1, 0.15, 0.2, 0.25, 0.3 | 2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1 | 1 x 12 |
| plstaminalifts | 12 | 0 x 12 | 1 x 12 | 1, 1, 1, 1, 1, 1, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2 |
| plstaminaliftswait | 12 | 0 x 12 | 1 x 12 | 1 x 12 |
| plstaminamax | 6 | 500, 520, 540, 560, 580, 600 | 0 x 6 | 0 x 6 |
| plstaminarecoverliftl | 10 | 1, 1, 1, 1, 1, 0, 0, 0, 0, 0 | 2, 2, 3, 3, 4, 0, 0, 1, 2, 3 | 1 x 10 |
| plstaminarecoverliftm | 10 | 1, 1, 1, 1, 1, 0, 0, 0, 0, 0 | 2, 2, 3, 3, 4, 0, 0, 1, 2, 3 | 1 x 10 |
| plstaminarecoverlifts | 10 | 1, 1, 1, 1, 1, 0, 0, 0, 0, 0 | 2, 2, 3, 3, 4, 0, 0, 1, 2, 3 | 1 x 10 |
| plstaminarecovernormal | 10 | 1.812, 1.625, 1.438, 1.25, 1.062, 0, 0, 0, 0, 0 | 1, 1, 1, 1, 1, 0, 0, 0, 0.2, 0.4 | 1 x 10 |
| plstaminarun | 6 | 0 x 6 | 1 x 6 | 1.2, 1.15, 1.1, 1, 0.9, 0.8 |
| plstaminarunassassin | 6 | 0 x 6 | 1 x 6 | 1.25, 1.2, 1.15, 1.05, 1, 0.95 |
| plstaminawalk | 6 | 0 x 6 | 1 x 6 | 1.09, 1.06, 1.03, 1, 0.97, 0.94 |
| plstaminawalkassassin | 6 | 0 x 6 | 1 x 6 | 1.12, 1.09, 1.06, 1.03, 1, 0.97 |

### rPlWeight

| File suffix | Rows | mWeight |
|---|---:|---|
| pleditweight | 5 | 24, 50, 70, 90, 110 |
| pltotalweightl | 6 | 0, 110, 130, 145, 155, 165 |
| pltotalweightll | 6 | 0, 130, 155, 180, 195, 210 |
| pltotalweightm | 6 | 0, 85, 95, 115, 125, 135 |
| pltotalweights | 6 | 0, 60, 80, 90, 95, 100 |
| pltotalweightss | 6 | 0, 34, 44, 54, 59, 64 |

### rPlLevel


param\pl\level\lvassassin

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 430 | 0 | 70 | 70 | 70 | 70 | 0 |
| 2 | 2 | 25 | 25 | 3 | 3 | 3 | 2 | 0 |
| 3 | 11 | 22 | 27 | 6 | 2 | 2 | 1 | 0 |
| 4 | 101 | 5 | 15 | 3 | 1 | 0 | 0 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvbattler

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 450 | 0 | 80 | 80 | 60 | 60 | 0 |
| 2 | 2 | 30 | 20 | 4 | 3 | 2 | 2 | 0 |
| 3 | 11 | 40 | 10 | 5 | 3 | 2 | 1 | 0 |
| 4 | 101 | 5 | 15 | 2 | 2 | 0 | 0 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvfighter

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 450 | 0 | 80 | 80 | 60 | 60 | 0 |
| 2 | 2 | 30 | 20 | 4 | 3 | 2 | 2 | 0 |
| 3 | 11 | 37 | 15 | 4 | 4 | 2 | 1 | 0 |
| 4 | 101 | 15 | 5 | 1 | 3 | 0 | 0 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvhunter

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 430 | 0 | 70 | 70 | 70 | 70 | 0 |
| 2 | 2 | 25 | 25 | 3 | 3 | 3 | 2 | 0 |
| 3 | 11 | 21 | 30 | 4 | 2 | 3 | 2 | 0 |
| 4 | 101 | 5 | 15 | 2 | 1 | 0 | 1 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvmagicuser

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 410 | 0 | 60 | 60 | 80 | 80 | 0 |
| 2 | 2 | 22 | 20 | 2 | 3 | 4 | 3 | 0 |
| 3 | 11 | 21 | 10 | 2 | 1 | 4 | 4 | 0 |
| 4 | 101 | 10 | 10 | 0 | 0 | 2 | 2 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvpaladin

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 450 | 0 | 80 | 80 | 60 | 60 | 0 |
| 2 | 2 | 30 | 20 | 4 | 3 | 2 | 2 | 0 |
| 3 | 11 | 30 | 20 | 2 | 3 | 3 | 3 | 0 |
| 4 | 101 | 15 | 5 | 1 | 1 | 1 | 1 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvstrider

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 430 | 0 | 70 | 70 | 70 | 70 | 0 |
| 2 | 2 | 25 | 25 | 3 | 3 | 3 | 2 | 0 |
| 3 | 11 | 25 | 25 | 3 | 3 | 3 | 2 | 0 |
| 4 | 101 | 5 | 15 | 1 | 1 | 1 | 1 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvtrickster

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 410 | 0 | 60 | 60 | 80 | 80 | 0 |
| 2 | 2 | 22 | 20 | 2 | 3 | 4 | 3 | 0 |
| 3 | 11 | 21 | 20 | 2 | 3 | 3 | 4 | 0 |
| 4 | 101 | 10 | 10 | 1 | 0 | 1 | 2 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

param\pl\level\lvwizard

| Row | mLevel | mHPMax | mStaminaMax | mAttack | mDefend | mMgcAttack | mMgcDefend | mNextExp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 | 1 | 410 | 0 | 60 | 60 | 80 | 80 | 0 |
| 2 | 2 | 22 | 20 | 2 | 3 | 4 | 3 | 0 |
| 3 | 11 | 16 | 15 | 2 | 1 | 5 | 5 | 0 |
| 4 | 101 | 10 | 10 | 0 | 0 | 3 | 1 | 0 |
| 5 | 201 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

### rPlExp


param\pl\level\exp

| Row | mLv | mRate |
|---:|---:|---:|
| 0 | 200 | 1 |
| 1 | 220 | 1 |
| 2 | 2 | 1.3 |
| 3 | 6 | 3 |
| 4 | 14 | 5 |
| 5 | 18 | 8 |
| 6 | 25 | 9 |
| 7 | 36 | 10 |
| 8 | 42 | 11 |
| 9 | 51 | 4 |
| 10 | 101 | 15 |
| 11 | 151 | 20 |
| 12 | 200 | 0 |

### rPlJobLevel


param\pl\joblevel\joblvassassin

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 800 |
| 2 | 2 | 1800 |
| 3 | 3 | 3000 |
| 4 | 4 | 4600 |
| 5 | 5 | 6400 |
| 6 | 6 | 8500 |
| 7 | 7 | 11000 |
| 8 | 8 | 14500 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvbattler

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 500 |
| 2 | 2 | 1000 |
| 3 | 3 | 2000 |
| 4 | 4 | 3500 |
| 5 | 5 | 4800 |
| 6 | 6 | 6500 |
| 7 | 7 | 8300 |
| 8 | 8 | 11000 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvfighter

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 500 |
| 2 | 2 | 1000 |
| 3 | 3 | 2000 |
| 4 | 4 | 3000 |
| 5 | 5 | 4000 |
| 6 | 6 | 5000 |
| 7 | 7 | 6000 |
| 8 | 8 | 7500 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvhunter

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 500 |
| 2 | 2 | 1000 |
| 3 | 3 | 2000 |
| 4 | 4 | 3150 |
| 5 | 5 | 4500 |
| 6 | 6 | 6000 |
| 7 | 7 | 7500 |
| 8 | 8 | 9500 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvmagicuser

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 500 |
| 2 | 2 | 1000 |
| 3 | 3 | 2000 |
| 4 | 4 | 3000 |
| 5 | 5 | 4000 |
| 6 | 6 | 5000 |
| 7 | 7 | 6000 |
| 8 | 8 | 7500 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvpaladin

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 800 |
| 2 | 2 | 1800 |
| 3 | 3 | 3000 |
| 4 | 4 | 4600 |
| 5 | 5 | 6400 |
| 6 | 6 | 8500 |
| 7 | 7 | 11000 |
| 8 | 8 | 14500 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvstrider

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 500 |
| 2 | 2 | 1000 |
| 3 | 3 | 2000 |
| 4 | 4 | 3000 |
| 5 | 5 | 4000 |
| 6 | 6 | 5000 |
| 7 | 7 | 6000 |
| 8 | 8 | 7500 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvtrickster

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 650 |
| 2 | 2 | 1650 |
| 3 | 3 | 2800 |
| 4 | 4 | 4300 |
| 5 | 5 | 5900 |
| 6 | 6 | 7700 |
| 7 | 7 | 9900 |
| 8 | 8 | 13200 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

param\pl\joblevel\joblvwizard

| Row | mLevel | mNextExp |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 600 |
| 2 | 2 | 1500 |
| 3 | 3 | 2700 |
| 4 | 4 | 4000 |
| 5 | 5 | 5400 |
| 6 | 6 | 7000 |
| 7 | 7 | 9000 |
| 8 | 8 | 12000 |
| 9 | 9 | 0 |
| 10 | 10 | 0 |

### rPlAbilityParam


param\pl\other\plabilityparam

| Row | mFVal0 | mFVal1 | mSVal0 | mSVal1 |
|---:|---:|---:|---:|---:|
| 0 | 0.5 | 1 | 0 | 0 |
| 1 | 20 | 0 | 0 | 0 |
| 2 | 0.5 | 1 | 0 | 0 |
| 3 | 5 | 0 | 0 | 0 |
| 4 | 1.35 | 1 | 0 | 0 |
| 5 | 10 | 0 | 0 | 0 |
| 6 | 100 | 0 | 0 | 0 |
| 7–9 | 0 | 0 | 0 | 0 |
| 10 | 0 | 0 | -1 | 0 |
| 11–12 | 0.5 | 1 | 0 | 0 |
| 13 | 0 | 0 | 0 | 0 |
| 14 | 2 | 1 | 2 | 0 |
| 15 | 1.3 | 1 | 0 | 0 |
| 16 | 100 | 0 | 0 | 0 |
| 17–19 | 0 | 0 | 0 | 0 |
| 20 | 0.5 | 1 | 0 | 0 |
| 21 | 1.2 | 1 | 0 | 0 |
| 22 | 1.5 | 1 | 0 | 0 |
| 23 | 1.3 | 1 | 0 | 0 |
| 24 | 15 | 0 | 0 | 0 |
| 25 | 10 | 0 | 0 | 0 |
| 26 | 30 | 0 | 0 | 0 |
| 27–29 | 0 | 0 | 0 | 0 |
| 30 | 0.5 | 1 | 0 | 0 |
| 31 | 50 | 0 | 0 | 0 |
| 32 | 1.5 | 1 | 0 | 0 |
| 33 | 0.75 | 0 | 0 | 0 |
| 34–35 | 1.2 | 1 | 1800 | 0 |
| 36 | 30 | 0 | 0 | 0 |
| 37–39 | 0 | 0 | 0 | 0 |
| 40 | 0.5 | 1 | 0 | 0 |
| 41 | 2 | 1 | 0 | 0 |
| 42 | 20 | 20 | 0 | 0 |
| 43 | 20 | 20 | 21 | 4 |
| 44–45 | 100 | 0 | 21 | 4 |
| 46 | 3 | 1 | 0 | 0 |
| 47–49 | 0 | 0 | 0 | 0 |
| 50 | 0.5 | 1 | 1 | 0 |
| 51 | 15 | 0 | 0 | 0 |
| 52 | 1.5 | 1 | 0 | 0 |
| 53 | 1 | 0 | 90 | 0 |
| 54 | 2 | 1 | 0 | 0 |
| 55 | 100 | 0 | 2 | 0 |
| 56 | 1.5 | 1 | 0 | 0 |
| 57–59 | 0 | 0 | 0 | 0 |
| 60 | 0.5 | 1 | 0 | 0 |
| 61 | 15 | 1 | 0 | 0 |
| 62 | 25 | 1 | 0 | 0 |
| 63 | 1.1 | 1 | 0 | 0 |
| 64 | 1.3 | 1 | 0 | 0 |
| 65 | 50 | 0 | 0 | 0 |
| 66 | 20 | 0 | 0 | 0 |
| 67–69 | 0 | 0 | 0 | 0 |
| 70–71 | 1.3 | 1 | 0 | 0 |
| 72–73 | 0 | 0 | 0 | 0 |
| 74 | 1.2 | 1 | 0 | 0 |
| 75 | 2 | 1 | 0 | 0 |
| 76 | 100 | 0 | 0 | 0 |
| 77–79 | 0 | 0 | 0 | 0 |
| 80 | 15 | 0 | 0 | 0 |
| 81 | 10 | 0 | 0 | 0 |
| 82 | 15 | 0 | 0 | 0 |
| 83 | 1.3 | 1 | 0 | 0 |
| 84 | 1.15 | 1 | 0 | 0 |
| 85 | 20 | 0 | 0 | 0 |
| 86 | 50 | 0 | 0 | 0 |
| 87–89 | 0 | 0 | 0 | 0 |
| 90–91 | 1.2 | 1 | 0 | 0 |
| 92 | 25 | 0 | 0 | 0 |
| 93 | 1.15 | 1 | 0 | 0 |
| 94 | 1.4 | 1 | 0 | 0 |
| 95–99 | 0 | 0 | 0 | 0 |
| 100 | 100 | 0 | 0 | 0 |
| 101 | 2 | 0 | 0 | 0 |
| 102–104 | 0 | 0 | 0 | 0 |
| 105 | 0.125 | 0 | 0 | 0 |
| 106 | 0 | 0 | 0 | 0 |
| 107 | 0.25 | 1 | 0 | 0 |
| 108 | 0 | 0 | 0 | 0 |
| 109 | 1.5 | 0 | 0 | 0 |
| 110 | 2 | 1 | 0 | 0 |
| 111 | 1.2 | 1 | 0 | 0 |
| 112–113 | 0 | 0 | 0 | 0 |
| 114 | 1.7 | 1 | 0 | 0 |

### rPlDefendParam


param\pl\other\pldefendparam

| Row | mShrinkMax | mShrinkSpeed | mBlowMax | mBlowSpeed |
|---:|---:|---:|---:|---:|
| 0–9 | 100 | 0.3 | 100 | 0.3 |

### rPlNeckPos


param\pl\other\plneckpos

| Row | mPos (four stored lanes) |
|---:|---:|
| 0 | -50, 0, 25, 0 |
| 1 | 50, 0, 25, 0 |
| 2–4 | 0, 0, 0, 0 |
| 5 | -50, 0, 50, 0 |
| 6 | 50, 0, 50, 0 |
| 7 | -50, 0, 25, 0 |
| 8 | 50, 0, 25, 0 |
| 9 | 0, 0, 0, 0 |

## Next step

Record stamina and the three timing-factor fields during standing, sprinting and
climbing in the original, then resolve all one-shot consumers and exhausted
recovery. Independently identify the two customization records used by
0x0081c170 to reproduce the recorded horizontal scale.
