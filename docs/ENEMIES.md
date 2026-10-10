# Enemy parameter data

Implemented and tested on the 2017-12-13 Steam PC install, 2026-10-09.
Not compared against live combat. This is resource decoding, not damage,
status-effect, projectile, or enemy simulation.

## Evidence and coverage

Starting reference: Riftstone format notes, sections **PRP -- enemy /
character parameters** and **Flat parameter formats** (MIT; private
reference `E:\ddda-re\refsrc\riftstone_formats.md`). The older reference's
counts are not this install's counts. No reference implementation was copied.
The layout claims were checked against the files and PC executable:

- `rPropParam::vf10`, `0x00fde3c0`: PRPZ magic, marker, class lookup, XFS
  deserialization. Confirms the wrapper layout independently of the PS3
  address in the reference.
- `rRegionStatus::vf10`, `0x00cd9f70`: region count, region number/type,
  element count, and all fourteen element scalars in disk order.
- `cRegionStatusElement::vf03`, `0x00cda9a0`: reflected names and memory
  offsets, joined to the writes in the preceding loader. In particular,
  `mSurface`, `mSeSurface`, `mAttr` are serialized in that order even though
  `mAttr` precedes the surface fields in memory.
- XFS property labels and types are self-describing. Japanese PRP labels
  are UTF-8 and survive the existing `mtxfs` reader.

`ddenemy/tests/install.rs` reads **every archive occurrence**, not just
the first copy exposed by a VFS path. Results:

| Resource | Archive occurrences | Distinct paths | Failures |
| --- | ---: | ---: | ---: |
| rPropParam | 2,530 | 229 | 0 |
| rRegionStatus | 110 | 106 | 0 |
| rStatusParam | 2 | 2 | 0 |
| rShlParamList | 490 | 22 | 0 |
| rAdjustParam | 1,254 | 73 | 0 |

All 2,444 occurrences of common-stat class `0x7e509fe9` also pass the
typed `CommonStats` adapter. The per-ID loader succeeds for 91 matching
`charparam\em\em####_cmn` IDs. These counts are measured, not hardcoded
acceptance conditions; validation supports another local installation.
Tests skip only when the installation directory is absent. Set
`DDDA_NATIVE_PC` to override the default maintainer path.

```text
cargo test -p mtprp -p ddenemy -- --nocapture
cargo run -p ddenemy --example inspect -- <nativePC> em0100 em0200
```

Eight synthetic unit tests cover PRP wrapper/class agreement, truncated
input, oversized counts, trailing data, RST field order, adjust float-bit
preservation, typed XFS extraction, path arrays, null slots, and IDs. One
install test covers all 4,386 occurrences and the per-ID loading convention.
No game fixtures are embedded.

## PRP wrapper and common stats

`mtprp::parse` accepts a 12-byte little-endian wrapper: `PRPZ`, marker
`0x77ced14c`, root class hash; an ordinary version-0x0109 XFS follows at
offset 12. The marker and root hash are validated, and the entire XFS
must parse without trailing bytes. The wrapper is not inherently an enemy
format: for example `effect\prp\d_pl` is a light-related class.

`charparam\em\em####_cmn` commonly holds class `0x7e509fe9`; the
un-suffixed PRP can hold species-specific data. `CommonStats` deliberately
accepts only the common class. The complete typed XFS tree is retained,
including every field not given a Rust convenience member.

**Evidence levels:** “traced layout” means the PC loader/reflection was
examined; “label-derived” means the stored name suggests the meaning but
its gameplay consumer has not been traced; “unknown” means no semantic
claim is made. None of the PRP gameplay meanings below is marked traced.

| Stored common-class label(s) | Type | Meaning / evidence |
| --- | --- | --- |
| 経験値 | u32 | Experience, label-derived; award modifiers untraced |
| 攻撃力 / 防御力 | f32 | Attack / defence, label-derived |
| 魔法攻撃力 / 魔法防御力 | f32 | Magick attack / defence, label-derived |
| 体重 | f32 | Weight, label-derived; units untraced |
| 大きさ | u32 | Size category, label-derived; **not** the scale field |
| スケール値 | f32 | Scale value, label-derived; placement composition untraced |
| 拘束タイプ / 投げられ挙動 / その場拘束タイプ | u32 | Restraint / thrown-behaviour categories suggested by labels; enum meanings unknown |
| 風圧ダメージ | u32 | Wind-pressure damage, label-derived |
| 地面揺れ有効 / ヒュージブル無効 | bool | Ground-shake enabled / named-feature disabled flags, label-derived; consumers unknown |
| 耐炎 / 耐氷 / 耐雷 / 耐聖 / 耐魔 | f32 | Fire / ice / thunder / holy / dark-or-magick resistance candidates, label-derived |
| 耐斬 / 耐打 | f32 | Slash / strike resistance candidates, label-derived |
| 耐毒 / 耐のろま / 耐暗闇 / 耐睡眠 | f32 | Poison / slow / blindness / sleep candidates, label-derived |
| 耐油まみれ / 耐ずぶ濡れ | f32 | Oil / drenched candidates, label-derived |
| 耐敵化 / 耐沈黙 / 耐スキル封印 / 耐呪い | f32 | Hostility / silence / skill-seal / curse candidates, label-derived |
| 耐延焼 / 耐氷漬け / 耐落雷 | f32 | Burning / frozen / lightning candidates, label-derived |
| 耐久聖 / 耐久魔 / 耐石化 | f32 | Holy / dark-or-magick durability and petrification candidates, label-derived |
| 耐攻撃力ダウン / 耐防御力ダウン | f32 | Attack / defence reduction resistance candidates, label-derived |
| 耐魔法攻撃力ダウン / 耐魔法防御力ダウン | f32 | Magick attack / defence reduction resistance candidates, label-derived |
| のけぞりガード / ぶっとびガード | f32 | Flinch / knockback guards, label-derived |
| ContType / JointType | u32 | Enum values unknown |
| Offset | f32 | Offset, role/units unknown |
| 基点タイプ / 参照関節番号 | s32 | Anchor type / reference-joint number, label-derived |
| 基点オフセット | vector3 | Anchor offset, label-derived; coordinate frame unknown |
| 経路サイズ / OBJ補正サイズ / 吸い込みサイズ | u32 | Path / object-correction / suction sizes, label-derived; units unknown |
| 人間敵 HP | f32 | Human-enemy HP, label-derived; do not substitute for creature RST HP |
| 人間敵 のけぞり耐久値 / 人間敵 ぶっ飛び耐久値 | f32 | Human-enemy flinch / knockback durability, label-derived |
| 近距離カメラ番号 / 中距離カメラ番号 / 遠距離カメラ番号 | u32 | Near / middle / far camera numbers, label-derived |
| 近距離カメラになる距離 / 中距離カメラになる距離 / 遠距離カメラになる距離 | f32 | Corresponding camera distances, label-derived |
| リターンテリトリー発動タイム / リターンテリトリー継続タイム | f32 | Return-territory activation / duration times, label-derived; units unknown |
| 落下になる高さ / ダメージを受ける高さ / 一撃死になる高さ | f32 | Fall / damage / fatal-height thresholds, label-derived |
| ダメージ値(最大体力の指定％) | f32 | Damage as percentage of maximum HP according to label; consumer untraced |
| 死亡沈み時、死体状態になる距離 | f32 | Death-sinking corpse transition distance, label-derived |
| 沈み中にブクブク SE KEY_OFF する距離 | f32 | Sinking sound key-off distance, label-derived |
| 拘束スローレート Lv1 / 拘束スローレート Lv2 / 拘束スローレート Ex | f32 | Restraint slow-rate variants, label-derived |

Resistance entries are **f32 values, not a per-byte lookup table** in
these PRPs. `CommonStats::resistances` keeps all 27 original labels. It
does not invert, divide, clamp, or convert them to percentages. Values
around 1 and values in the hundreds/thousands coexist and likely have
different consumers; applying one formula to all would be a guess.

## Region status (RST)

Little-endian magic `0x20110930`, u32 region count, then each region:
u32 `mNo`, u32 `mType`, u32 element count, followed by 56-byte elements.
Every scalar below has a **traced layout/name**. Gameplay roles beyond
the names, region selection, ID meanings, timers and body-part joins
remain untraced.

| Element offset | Type | Reflected name |
| --- | --- | --- |
| 0x00 / 0x04 | u32 | mNo / mId |
| 0x08 | f32 | mHPMax |
| 0x0c / 0x10 | f32 | mDPMax / mDPSpeed |
| 0x14 / 0x18 | f32 | mBPMax / mBPSpeed |
| 0x1c / 0x20 | f32 | mDamageAdj / mHitStopAdj |
| 0x24 / 0x28 / 0x2c | u32 | mSurface / mSeSurface / mAttr |
| 0x30 / 0x34 | f32 | mDPResetTimerMax / mBPResetTimerMax |

The reference identifies `mHPMax` as creature health. This work verifies
the field's name and bytes, **not** the runtime `getHp` consumer. Multiple
regions and elements can repeat HP or carry different part values;
`RegionStatus` therefore preserves them all and offers no summed HP API.

## Status, shell and adjust parameters

`rStatusParam` has XFS root `rStatusParam`: u32 `mQuality`, class-reference
array `mArray`. Each entry has u32 `mCategory`, bool `mIsOverwrite`, u32
`mOverwriteNum`, f32 `mTimer`, f32 `mParam0`, f32 `mParam1`.
`param\status\enemy` and `param\status\player` each parse. Categories,
overwrite rules, timer units and both parameter roles are **unknown**;
their Rust names mirror labels, without implementing status behavior.

`rShlParamList` has u32 `mQuality`, object `mParamList` (an `MtArray` whose
`mpArray` contains groups), plus string `mCmnEpvPath`, `mCmnSePath` and
`mCollisionPath`. Group fields `epvPath` and `sePath` are **string arrays**,
not scalar strings; u32 `seOption` and another `MtArray` `mShlList` follow.
Shell objects are polymorphic (`cShlParamSpeed` and other subclasses).
The reader types the common fields below and preserves the entire XFS
tree, including subclass, collision, model, effect and sound fields.

| Typed shell field | Stored type | Interpretation |
| --- | --- | --- |
| axis / shlSetType | s32 | Unknown enum values |
| scale | f32 | Scale, label-derived |
| initWaitFrame / dieFrame | f32 | Frame durations suggested by labels; clock untraced |
| isTimeDie | bool | Timed death switch suggested by label |
| durable | f32 | Durability suggested by label |
| maxHitCount | s32 | Hit limit suggested by label; sentinel semantics unknown |
| shlOption | u32 | Unknown bit flags |

Null class-reference slots remain `None`. `source` retains the typed
values and original property labels for all other fields; their behavior
is **unknown**, not silently inferred from a similar shell subclass.
Shared lists include `param\cmn_shl`, `param\em_magic`,
`param\goblin_magic`, `param\wight_magic`, and weapon/human-enemy tables.

`rAdjustParam`: `ajp\0`, u32 `0x0201`, u32 count, then count f32 values.
All 1,254 occurrences satisfy this layout. `AdjustParam` retains float
bits and index order; enemy index meanings are **unknown**. The format
was already documented in [FORMATS.md](FORMATS.md); this crate supplies a
headless reader, not tuning constants.

## Sample enemies

Names below come from the game's own
`id\DDN\message\common\enemy_name_eng` labels, read by the text task
(board messages #71 and #100). The `e####` label to `em####` resource
association is **an inference from matching numeric IDs**, not a traced
name-resolution rule. It is not safe as a universal mapping: other name
labels can share a prefix. Plural names are kept as stored.

Stats are read from `charparam\em\<id>_cmn` PRP; HP is specifically
`charparam\em\<id>` RST region `mNo=0`, element `mNo=0`, `mId=0`.
All values below use the VFS's first archive copy. HP is not a sum and
has not been checked in a live encounter. Resistance columns contain
raw floats for labels 耐炎, 耐氷, 耐雷, 耐聖, 耐魔 in that order.

| Game text (label; message index) | Resource ID | HP field | Fire / ice / thunder / holy / dark candidate | EXP | Scale |
| --- | --- | ---: | --- | ---: | ---: |
| Goblins (`e0100_goblin`; 0) | em0100 | 1,000 | 1 / 1 / 1 / 1 / 0.6 | 65 | 1 |
| Wolves (`e0200_wolf`; 3) | em0200 | 800 | 1.1 / 0.85 / 0.9 / 1 / 1 | 50 | 1 |
| Saurians (`e0400_saurian`; 10) | em0400 | 2,500 | 0.6 / 2.5 / 1 / 1 / 1 | 410 | 1 |
| Harpies (`e0600_harpy`; 19) | em0600 | 800 | 1.2 / 1 / 1 / 1 / 0.8 | 140 | 1 |
| Cyclopes (`e5000_cyclops`; 37) | em5000 | 25,000 | 0.6 / 0.6 / 1.8 / 1 / 1 | 4,000 | 1 |

For example, em5000 also has HP values 500 and 0 in other elements;
em0400 has a 200-HP element. This is why the API exposes all regions and
does not claim that the first row alone is a complete enemy health model.

## Per-ID loading and remaining work

`EnemyData::load` takes validated `em####` IDs and looks for PRP
`charparam\em\<id>_cmn`, species PRP and RST `charparam\em\<id>`,
and adjust/shell `param\<id>`. Missing resources remain `None`.
This is an **observed path convention**, not an engine resource-list
resolver. Shared status and shell lists require explicit caller loading;
do not assume each creature has a private copy. Alternate suffixes and
resource-list aliases are not resolved. VFS first-copy precedence remains
unverified against the engine.

Next action: trace one common enemy's resource-list selection and damage
consumer, joining the PRP resistances to selected RST region/element IDs.
That will establish units, effect thresholds and the base-HP selector
needed by M4 without prematurely baking guesses into simulation.


## Cassardis ambient units (2026-10-09, source checkpoint)

`uEm8300::vf33` at `0x00a9be40` loads 17 resource descriptors at
`0x01861b68`, 18 family action descriptors at `0x018616a0`, and installs
ordinary think root `0x0164e5d0`. Explicit first-table selections instead
index `0x0165d670`; this checkpoint does not substitute the ordinary root
for those quest variants. The constructor `0x00a9bd00` writes unit kind
`0x65` to the byte at `+0x2d`.

`uEm8602::vf33` at `0x00aa1b80` loads 15 resources at `0x018627e0`,
15 actions at `0x01862270`, and think root `0x01645228`. The two first
motion-bank requests intentionally share the e8600 common list. Its
constructor `0x00aa0000` overwrites the parent uEm8600 kind (`0x6b`) with
**`0x6d`**. The Cassardis e03 layouts have class `cSetInfoEnemy8600` but
name `em8602`: class alone cannot select the concrete unit's search kind.
This matters to the door occupancy exclusion of kind `0x6b`.

`ddenemyrt::ambient` reads resource paths/classes and action descriptors
from the player's executable; no bulk lists are embedded. Descriptor
word +4 is an action-specific argument, not universally a motion. Wait
entries use it as the motion ID (`0x00a9dcf0`, `0x00aa0d40`, `0x00aa0e20`),
with start frame 0, blend 5, rate 1. Other action classes interpret it
differently. A test loads both models, all three banks for each unit,
checks the Wait motions and resolves both think graphs.

**Implemented/tested:** identity and installed resource/action readers.
**Not yet implemented or compared:** generic host population, action/AI
execution, motion movement, despawn and visual species identification.
The authored placement feed remains a stand-in until those execute.


### Ambient live identity and wait callbacks

Read-only snapshot of PID23408 (private
`ambient-creatures-codex34/population-paused.json`) found two active
uEm8300 and eight loaded uEm8602, corroborating kinds0x65/0x6d. This does
not mean all seventeen authored birds were loaded. The ground actors
selected first tables6/5: roots0x0165d0d0/0x0165ce50, read through
0x0165d670. Current actions were bank1 indices3/4 and motions0x0a/0x0b;
birds used bank1 index3, motion0x0a. Paused/bgOff remained unchanged.

Shared action bank1 is installed by uEnemy::vf05 (0x00aa8830):79
descriptors at0x018635d0. 0x007f1ea0 selects bank from bits24..31 and
index from bits0..15. 0x004c9e50 copies descriptor+0x14..0x20 to action
+4..0x10. Shared cEmActWait reads motion from copied+4 (0x0089c2f0),
not descriptor argument+4. Completion0x0089c3b0 waits motion-end only
when think+0x120f was set on action entry; it ORs body2360/2370 bit0.

Family em8300 Wait vf06 at0x00907620 waits motion-end and ORs those
bits; em8600 Wait/WaitAir vf06 at0x009c5b10 completes immediately.
FlyStart inherits WaitAir entry0x00aa0e20 but waits motion-end through
0x0095bb80. Supported callbacks preserve these distinctions; unsupported
actions return None. Installed ddspawn regression confirms scene1100
admits2/17 and preserves first-table6/5; scene0 does not admit birds.

Three unit tests and two installed tests pass. Host frame-loop execution
and moving-creature visual comparison remain outstanding.


### Flight movement and group removal checkpoint

`cEm8600ActFlyLoop::vf06` (`0x00aa1030`) advances horizontal position
using the previous yaw and unit delta, corrects height towards unit
+0x6024, then turns toward +0x5c10 through `0x00aa0a80`. The turn is
1/8 of the wrapped difference times delta, capped at 0.41887903 radians.
Afterward it adds sin(phase) to y; phase advances by the f64 constant
0.13962634404500326 each action update, **without delta scaling**. The
SSE instruction order was checked: translation precedes turning. Status
bits0x800/0x1000 skip movement and motion completion. The helper ports
position/heading only; the caller still owes the two body-byte writes and
sourced target/height/speed observations. The global zero vector at
0x018cfe80 was corroborated by a separate read-only live read.

`ddspawn::should_despawn` translates `0x0075f860`: removal has independent
flag/scene/hour enable bits. A false spawn gate alone does not remove an
existing group. The enabled removal paths read raw authored windows even
when their spawn gate bits are off. Installed regression covers that
difference and the invalid-hour-window case.

Installed first-table execution (existing ThinkMgr, no supplied callback
answers) selects actions0x1000004/0x1000003 for tables5/6, matching the
read-only original snapshot. Four unit tests and two installed tests pass;
crate-only clippy passes with the existing combat type_complexity allowance.
These are source/function-level checks, not a full moving-bird comparison.


### Ambient canonical ownership and Wait clocks

`ddhost::ambient::Population` owns actors by stage/group/cell/record plus
allocation generation. Duplicate cell admission preserves state; explicit
cell retirement invalidates stale updates. Admission reads ordinary GPL
conditions and kill records, refuses unresolved gates/missing layouts, and
returns unsupported families. Host state hashes include the population.
The owner currently requires its caller to track group retirement before
re-admission; it is not a complete layout-controller lifecycle.

`ddenemyrt::ambient::Runtime` now retains action, interpreter, canonical
motion/frame, completion latch and root travel. Initial actions come from
vf33 0x00a9be40 / 0x00aa1b80; ordinary selection gates from 0x00a9c7d0 /
0x00aa05c0; action callbacks retain their earlier source addresses. Motion
advance follows 0x010629f0 / 0x0105b740, before collision. Unsupported
actions return errors without replacing the previous state with an idle.
Installed first-table5/6 regression selects the observed bank1 actions4/3,
motions11/10; clock advance and latched completion are tested. Resource
names are now scanned once per action bank rather than once per row.

Validation: two installed runtime/resource tests and one installed host
ownership test pass; ddhost/ddenemyrt crate-only clippy passes with existing
combat type_complexity and install chunks_exact_to_as_chunks allowances.
Not yet connected to the host frame loop or renderer; bird flight and
physical collision remain pending. No new original-game comparison in
this checkpoint, and this does not yet replace door occupancy stand-ins.


### Ambient host/view checkpoint at stand-down

The host frame pass now drives authored8300 idle tables5/6 and ddview draws
installed ambient models from canonical owners. Same-motion force=0
requests preserve clocks (0x00886e70). Initial8300 action1 plays motion0
with nonzero root travel; proposals remain separate from canonical
positions until scenery collision is connected. The 300-frame regression
therefore reports19 unresolved actors:2 physics and17 bird AI, while
checking authored idle actions and progressed frames. Group-removal
latches and stage retirement are explicit. Viewer release-check only;
no new screenshot/live comparison, no completed occupancy-feed replacement.

A source lead for the existing unresolved shared battle flag: sGameSys
+BE2CC is written by0x00441650, called from vf06 0x0043ceb0. It consumes
+BE2CD explicit trigger or checks the hostile list(+BE25C/+BE268), live
health, vf108==2, predicate0x007d3f50 and squared range<6250000, then
accumulates+BE2D4 to threshold+BE2D0. The vf06 clearing pass uses
+BE2DC/+BE2D8 and0x00441800. This is a trace lead, not an implemented
host battle-state claim. Full results and exact next functions are in the
ambient-creatures brief.
