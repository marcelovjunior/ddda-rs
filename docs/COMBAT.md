# Combat rules: hits, damage, stagger, status build-up

Traced from the PC exe (2017-12-13 Steam build, see METHODOLOGY.md) with
read-only Ghidra decompilation and disassembly, and from the installed
data through `ddcombat` (this brief), `ddenemy` and `ddparam`.
**Mostly not yet compared against the running game**: the oracle
confirmed the player attack inputs (board #758); the goblin hit
measurement is queued in ORACLE.md until a goblin is present (board #698). Every rule below cites the function it comes
from; anything not traced is marked *unknown* or *guessed*.

Evidence levels used here:
- **traced**: read in the decompiler and, for the arithmetic, in the
  disassembly;
- **data**: read from the installed files with a tested reader;
- **inferred**: follows from traced offsets plus naming, but the exact
  consumer or meaning was not read;
- **unknown**.

Offsets are into the runtime objects (`uEnemy`, `uPlayer`,
`cObjCollision`, ...). Field names come from the reflection data
(`research/dti/dti_props.py`, the classes' `vf03`).

## 1. Data a hit uses

### 1.1 `rObjCollision` (`.ocl`): volumes and attacks

Reader: `ddcombat::ObjCollision` (layout from the loader
`rObjCollision::vf10` 0x00CCB980; names and offsets from the `vf03` of
`Group` 0x00CCD510, `Group::Node` 0x00CCDAA0, `Attack` 0x00CCEAE0,
`SeqIndex` 0x00CD3C70; Riftstone's notes agree). All **8,629 archive
occurrences (415 paths)** parse to their last byte: 172,322 groups,
154,977 nodes, 180,764 attacks, 141,316 empty attack slots, 167,640
sequence entries (`cargo test -p ddcombat --release --test install`).

- **Group**: `mNo`, `mKind`, flags `mIsAttack` (hit volume),
  `mIsDamage` (hurt volume), `mIsScrAdj`, `mIsObjAdj` (push-out against
  scenery / objects), `mIsHang`, `mIsHanged` (grab/climb), `mIsCheck`,
  `mIsNotice`, `mIsNotice2`, and its nodes.
- **Node**: `mShape` (0 capsule, 1 sphere, 4 capsule ending at the points;
  Riftstone's shape table, consistent with `0x0077388A`), `mRadius`,
  `mJoint0`/`mOffset0`, `mJoint1`/`mOffset1`, `mRegionId` (index into the
  owner's runtime region list, 1.3), `mHitPrio`, `mNodeAttr`,
  `mNodeEffectAttr`, `mNodeFreeWork`.
- **Attack**: 83 values + 57 flags, see `ddcombat::ocl::ATTACK_FIELDS` /
  `ATTACK_FLAGS` for the file order and memory offsets. The ones the
  damage code reads are listed with their consumers in sections 3 to 6.
- **SeqIndex**: `mNo`, `mModelID`, `mGroupNo`, `mAttackNo`, `mRelation`:
  ties a collision sequence number (switched on by the motion's event
  bits, section 2.1) to a hit group and an attack.

Files come in pairs: the model's own file holds the groups and the
sequence index, a second file holds the attack table. Goblin:
`collision\em\e01\e0100\e0100` (46 groups, 61 sequence entries, no
attacks) and `collision\em\e01\e0100\e0100_00` (28 attacks; `_00` is the
variant, codex-13's ENEMY_RESOURCES.md has the selection). Player:
`collision\pl\m0001` (groups) and `m0001_00_lv1..3` (attacks; the
`lv` suffix's selector is *unknown*).

Goblin hurt volumes (group 0, `mIsDamage`): spheres r 35 on joints 1 and 2
(region 0) and r 22 on joint 136 (region 1, the head).

### 1.2 `rRegionStatus` (`.rst`): the per-part pools

Read by `ddenemy::RegionStatus` (loader `rRegionStatus::vf10` 0x00CD9F70;
runtime element `cRegionStatusElement`, `vf03` 0x00CDA9A0: `mHPMax` +0x0C,
`mDPMax` +0x10, `mDPSpeed` +0x14, `mBPMax` +0x18, `mBPSpeed` +0x1C,
`mDamageAdj` +0x20, `mHitStopAdj` +0x24, `mAttr` +0x28, `mSurface` +0x2C,
`mSeSurface` +0x30, `mDPResetTimerMax` +0x58, `mBPResetTimerMax` +0x5C).

`FUN_00770910` builds one runtime `cObjCollision::RegionStatus` (0x1E8
bytes, ctor 0x00776BA0) per element of the **first region whose `mType`
matches** (codex-13: type 0 for goblins), appended to the owner's
`cObjCollision`+0xC38 list (count +0xC2C). Copy (traced):

| RegionStatus | Meaning | From element |
|---|---|---|
| +0x04 | id (0 = main pool) | `mId` |
| +0x08 / +0x0C | HP / HP max | `mHPMax` |
| +0x10 / +0x14 | DP (stagger pool) / max | `mDPMax` |
| +0x18 | DP regen per frame | `mDPSpeed` |
| +0x1C / +0x20 | BP (knockdown pool) / max | `mBPMax` |
| +0x24 | BP regen per frame | `mBPSpeed` |
| +0x28 | damage multiplier | `mDamageAdj` |
| +0x2C | hit-stop multiplier | `mHitStopAdj` |
| +0x30 | attribute bits | `mAttr` |
| +0x3C / +0x48 | DP / BP refill timer max (frames) | `mDPResetTimerMax` / `mBPResetTimerMax` |
| +0x1BC | weak-point class source | `mSurface` |
| +0x1E4 | | `mSeSurface` |

Resistances are filled per region by `FUN_00770FD0` (traced) from a
27-float array, which `uEm0100::vf33` passes as `actor+0x59E8`:

| RegionStatus | Count | Default | Source (`cCharParamEnemy`, 1.4) |
|---|---|---|---|
| +0x54..+0x64 | 5 | 1.0 | fire, ice, thunder, holy, dark (`耐炎`..`耐魔`) |
| +0x68 / +0x6C | 2 | 1.0 | slash / strike (`耐斬` / `耐打`) |
| +0x70..+0xBC | 20 | 0 | status build-up accumulators |
| +0xC0..+0x10C | 20 | 999 | status thresholds (`耐毒`..`耐魔法防御力ダウン`; `耐延焼` and `耐氷漬け` swap places, as in memory) |
| +0x114..+0x160 | 20 | 0 | build-up decay timers |
| +0x164..+0x1B0 | 20 | 1.0 | status success chances (0..1) |

The resist values are **multipliers** (1 = normal, 0.6 = takes 60 %), as
their use in 4.1 shows. The status thresholds are build-up amounts
(goblin `耐延焼` 300, `耐毒` 1000; >= 10000 or 0 means immune, 6.).

`mAttr` bits seen in the code (traced, names *guessed* from use):

| Bit | Effect |
|---|---|
| 0x01 | region takes no HP damage (`0x00776F50`) |
| 0x02 / 0x04 | region takes no DP / BP damage (`0x00777110`) |
| 0x08 | HP damage is also applied to the main pool (region 0) (`0x0076DB10`) |
| 0x10 | DP/BP damage also applied to the main pool (`0x0076DB10`) |
| 0x20 | HP never drops below 1 (`0x00776F50`) |
| 0x40 / 0x80 | DP / BP never drop below 1 (`0x00777110`) |
| 0x100 | use the main region's resistances (`uEnemy::vf68`), skip status build-up on this region (`0x0076DB10`) |

Goblin (`charparam\em\em0100`, region type 0): two elements, body (id 0)
and head (id 1), each HP 1000, DP 100 (+0.3/frame), BP 100 (+0.3/frame),
`mDamageAdj` 1, `mSurface` 1, `mAttr` 0x118, DP refill 75 frames, BP
refill 30 frames. A head hit therefore drains the head pool **and** the
body pool (bit 0x08); the goblin dies when pool 0 is empty.

### 1.3 Runtime layout on characters

- `uObjModel` embeds its `cObjCollision` at **+0x10E0** (ctor call at
  0x0088475C). So: regions at actor+0x1D0C/+0x1D18, the per-frame merged
  `HitInfo` at actor+0x1720 (`cObjCollision`+0x640).
- `uObjModel::vf05` (0x00885C10) installs the collision callbacks as
  thunks into the owner's vtable: +0xCA8 → vf59 (accept hit), +0xC7C → vf66,
  **+0xC8C → vf68 (damage mitigation)**, +0xC94 → vf69, +0xC90 → vf72 (main
  pool emptied: death), +0xCA0 → vf75 (part pool emptied), +0xCB0 → vf60,
  +0xC80 → vf67, +0xC98 → vf73.
- Per-collision rates (`cObjCollision`, ctor defaults): +0xD1C damage
  rate, +0xD20 BP-damage rate, +0xD24 DP-damage rate, +0xD28 / +0xD2C
  status build-up rate without / with magick damage, all 1.0; +0xD14 a mode
  (0 default; set per unit state by `uEnemy::vf90` 0x00AA96E0, meaning
  *unknown*).

### 1.4 `cCharParamEnemy` (the enemy common PRP)

`charparam\em\em####_cmn` (class hash 0x7E509FE9) is `cCharParamEnemy`
(`vf03` 0x0074A410, 0x140 bytes). Offsets: exp +0x08, `攻撃力` attack
+0x0C, `防御力` defence +0x10, `魔法攻撃力` +0x14, `魔法防御力` +0x18,
resists +0x38..+0xA0 (as 1.2), `のけぞりガード` +0x120, `ぶっとびガード`
+0x124, human-enemy HP +0xDC. The actor keeps a copy at **+0x59B0**
(codex-13: `0x00AAFDB0` copies to +0x5870 and +0x59B0), so attack is
actor+0x59BC, defence +0x59C0, magick attack +0x59C4, magick defence
+0x59C8, resists +0x59E8. Some species rescale these at run time (e.g.
`FUN_009186E0` multiplies the +0x587C copy by a factor; `FUN_00A85DC0`
doubles attack and multiplies defence by 1.5 for an enraged state).

Goblin: attack 250, defence 75, magick attack 80, magick defence 75,
resists fire/ice/thunder/holy 1, dark 0.6, slash/strike 1.

### 1.5 Tuning tables

`rAdjustParam` `param\game` (`sGameSys`+0xB8754, loaded by `0x00444740`)
and `param\dlc` (+0xB875C, `0x004447C0`); the getter
`rAdjustParam::vf15` (0x00CA47D0) returns `values[i]` unchecked. Indices
used by combat (values are this install's):

| Table[i] | Value | Use |
|---|---|---|
| game[5], game[6] | 100, 0.5 | player defence curve (5.2) |
| game[0x1F], [0x20], [0x21] | 2, 8, 8 | damage / DP / BP multiplier when collision mode +0xD14 == 1 |
| game[0x22,0x25,0x28,0x2B,0x2E] | 1.25, 1, 0.75, 0.4, 0 | damage by weak-point class 0..4 |
| game[0x23,0x26,0x29,0x2C,0x2F] | 1.25, 1, 0.75, 0.5, 0.3 | DP damage by class |
| game[0x24,0x27,0x2A,0x2D,0x30] | 1.25, 1, 0.75, 0.5, 0.3 | BP damage by class |
| game[0x0C] / game[0x3C] | 2 / 2 | enemy flag 0x20000000 damage multiplier (hit flag 0x234 bit 8 clear / set) |
| game[0x12] / game[0x13] | 3 / 10 | enemy flag 0x10: damage / DP-BP multiplier |
| dlc[0x19], dlc[0x1A], dlc[0x1B] | 25, 9, 0.1 | player "critical" roll (4.2) |
| dlc[0x0C..0x0F] | 10, 2, 10, 2 | player per-count attack bonus (4.2) |

## 2. The hit pipeline (one frame)

0. **Activation** (section 2.1): the motions playing on the model switch
   collision sequence entries on for this frame; each active entry places
   its group's nodes as volumes carrying its attack.
1. **Hit detection** (`cObjCollision`, `FUN_0076B7E0`, called from the
   collision manager's update `FUN_00479230`): pairs of attack volumes
   (groups with `mIsAttack`) and hurt volumes (`mIsDamage`) are found. The shape tests are most likely
   `FUN_00771370` / `FUN_007725C0` (large geometry routines next to it;
   *not read*). Each pair gives a node-hit
   record carrying a **copy of the attacker's `Attack` block** (copied by
   `FUN_0076FAB0` at +0x198 in the attack entry, +0x1B0 in the node-hit
   record) and the hurt node's `mRegionId`.
2. Before the copy is used, the **attacker's stats are folded in**
   (`FUN_0088A910`): enemies through `FUN_0088B530`, players and pawns
   through `FUN_0088AA20` (section 3). They overwrite the copy's
   `mAttack` / `mMgcAttack` with the final attack values.
3. For each pair the game builds a `cObjCollision::DamageInfo`
   (`FUN_00774ED0`), runs the accept callback (vf59) and the per-hit
   callback (vf66), then **merges** it into the victim's frame `HitInfo`
   (`FUN_00775D00` copies the first, later hits add):
   `HitInfo+0x74` += physical attack, `+0x78` += magick attack,
   `+0xB8[mElement]` += `mElementRate`, `+0x104` += `mShrinkAttack`,
   `+0x10C` += `mBlowAttack`, `+0x13C` += `mStaminaAttack`, and
   **`+0xD0` counts the merged hits**. The loop tracks the largest
   physical, magick, stagger and knockdown values: the strongest physical
   hit's `mSwordRate`/`mHitRate`, the strongest magick hit's element, the
   strongest stagger hit's `mShrinkType` and trajectory (`FUN_00776980`)
   and the strongest knockdown hit's `mBlowType` and trajectory
   (`FUN_00776A80`) replace the merged ones. A later hit's element
   build-up is added to the slot of the element the merge holds at that
   moment (`[0x6F8 + element*4] += rate` before the element update). The
   status build-ups add up (`FUN_00776810`). A further hit is **not
   merged** if it shares one of the four `mIsSameFrameNoHit00..03` bits
   with the first hit (`(first & bits) == 0` in `FUN_0076B7E0`; the bits
   travel as DamageInfo+0x21E bits 5..7 and +0x21F bit 0).
4. **Mitigation** (`FUN_0076DB10` → victim vf68 through +0xC8C): the
   victim's class subtracts defence and applies resistances (section 5).
5. **Region scaling and application** (`FUN_0076DB10`, section 6): rates,
   `mDamageAdj`, weak-point class; then HP, DP, BP and status build-up on
   the hit region (and on the main region per `mAttr`).
6. Pools emptied raise flags in RegionStatus+0x110 (bit 0 dead, bit 1
   staggered, bit 2 knocked down, bits 3.. statuses); `FUN_0076E7C0` calls
   vf72 (main pool) or vf75 (part pool) on death.

### 2.1 Which frames switch hit volumes on (traced)

Hit timing lives in the **motion data**, not in per-action code:

- Every motion of an `rMotionList` has an event block of four 72-byte
  groups `{u16 remap[32], u32 count, u32 list}` with `count` run-length
  events `{u32 bits, u32 frames}` (Riftstone's LMT notes; offsets are file
  offsets). **Group 0 is collision.**
- Each frame, `FUN_0076F430` (for `uActorModel` owners; called from
  `FUN_0076F2D0`) walks the model's 8 motion layers (uModel+0x380, stride
  0x140). For each layer it calls `FUN_01057060(group 0, f, f)` with
  `f` = the layer's current frame truncated to an integer
  (`CVTTSS2SI` at 0x0076F49D): the OR of the bits of the event run that
  contains `f`.
- For each set bit `i` in 0..15 it reads `remap[i]`:
  `remap / 1000` is an `rObjCollision.mResourceID`, `remap % 1000` an
  index into that file's `SeqIndex` table. The collision keeps up to 8
  body files (+0xC3C) and their attack files (+0xC5C, same slot).
- `FUN_0076F660` -> `FUN_0076F740` -> `FUN_0076F7A0` per node of the
  entry's group `mGroupNo`: the group's kind bits must meet the pass's
  mask (+0xD44) and the node is registered with the entry's attack
  `mAttackNo`, taken from the body file's attack table or, when that is
  empty (goblins), from the paired attack file (`e0100_00`).
  `FUN_0076FAB0` copies the attack and stores the entry's `mNo`,
  `mModelID` and **`mRelation`** (entry+0x190); the hit-history key at
  entry+0x178 is `(seq mNo << 8 | group mNo) << 16 | entry+0x184`.
- Passes: `uObjModel::vf93` -> `FUN_0076B470` runs the activation with
  mask 0x18 (hang / object push-out), `uObjModel::vf09` -> `FUN_0076B300`
  (mask 0x180, the notice groups) and `FUN_0076B660` (mask 0x7FFFFE6F:
  attack, hurt and the rest). Volumes are rebuilt every frame: the hit
  loop frees last frame's entries first.

Goblin example (`motion\em\e01\e0100_at\e0100_at`, motion 1): sequences
0 and 1 (hurt volumes, scenery push-out) are on for the whole motion;
sequence 2 (group 2, attack 0: `mAttackRate` 0.8 strike) is on for
**frames 45..48**, the swing's hit window. Read any list with
`cargo run -p ddcombat --example lmt_hits -- <nativePC> <motion list>`.

Other readers of the event block (all through `FUN_01057060`) do not
place hit volumes: `cMotionSeqCount` (0x0082B452, any group, used by
actions and AI for timing windows), `nCollision::cMotionSequenceSupport`
(0x01108819, scenery collision per sequence page), `0x010852B5` (group
from a byte at +0x6A), and enemy action code at 0x00A51AF0 / 0x00A6E4E6
(group 0 over frame ranges). So group-0 bits 16..31 (set in 2,963 of
9,158 motions) and groups 1..3 are timing flags for actions, not hits;
which bit means what is per action and is left to the action work. The
non-actor path of `FUN_0076F2D0` (a fixed 16-entry table per bit of
+0xD3C) is not used by characters.

### 2.2 Volumes on joints and the shape tests (traced)

- **Points** (`FUN_00CCDE30` with `FUN_00CCE190`): a node point is its
  offset as a row vector times the joint's world matrix with rows 0..2
  normalized (joint scale removed). Joint -1 is the model's world matrix
  (`uModel`+0xB0); -3/-4/-5/-6 are the midpoint of the offset placed on
  joints 14 & 18, 15 & 19, 16 & 20, 17 & 21 (`mov edi, 0xe / 0x12`, ...);
  -2 is a Y rotation at the world origin by an angle argument (*source not
  traced*); a joint the model lacks uses the identity, so the point is the
  offset itself.
- **Volume** (`FUN_00773830`, every frame for every live entry): radius =
  `mRadius` x the owner's scale (the mean of owner +0x60..+0x68; entry
  flag 0x318 bit 6 is set by default in `FUN_007735C0`). Shape 0 is a
  capsule p0..p1; 1 a sphere at p0; 4 a capsule pulled in by the radius
  at both ends, or, when the points are closer than two radii, a sphere one
  radius from p0 towards p1. The entry carries an `MtGeomCapsule`
  (+0xA0: points +0xB0 / +0xC0, radius +0xD0) or `MtGeomSphere` (+0xE0:
  centre +0xF0, radius +0xFC); `FUN_0076B660` hands it to the generic
  `sCollision` broadphase (a grid, `FUN_0116E820`), hurt-side entries as
  queries with the pair callback `0x0076D730`.
- **Tests** (the `MtGeom` double-dispatch slots call `MtCollision`
  functions): sphere-sphere `0x00D13600`, capsule-sphere `0x00D139A0`,
  capsule-capsule `0x00D154E0` (segment-segment squared distance
  `0x00D54B20`, point-segment `0x00D54AE0`). All hit when the squared
  core distance is **<=** the squared sum of radii (touching counts); a
  capsule with coincident points (length^2 < 1.19e-7) acts as a sphere.
  Only current positions are tested: there is no sweep between frames.

### 2.3 Which pairs count, and repeated hits (traced)

- **Pair callback** `0x0076D730` (attacker entry, victim entry): the same
  owner is rejected unless `mIsOwnHit`; `mIsPlNoHit` / `mIsEmNoHit` /
  `mIsEtcNoHit` / `mIsNpcNoHit` reject a victim group of `mKind` 0 / 1 /
  2 / 3. The kinds then decide: bit `0x0150A5A0[victim kind]` must be in
  `0x0150A5E8[attacker kind]` (kind 1 enemies hit kinds 0, 2, 3, 4; kind 0
  hits 1, 2, 3, 4, 7); otherwise only the friendly-hit flags open it (row 2
  of that table, then the team matrix `0x0150A630` by the collisions'
  +0xD30 teams). `mIsFriendHitDamage1_2` / `1_4` / `0` set a factor 0.5 /
  0.25 / 0 stored at the victim entry +0x174 (*consumer not traced*).
  Accepted pairs become `NodeHitInfo`s (hurt node at +0x10, attack node at
  +0x350) on the victim's lists.
- **Ignore zones** (`FUN_00779B10`): a contact inside one of the victim's
  ignore volumes (+0xDA8 list: capsule, sphere, AABB, OBB) is dropped.
- **Same-frame dedup** (`FUN_00779860`): per attacker, contacts with the
  same key or the same non-zero `mRelation` collapse to one; the hurt node
  with the lower `mHitPrio` wins (goblin head 1 beats body 5).
- **History** (victim `cObjCollision`+0xC04): an entry is (attacker, key,
  `mRelation`, timer). The key is entry+0x178 =
  `(SeqIndex.mNo << 8 | Group.mNo) << 16` (the low half, entry+0x184, is
  0 for the per-frame entries `FUN_007735C0` builds). `FUN_0076EAF0`
  blocks damage when the attacker has an entry with the same key, or with
  the same non-zero `mRelation`. Every contact is recorded for the frame
  (`FUN_0076EB60`, up to 0x40). At the end of the frame (`FUN_0076ECA0`)
  a plain entry survives only if the same contact happened again this
  frame; an entry with a non-zero `mRelation` survives while the
  attacker's flag +0x1EC1 is clear; an `mMultiHit` entry keeps a timer of
  `mResetTime` frames that `FUN_0076EF10` counts down by the attacker's
  step and that frees the attack to hit again. New contacts are then
  added.
- Result: **an attack sequence hits a target once per continuous
  contact**; a multi-hit attack again every `mResetTime` frames; all
  sequences sharing a non-zero `mRelation` count as one swing.
- Not modelled: the wall check `FUN_00779270` (a ray from the attacker
  to the victim for owners with `uObjModel`+0x1E7C set), the extra check
  `FUN_0076F020`, and +0x1EC1.

Goal check (`ddcombat` tests): the goblin's first attack (`e0100_at`
motion 1, sequence 2: sphere r 60 at offset (30, 20, 50) of joint 8)
against a standing Arisen (`collision\pl\m0000` hurt group 1: head
sphere r 25 on joint 4, body shape-4 capsule r 30 between joints 1 and 2)
with rigid poses gives **exactly one hit, on frame 45**, out to a
distance of 50 + sqrt(90^2 - 30^2 - 15^2) = **133.5** between the roots,
and none beyond (animated joints change the numbers; the poses are an
input).

## 3. The attacker's value

### 3.1 Enemies (`FUN_0088B530`, traced)

```
phys = actor.attack(+0x59BC)     * mAttackRate   [* actor+0x20A8 if actor+0x20A4]
mag  = actor.mgcAttack(+0x59C4)  * mMgcAttackRate [* actor+0x20B8 if actor+0x20B4]
phys *= Π status(type 0x0E or 0x12) [* status(9) if status flag 0x200]
mag  *= Π status(type 0x10 or 0x14) [* status(9) if status flag 0x200]
attack.mAttack := phys; attack.mMgcAttack := mag
```

`Π status(t)` is `FUN_00837480`: the product of entry+0x18 of every active
`cStatus` entry (list actor+0x2698) whose type is t (1.0 if none). The
data `mAttack` of enemy attacks is 0, so `mAttackRate` alone scales the
enemy's attack stat. Enemies never roll criticals.

### 3.2 Players and pawns (`FUN_0088AA20`, traced)

Inputs: `cCalcPlayerAttackParam` P at player+0x3E80 (ctor 0x0088B760,
filled by `FUN_00B6BC80`): P+0x08 `cPlayerInfo.mBasicAttack`, P+0x0C
`mBasicMgcAttack`, P+0x10 / +0x14 weapon (+equipment) physical / magick
attack (item lookups 0x004599A0 / 0x00459A60), P+0x30 / +0x34 stagger /
knockdown power in % (default 100), P+0x48 / +0x50 physical / magick
multipliers (default 1), P+0x4C / +0x54 flat bonuses (default 0),
P+0x58..+0x60 enchantment (element, element rate, magick bonus).

```
crit = 1
if (P+0x144 or mIsAllCritical) and not mIsNoCritical:
    r uniform in [0,1)
    crit = dlc[0x1A] if r <= dlc[0x19]/100 else dlc[0x1B]      # 9 / 0.1 at 25 %
phys = ((mAttackRate*P.str + P.wpAtk*mWpPhyAtkRate*crit*mWpCriticalRate) * P+0x48
        + mAttack + P+0x4C) * Π status(0x0E|0x12) [* status(9)]
mag  = (((k*P+0x60 + P.wpMag)*mWpMgcAtkRate*crit*mWpCriticalRate + mMgcAttackRate*P.mag) * P+0x50
        + mMgcAttack + P+0x54) * Π status(0x10|0x14) [* status(9)]      # k = 0.4 if P+0x24 > 0 else 1
phys *= (dlc[0x0D]-1)/dlc[0x0C] * P+0x7C + 1
mag  *= (dlc[0x0F]-1)/dlc[0x0E] * P+0x80 + 1
mShrinkAttack *= (P+0x30 + P+0x40)/100 ;  mBlowAttack *= (P+0x34 + P+0x44)/100
status build-ups (mPoison.., mAtkDown..) += weapon values, then *= P+0x88
```

Live (claude-4, board #758, read-only): on the running game P+0x08 =
72.0 = `cPlayerInfo.mBasicAttack` (+0x2B4), P+0x48 = 1.0, P+0x4C = 0.0,
P+0x10 / +0x14 = 2.0 / 10.0 for the equipped weapon, as traced.

So a player hit has **both** a physical and a magick part: the weapon's
magick attack enters every hit through `mWpMgcAtkRate` (1 in the player
attack tables). The "critical" branch is only reached for equipment that
sets P+0x144 or attacks flagged `mIsAllCritical`; what sets P+0x144 is
*unknown*. P+0x7C / P+0x80 are integer counts (*unknown* source).
Augment and vocation effects on P are claude-9's / claude-6's ground.

## 4. Defence on enemies (`uEnemy::vf68` 0x00AAA490, traced)

R = the hit region (or region 0 if `mAttr` & 0x100); n = merged hit count
(`HitInfo+0xD0`).

```
if FUN_00858270(): phys *= dlc[0]; mag *= dlc[0]       # condition unknown (dlc[0] = 2)
def  = actor.def(+0x59C0)    [* actor+0x20B0 if actor+0x20AC]   (0 if hit flag 0x233 bit1)
mdef = actor.mdef(+0x59C8)   [* actor+0x20C0 if actor+0x20BC]   (0 if hit flag 0x233 bit1)
phys = max(0, phys - n*def)
mag  = mag * R.elementResist[mElement]      # 1..5 -> R+0x54..+0x64; element 0 keeps 1.0
mag  = max(0, mag - n*mdef)
phys = mSwordRate/100*phys*R.slash(+0x68) + mHitRate/100*phys*R.strike(+0x6C)
phys *= Π status(0x0F|0x13) [*status(9)] ;  mag *= Π status(0x11|0x15) [*status(9)]
actor+0x2360 bit 0x20000000: dmg *= game[0x0C] (or game[0x3C] if hit 0x234 bit 3)
status flags (+0x26A0) bit 0x10: dmg *= game[0x12]; shrink, blow *= game[0x13]
global flag (0x018FA4F8)+0x20E77B bit 2, area != 0x25D: shrink, blow *= FUN_0044DB90()
then uCharacterBase::vf68 (0x0084D8B0): attacker-side extras (accessory 0x47C
via 0x00B84F10, ...), and x2 for hit flag 0x233 bit 6 against flag 0x8000000.
```

Defence is **subtracted once per merged hit** and **before** the
slash/strike split, and the element resistance multiplies only the
magick part. Hit flag 0x233 bit 1 is the attack's
`mIsNoCalcEnemyDefence` (packed to entry+0x317 bit 1 by `FUN_0076FAB0`,
node record +0x32F, DamageInfo +0x21F by `FUN_00774ED0`, HitInfo +0x233 by
`FUN_00775D00`): with it, both defences and the element multiplier are
skipped. `uEm0100::vf68` (0x008D3840) adds goblin cases
(shield block: damage 0, DP/BP x0.1 or x0.25).

## 5. Defence on players and pawns

### 5.1 Equipment sums (`FUN_00B87450`, traced)

s[0] physical defence, s[1] magick defence, s[2] = 100 - slash resist,
s[3] = 100 - strike resist, s[5..9] fire, ice, thunder, holy, dark resist
(%), s[10..] status resists; each summed over equipment slots with the item
lookups 0x0045A1B0.. (claude-6's item tables).

### 5.2 Curve (`uPlayerBase::vf68` 0x00B7AD90 and `FUN_00B83EE0`, traced)

```
mag  = (100 - s[4+mElement]) * mag / 100
phys = (mHitRate*phys*s[3] + mSwordRate*phys*s[2]) / 10000
F(x, a, b, c, d, e) = (x - x*D/(D + game[5])*c - d) * e,   D = game[6]*a + b
phys = max(0, F(phys, s[0], mBasicDefend, c_p, d_p, e))
mag  = max(0, F(mag,  s[1], mBasicMgcDefend, c_m, d_m, e))
```

With game[5] = 100 and game[6] = 0.5: `D = 0.5*equipDef + baseDef`,
damage taken = `x * (1 - c*D/(D+100)) - d`. c_p (player+0x4B88), c_m
(+0x4B9C) default 1.0 and d_p (+0x4B8C), d_m (+0x4BA0) default 0
(`FUN_00B6CAD0`: abilities and a difficulty factor change them); e
(+0x4BA4) = 1 if player+0x273C bit 1 is set, else 1 + 0.2 x v, where v is
augment 40's value from `FUN_00783340` (traced: `rPlAbilityParam` row 40
`mFVal0` = 0.5 if augment 40 is among `cPlayerInfo.mAbility[6]` (+0x258),
else `mFVal1` = 1; claude-9 board #744). So e is 1, 1.1 or 1.2; which
characters have bit 1 of +0x273C is *unknown*, so the size of this
factor in normal play needs the live check. So players use a **hyperbolic** reduction and enemies
a **linear** one. Further multipliers follow (status 0x13/0x15, guard
0x4CA0 halves, `dlc[2]`, `dlc[3]` for ability bits 0x20 / 0x200000 of
+0x3EF0).

## 6. Region scaling, HP, stagger, knockdown (`FUN_0076DB10`, traced)

```
if coll+0xD14 == 1: dmg *= game[0x1F]; shrink *= game[0x20]; blow *= game[0x21]
if hit flag 0x234 bit 6: dmg *= HitInfo+0x90
dmg  *= coll.damageRate(+0xD1C) * R.mDamageAdj(+0x28) * game[0x22 + 3*class]
shrink *= game[0x23 + 3*class];  blow *= game[0x24 + 3*class]
total = phys + mag                                   # HitInfo+0x7C
```

`class` = R+0x1C0, recomputed each frame by `FUN_007781A0`: `mSurface`
(+0x1BC) plus an offset (+0x1E0), clamped to 0..4 (mode +0xD14 == 1 maps
surface 1/2/3 to 0/1/2). Class 0 is a weak point (x1.25), 1 normal, 2
hard (x0.75), 3 x0.4, 4 immune.

- **Minimum damage** (`FUN_00775C70`, before the total): a part the hit
  has (physical when `mSwordRate`/`mHitRate` give a positive share,
  magick when the magick attack is positive) that mitigation brought to
  <= 0 deals the attacker object's +0x14 value instead (1.0 when there is
  no attacker; +0x14 is `cUnit.mDeltaTime` if that object is the unit,
  *not confirmed*). If no part dealt real damage, the stagger and
  knockdown damage are dropped unless the attack has
  `mIsNoDamageNoResetShrink` / `mIsNoDamageNoResetBlow` (table at
  0x015947A8).
- **HP** (`FUN_00776F50`): `R.HP -= total` unless `mAttr` & 1 or the
  collision is invulnerable (+0xCFC bit 0). With `mAttr` & 0x20, +0xCFC
  bit 1, or an `mIsNoDeath` attack on the main pool, HP stays >= 1. At
  HP <= 0: HP = 0, dead flag.
- **Stagger** (`FUN_00777110`): only if `mShrinkType` != 0:
  `R.DP -= shrink * R+0x4C * coll+0xD24`; DP <= 0 sets flag 2 (the victim
  flinches) and DP = 0 (or 1 with `mAttr` & 0x40). `mIsForceShrink` /
  `mIsForceShrink2` force it when the victim's collision flags +0xCFC
  have 0x1800 / 0x4000; `mIsForceBlow` / `mIsForceBlow2` likewise force
  the knockdown with 0x2000 / 0x8000.
- **Knockdown**: only if `mBlowType` != 3: `R.BP -= blow * R+0x50 *
  coll+0xD20`; BP <= 0 sets flag 4. R+0x4C / +0x50 default 1.0.
- **Recovery** (`FUN_007781A0`, per frame, dt = the owner unit's
  `cUnit.mDeltaTime` +0x14, i.e. in 1/30-s frames): `DP += mDPSpeed*dt`
  up to max, `BP += mBPSpeed*dt`; a running refill timer (+0x38 / +0x44)
  that passes `mDPResetTimerMax` / `mBPResetTimerMax` refills the pool
  at once.
- **Reaction** (`FUN_0076E340`, after HP): each triggered status calls
  vf74 (+0xC9C) with its index and resets its build-up. A knockdown calls
  vf71 (+0xC88), refills DP, stops the DP timer, starts the BP timer and
  sets BP = 0; otherwise a stagger calls vf70 (+0xC84), starts the DP
  timer and sets DP = 0. Status 11 (`耐氷漬け`, frozen) cancels both and
  refills DP and BP. So after a flinch DP regrows by `mDPSpeed` per frame
  and is refilled when the timer passes `mDPResetTimerMax` (goblin: 0.3
  per frame, refill after 75 frames).

The motion played on a flinch or knockdown (`mShrinkType` 1/2,
`mBlowType` 0..3, the knockback `mDistanceZ`.. fields used by
`FUN_0084ED90`) belongs to the victim's damage actions and is not traced.

## 7. Status build-up (`FUN_007772B0`, traced; `ddcombat::status`)

Per merged hit, on region R, `rate` = `cObjCollision`+0xD28 when the
magick part is <= 0, else +0xD2C (both default 1). Twenty slots, each with
an accumulator (R+0x70 + 4i), a threshold (R+0xC0 + 4i, from the PRP
resistances by `FUN_00770FD0`), a chance (R+0x164 + 4i, default 1) and a
decay timer (R+0x114 + 4i):

| Slot | Threshold label | Built by | Kind (`0x015098C0`) |
|---:|---|---|---:|
| 0 | `耐毒` poison | `mPoison` | 0 |
| 1 | `耐のろま` slow | `mSlow` | 1 |
| 2 | `耐暗闇` blind | `mBlind` | 2 |
| 3 | `耐睡眠` sleep | `mSleep` | 4 |
| 4 | `耐油まみれ` oil | `mOil` | 5 |
| 5 | `耐ずぶ濡れ` drenched | `mWet` | 6 |
| 6 | `耐敵化` | `mEnemy` | 7 |
| 7 | `耐沈黙` silence | `mSilence` | 3 |
| 8 | `耐スキル封印` skill seal | `mSeal` | 8 |
| 9 | `耐呪い` curse | `mCurse` | 9 |
| 10 | `耐延焼` burning | fire `mElementRate` x fire resist | 10 |
| 11 | `耐氷漬け` frozen | ice rate x ice resist | 11 |
| 12 | `耐落雷` lightning | thunder rate x thunder resist | -1 |
| 13 | `耐久聖` holy | holy rate x holy resist | -1 |
| 14 | `耐久魔` dark | dark rate x dark resist | -1 |
| 15 | `耐石化` petrify | `mStone` | 12 |
| 16 | `耐攻撃力ダウン` | `mAtkDown` | 14 |
| 17 | `耐防御力ダウン` | `mDefDown` | 15 |
| 18 | `耐魔法攻撃力ダウン` | `mMgcAtkDown` | 16 |
| 19 | `耐魔法防御力ダウン` | `mMgcDefDown` | 17 |

- Only the merged element's slot builds (`HitInfo`+0xF8 picks it);
  burning, frozen and lightning replace each other (a newly fired one
  clears the other two).
- A slot builds only if its threshold is < 10000 and not 0. The
  burning threshold is -800 with region bit 1 and +1000 with region bit 0
  (+0x1DC), frozen and lightning -800 with bit 0 (*inferred*: oil and
  drenched).
- `acc += value * rate`; then a roll `rand * 2^-32` <= chance: a positive
  build-up resets the decay timer, and `acc >= threshold` fires the slot
  (`acc` clamps to the threshold). A failed roll still adds.
- **Decay** (`FUN_007781A0`, constructor defaults of 0x00776BA0): after
  150 frames without build-up the accumulator drops by 0.5 per frame,
  after 450 frames by 1 per frame.
- **Firing** (`FUN_0076E340`): each fired slot calls the owner's vf74
  with its index and resets the accumulator. `uEnemy::vf74` (0x00AAB230)
  maps the slot through `0x015098C0` to a status kind; slots 12..14 are
  handled in `uCharacterBase::vf74` (0x0084E560: lightning chains via
  `uShlElemThunder`, the holy and dark effects). The kinds 14..17 of the
  "down" slots are the `cStatus` types 0x0E..0x11 that the damage rules
  multiply by (sections 3 and 4); 0x12..0x15 are *guessed* to be the
  matching "up" buffs.
- Not traced: what each kind does once applied (duration, the
  `rStatusParam` entry it reads, its effect), how the player side builds
  up statuses (equipment resistances, s[10..] in 5.1), and the weapon's
  own status values added by `FUN_0088AA20`.

## 8. Worked example: a sword light slash on a goblin

Attacker: the Arisen with a sword, attack 1 of
`collision\pl\m0001_00_lv1` (`mAttack` 50, `mAttackRate` 1, `mSwordRate`
100, `mHitRate` 0, `mMgcAttack` 0, `mMgcAttackRate` 0, `mWpPhyAtkRate` 1,
`mWpMgcAtkRate` 1, `mWpCriticalRate` 1, `mShrinkType` 1,
`mShrinkAttack` 100, `mBlowType` 3, `mStaminaAttack` 60). Which motion
uses attack 1 is *unknown* (motion-event side). Let B = `mBasicAttack`,
M = `mBasicMgcAttack`, W / Wm = equipped physical / magick attack; no
buffs, no augments, not a critical-capable weapon.

Victim: goblin `em0100` (1.2, 1.4): defence 75, magick defence 75,
slash 1, element-0 multiplier 1, body region, `mDamageAdj` 1, class 1.

| Step | Value |
|---|---|
| attacker phys (3.2) | (1*B + W*1*1*1)*1 + 50 + 0 = **B + W + 50** |
| attacker mag | (Wm*1*1*1 + 0*M)*1 + 0 = **Wm** |
| enemy phys (4) | max(0, B + W + 50 - 75) = B + W - 25 (n = 1) |
| enemy mag | max(0, Wm*1 - 75) |
| slash split | 100/100 * phys * 1 + 0 = unchanged |
| region (6) | x 1 (rate) x 1 (`mDamageAdj`) x 1.0 (class 1) |
| **HP lost** | **(B + W - 25) + max(0, Wm - 75)** from 1000 |
| stagger | DP 100 - 100 * (P.stagger/100) * 1 * 1 = 0 → **flinch** at default stagger power |
| knockdown | none (`mBlowType` 3) |

Concrete numbers (the Fighter's starting weapon is actually the Rusted
Sword, item 0x141, per claude-9 / claude-6 #882; the example keeps the
Iron Sword): a level-1 Fighter (`param\pl\level\lvfighter` row 1
gives attack 80, *inferred* to be the starting `mBasicAttack`; claude-9
owns the level rule) with an **Iron Sword** (item 301 at forge level 0:
strength 50, magick 0, stagger and knockdown power 100; claude-6, board
#705, from `etc\item\itemList`):

```
phys  = (1*80 + 50*1*1*1)*1 + 50 + 0 = 180
mag   = (0*1*1*1 + 0*60)*1 + 0     = 0
enemy : max(0, 180 - 1*75) = 105 ; mag max(0, 0 - 75) = 0
split : 100/100 * 105 * 1 = 105 ; x1 x1 x1.0 -> total 105
HP    : 1000 -> 895 (body pool; a head hit takes 105 from both pools)
DP    : 100 - 100*(100/100)*1*1 = 0 -> flinch
```

Ten such hits kill the goblin (9.52 rounded up; no rounding to integers
appears anywhere in the traced path, HP is a float). A head hit also
drains the head pool by the same amount (`mAttr` 0x08), so head damage
is not doubled.

Counter-example, a goblin's attack 0 on the Arisen (`mAttackRate` 0.8,
`mHitRate` 100): attacker phys = 250 * 0.8 = 200; player side with no
strike resist: x = 200, damage = 200 * (1 - D/(D+100)) with
D = 0.5*equipDef + `mBasicDefend`; e.g. `mBasicDefend` 80, no armour:
200 * 100/180 = 111.1, times e (5.2): **111.1** if player+0x273C bit 1 is
set, else 133.3 (122.2 with augment 40). Unresolved until measured.

Both are predictions from the traced code, not yet measured. The first
is the regression test of `ddcombat::damage` (synthetic
`tests/damage.rs` and, from the installed files,
`tests/install.rs::worked_example_from_install`: 105). The live goblin,
head and magick checks are queued with claude-4 (board #698, #831).

## 9. Implementation (`ddcombat`)

`ddcombat::damage` holds the rules above as pure functions, each with its
source address: `enemy_attack` (0x0088B530), `player_attack` and
`critical_multiplier` (0x0088AA20), `merge_frame` (0x0076B7E0),
`enemy_mitigate` (0x00AAA490), `player_mitigate` and `defence_curve`
(0x00B7AD90, 0x00B83EE0), `scale_for_region` and `resolve_enemy_hit`
(0x0076DB10), `min_damage_rule` (0x00775C70), `apply_hp` (0x00776F50),
`apply_break` (0x00777110), `resolve_reaction` (0x0076E340),
`tick_region` and `region_class` (0x007781A0). Inputs are plain values
(attacker, victim, region pools, `Adjust` tables), so any number of
players and pawns resolve independently. `ddcombat::events` reads the
LMT event groups and translates 0x01057060 / 0x0076F430
(`activations(motion, frame)`). `ddcombat::volumes` places the volumes
(`node_point` 0x00CCDE30, `build_volume` 0x00773830), tests them
(`overlap`: 0x00D13600, 0x00D139A0, 0x00D154E0), filters pairs
(`pair_filter` 0x0076D730), deduplicates (`contacts` 0x00779860) and keeps
the history (`HitHistory` 0x0076EAF0, 0x0076EB60, 0x0076ECA0, 0x0076EF10);
`resolve_frame` runs one victim's frame and returns the attacks that deal
damage, ready for `enemy_attack` / `player_attack` and `merge_frame`.
Tests: `tests/volumes.rs` (5 synthetic) and
`tests/install.rs::goblin_swing_against_arisen_from_install`.

Tests: `tests/damage.rs` (8 synthetic cases: the worked example, head
link, per-hit defence and same-frame exclusion, critical roll and magick
part, enemy attack vs player curve, element on magick only and
`mIsNoCalcEnemyDefence`, minimum damage, regeneration/refill and
classes) and `tests/install.rs` (`worked_example_from_install` builds the
example from `collision\pl\m0001_00_lv1`, `lvfighter`, `em0100`,
`param\game` and `param\dlc` and gets 105; `motion_collision_events`
reads all 1,032 motion lists and checks the goblin's frame 45..48
window).

`ddcombat::status` holds the build-up (`build_up` 0x007772B0, `decay`
0x007781A0, `fire` 0x0076E340, `StatusPools::from_char_param`
0x00770FD0; tests `tests/status.rs`).

Not implemented: timed statuses and their effects, the species- and
player-specific multipliers listed in sections 4 and 5.2 (the functions
take the core inputs and say what they omit), the special enemy case of
0x0076DB10 (victim vf+0x148 x 0.091), knockback trajectories, guard,
volume placement on joints and the shape tests.

## 10. What is still unknown

- Hit timing and volumes: traced (2.1-2.3). Open: the team ids of the
  friendly-hit matrix, the consumer of the friendly damage factor, the
  wall check, attacker flag +0x1EC1, joint -2's angle, and the meaning of
  event bits 16..31 for actions.
- Hurt/hit shape tests in detail (`FUN_00771370`, `FUN_007725C0`) and
  `mHitPrio` ordering. Riftstone's shape table is taken as is.
- What sets: collision mode +0xD14, P+0x144 (crit-capable), P+0x7C/+0x80,
  `FUN_00858270`, region class offset +0x1E0, the DP/BP refill timers,
  the status decay defaults.
- `cStatus` type numbers (0x0E..0x15, 9) to game names, and the
  `rStatusParam` consumer (status duration and effect).
- The stamina damage consumer (`HitInfo+0x13C`, `mStaminaAttack`), guard
  and just-guard, friendly fire (`mIsFriendHitDamage*`), pawn-specific
  and boss-specific vf68 overrides, the experience award (`mExpRate`).
- No live measurement yet.

## 11. Proposed implementation briefs

1. **combat-hit-volumes** (core, `crates/ddcombat` + a sim crate): place
   group nodes from joints each tick (shapes 0/1/4), activate attack
   groups from the sequence index, pair attack and hurt volumes, filter
   self/friendly hits and same-frame duplicates, produce per-victim
   merged hits with the hit count. Needs the motion-event trace first
   (a small research brief: LMT collision events → `SeqIndex`).
2. **combat-damage** (core, headless): translate sections 3, 4, 5, 6 as
   pure functions over typed inputs (attack row, attacker stats, victim
   stats, region) with regression tests built from this document's
   worked example; enemy and player paths; criticals; per-region HP/DP/BP
   pools with recovery and refill timers; death and part-break events.
   Inputs from `ddenemy` (PRP, RST), `ddcombat` (OCL), adjust tables.
3. **combat-status** (research + core): join `cStatus` types and
   `rStatusParam` categories, trace status durations and effects, then
   implement build-up (section 7) and the timed statuses.
4. **combat-oracle-checks** (oracle lane, claude-4): measure the worked
   example and a few more hits (dark-weak goblin vs magick, head hit,
   stagger counts) to pin the guessed and unknown parts.
5. **player-attack-params** (claude-9 / claude-6 lanes): fill
   `cCalcPlayerAttackParam` and the defence sums from vocation, level,
   equipment and augments (`FUN_00B6BC80`, `FUN_00B87450`).
