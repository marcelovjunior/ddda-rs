# AI resources and state-machine graphs

Evidence checked 2026-10-09 against the player's installed archives and the
2017-12-13 Steam executable (identity in METHODOLOGY.md). Addresses below are
virtual addresses in the unpacked analysis copy. Ghidra was read-only.
Implemented: the headless graph reader and dump command (`mtfsm`), and a
headless interpreter for enemy FSMs (`ddai`, section "Runtime"). Tested:
the entire installed rAIFSM corpus, synthetic regressions, and the goblin
FSMs from synthetic inputs. **Nothing has been compared with a running
original game yet.**

## Corpus and locations

Reproduce the count with:

~~~
cargo run -p mtfsm --example survey -- <nativePC>
cargo test -p mtfsm -- --nocapture
cargo run -p ddtool -- fsm-dump <nativePC> <resource path>
~~~

The install test uses DDDA_NATIVE_PC if set, otherwise the maintainer's
documented nativePC path; it prints a skip message when the install is absent.
The survey reads every archive occurrence, deduplicates by exact inflated
bytes (not a possibly colliding hash), and does not export game data.

| Resource | Unique normalized paths | Archive occurrences | Byte-distinct payloads | Encoding |
|---|---:|---:|---:|---|
| rAIFSM | 3,087 | 4,385 | 3,073 | XFS |
| rAIGoalPlanning | 186 | 2,333 | 158 | XFS |
| rAIPriorityThink | 99 | 162 | 99 | XFS |
| rAIEnemyActionParameter | 59 | 245 | 54 | binary, EAP_ signature |
| rAISensorExt | 76 | 101 | 48 | binary, SNR2 signature |

Thus the brief's 3,073 is a distinct-content count, not a path count.
All 4,385 FSM occurrences pass the graph/reference checks, including archive
copies a first-copy-only VFS scan would miss. The distinct FSMs contain
5,770 clusters, 40,942 states and 42,828 links, of which 27 are disabled.
These independently measured totals agree with the MIT Riftstone format
notes, which supplied research leads; no Riftstone implementation was copied.

Enemy FSMs include ai\fsm\enemy\st100\quest\q0002_goburin and
q0003_em5000; NPC schedules include
ai\fsm\npc\st100\action\go002_morning. AI also occurs in quest\,
scr\<stage>\fsm\ and event\<stage>\...: rAIFSM is not exclusively enemy
combat AI. Pawn-related candidates are the cmc families:
ai\goap\cmc\air, ballista and barrelbomb;
ai\priothink\cmc; ai\sensor\cmc\cmc_normal. The cmc-to-pawn runtime
ownership must still be traced; these paths alone do not establish a complete
per-pawn FSM loading map. Do not assume one global pawn or one AI owner.

The air GOAP sample has PremiseConditionList, GoalList and
ActionInterfaceList (MtArray wrappers). Goal Air references premise 1;
two action interfaces have ids 22/23 and reflected classes
cCmcStrongAttackAir/cCmcClingAir. The cmc priority sample has named groups
such as QUEST - 00, each containing cPrioParam records with sensor, code,
category, OBJECT_ID, extra information, personality and order lists.
Numeric meanings and executable planners remain untraced. Enemy action
parameters live under ai\character\enemy\em*_enemy_act_param; sensor
examples include ai\sensor\enemy\em0100a and em0101a. This brief surveys
the related resources; it does not claim binary readers for EAP_ or SNR2.

Measured schedule example: go002_morning condition 24 combines
cAIActionFSMNpc.GameHour >= 6 and < 8; condition 25 combines >= 8 and < 12.
The stored operators are 16 with children 8 and 5. This establishes where
time-of-day predicates live, not the NPC's route or global scheduling policy.

## Graph and preserved information

rAIFSM stores mOwnerObjectName, mFSMAttribute, mpRootCluster and
mpConditionTree. cAIFSMCluster has its id, owner-node unique id, initial
state id, and ordered mpNodeList. A state has a cluster-local mId, a
mUniqueId, owner id, name, setting/user-attribute bits, processes, links,
optional subcluster and optional transition-from-all condition.
The serialized spelling is **Trainsition**, preserved by the reader.

A cAIFSMLink has a name, destination node id, mExistCondition boolean and
mConditionId. The destination resolves within the current cluster;
conditions resolve in the resource's shared rAIConditionTree. TreeInfo's
mName is a cAIDEnum (element name and id), not the expression itself;
mpRootNode holds the expression. Expression nodes include OperationNode,
VariableNode, ConstS32Node and ConstF32Node. Variables retain owner/property
names, singleton flag, array/bit/index flags and dynamic/enum index records.
Raw operators and all named parameter values remain available.

Processes are **named dispatch records**, not numeric instructions:
mCategoryName + mContainerName + mpParameter. The parameter's reflected
class and all fields (including nested objects and arbitrary string bytes)
are retained. Numeric ids inside particular parameters must not be mistaken
for a universal process opcode. Condition ids likewise select trees; only
mOperator selects an operator routine.

mtfsm exposes Graph, Cluster, State, Link, Process, Condition and Expression.
Heterogeneous reflected parameter records use Record/Parameter rather than
invented field meanings. Expression.record preserves the complete tree;
Expression.children additionally supplies ordered nullable children for
graph consumers. Lists remain ordered and duplicate ids retain first-match
lookup. Name strings remain bytes; only the CLI applies lossy display.
Editor-only root/node metadata is outside the typed gameplay projection;
mtxfs remains available for a complete original XFS tree.

Graph::validate checks initial states, enabled link destinations, enabled
link condition ids, transition-from-all condition ids and nested clusters.
Disabled links remain visible with their raw ids, including 0xffffffff;
the interpreter skips them, so they are not dangling traversable edges.
Twenty-four archive occurrences contain null child operands. These are
retained as None; their runtime meaning is **unknown**, not silently
converted to true/false or removed. A null condition root is also retained.
The reader validates reference integrity, not reachability or satisfiability.

## Executable interpreter

These observations were independently checked in Ghidra this session.
No evaluator or action behavior was translated into Rust; only first-match
state/condition lookup is mirrored, with source comments and a duplicate-id
regression test.

| Address | Observed behavior |
|---|---|
| 0x00E092A0 | Drives each active core and cluster level; with state mSetting mask 1 clear checks transitions before processes, otherwise after processes. Entry/update callback dispatches pass through 0x010ADDC0. |
| 0x00E06710 | Returns early while the cluster timer is active. Runtime core attribute mask 2 enables ordered transition-from-all checks, excluding the current state; mask 8 on a candidate consults its unique-id once list. If none qualifies, runtime attribute mask 1 permits normal links. |
| 0x00E05800 | Iterates links in list order, skips mExistCondition=false, looks up and evaluates the condition, returns the destination of the first true condition. |
| 0x010C9290 | First state matching mId (state stride 0xAC); absent id returns null. |
| 0x0117AAA0 | First condition tree matching id (work record stride 0x34), then prepares its root via 0x01179B70. |
| 0x010ADDC0 | Generic process invocation: virtual slot +0x18 guards/selects dispatch, then slot +0x14 invokes. Callback signatures in decompiler output are incomplete; argument roles are not asserted here. |
| 0x005A8470 | Builds cAILevelAbstractFSM and registers named process containers with callback pointers and parameter DTI. Some Ghidra display names alias neighboring DTI functions: address and body are the evidence. |
| 0x00E088B0 | Creates a process container and wraps supplied callback pointers under the literal names update and state. |

The runtime core attribute in 0x00E06710 is **not assumed identical** to the
serialized rAIFSM.mFSMAttribute. Other setting bits, external forced
transitions, timer units, complete startup/exit policy and multi-core
ownership require further work.

Operator-to-function map: 0x0117BD30 dispatches on the stored operator.
Functions 0x0117B200 and 0x0117B590 were inspected to confirm numeric < and >
(including float paths). This table names dispatch behavior; it is not a
complete coercion specification or permission to substitute Rust operators
for all engine types.

| Code | Dispatch | Interpretation / limits |
|---|---|---|
| 1 / 2 | 0x01179D50 / its negation | truth test / negation |
| 3 / 4 | 0x0117AD10 / its negation | equality / inequality |
| 5 / 8 | 0x0117B200 / its negation | less-than / not-less-than |
| 7 / 6 | 0x0117B590 / its negation | greater-than / not-greater-than |
| 9 | 0x01179DE0 | bit containment per prior Riftstone research; only dispatch verified here |
| 10 | 0x01179F80 | bit intersection per prior research; only dispatch verified here |
| 16 / 17 | 0x0117A0F0 / 0x0117A3B0 | AND / OR per prior research; types affect semantics |
| other, including 0 | default false in dispatch | Empty-operation handling is outside this switch. |

Riftstone reports empty operations true and adjacent-pair evaluation for
multiple operands. Do not infer that operator 0 is always true: the
dispatcher defaults false when reached. The wrapper at 0x0117C040 is not
defined as a Ghidra function in this project; read-only disassembly confirms
a call to 0x01179AF0 and an early return, but this brief has not independently
revalidated all wrapper/coercion behavior or null operands.

Named process registration table from 0x005A8470 through 0x00E088B0:

| Container | update callback | state callback | Parameter class suffix |
|---|---|---|---|
| CtrlMotion | 0x005A8890 | 0x005A8990 | cCtrlMotion |
| CtrlAction | 0x005A8950 | 0x005A8990 | cCtrlAction |
| CtrlActionEx | 0x005A89E0 | 0x005A8990 | cCtrlActionEx |
| SearchSetTarget | 0x005A8A90 | 0x005A8AA0 | cSearchSetTarget |
| SetTargetPos | 0x005A90E0 | shared MtStream::vf12 stub | cSetTargetPos |
| SetEscapePos | 0x005A9170 | shared MtStream::vf12 stub | cSetEscapePos |
| CtrlThinkTbl | 0x005A8A90 | 0x005A9240 | cCtrlThinkTbl |
| CheckHangedNum | 0x005A8A90 | 0x005A9300 | cCheckHangedNum |

Read-only disassembly of CtrlAction confirms that it clears owner flags
0x60000000, reads parameter +8, and calls controlled-actor vtable +0x210.
CtrlMotion clears the same flags and invokes vtable +0x214 with motion
settings. The common state callback can set 0x20000000. Full flag meanings,
actor-specific action ids and all remaining process families are unknown;
registration is evidence of dispatch, not behavioral parity.

## Worked enemy example

Smallest inflated payload under ai\fsm\enemy\ (first in sorted path order
among ties): ai\fsm\enemy\st100\quest\q0007_goblin0000_ai, 1,128 bytes.
A summarized dump, generated by fsm-dump:

~~~
owner=cAILevelAbstractFSM attribute=0x0
cluster 0 initial=0
  state 0 "s0000" unique=2014556473 setting=0x0 from_all=None
~~~

This is a complete one-state graph with no links, processes, subcluster or
condition tree. The initial id resolves to its only state. Its own graph
cannot select another state or request an action; this says nothing about
external actor controllers or scripted commands. The smallest example is
therefore a useful structural boundary case, not an enemy combat policy.

For a minimal nonempty action example, q0002_goburin has initial state 0
(s0000), setting 1, attribute 3 and one process:

~~~
cAIFSMProcessContainer::CtrlAction
  cAILevelAbstractFSM::cCtrlAction
    敵ＩＤ = 5
    アクション番号 = 1
~~~

It has no links or condition tree. CtrlAction resolves through the registration
above to 0x005A8950; the action number remains data, not an inferred animation.

For a transition example, q0003_em5000 has five states; initial id 1 runs
Set Invincible(true) and CtrlMotion(motion 622, blend enabled, 10 frames).
Its enabled link targets state 2 through condition 0, operator 10 over
cAILevelAbstractFSM.SysCondition and mask 0x20000000. State 2 requests
CtrlAction(enemy id 63, action 30), then links to state 4 through condition 1,
Motion Frame >= 275.0. State 4 requests action 2, state 3 requests action 92,
and state 0 requests Finish. Conditions 2 and 3 repeat the SysCondition mask.
The ordered chain is 1 -> 2 -> 4 -> 3 -> 0. Callback disassembly ties the
mask to a real engine flag; exact action identities and behavior remain
unverified against the original.

## Runtime (`crates/ddai`)

Enemy and script FSMs run on one core owned by a `cBaseFSM` subclass:
`cAIObject -> cBaseFSM -> cFSMOrder` (quest and stage scripts, `ddstory`)
and `cAIObject -> cBaseFSM -> cThinkFSM -> cAIConsiderFSM ->
cAILevelAbstractFSM` (enemies). `ddai` holds the shared core and the enemy
owner; `ddstory` may move its cFSMOrder owner onto the core later (agreed
with claude-7, board #957).

- `ddai::cond`: condition trees (0x0117c040, operators 0x0117bd30 and their
  typed routines), ported from `ddstory::cond`. An operation without
  operands is true: 0x0117c040 leaves its result word at 1 (checked in the
  disassembly). Bit-number and dynamic or enum indices are explicit errors.
- `ddai::core`: the level stepper 0x00e092a0 with 0x00e06710 / 0x00e05800,
  ported from `ddstory::script` and generic over an `Owner` (state-change
  hook, variable lookup, update/state process callbacks). Checked again in
  0x00e092a0: a state with `mSetting` bit 2 reruns its level in the same
  frame until a transition fires (bit 4: also after a transition), so such a
  state with no true link would hang the original; the port stops after 1000
  reruns with an error. `mFSMAttribute` bit 2 makes 0x00e09750 create
  per-cluster work memory (0x00e06e00 / 0x00e087e0), which only matters for
  sub-machines; graphs with both are refused. The core attribute is 3
  (0x00e05797) and the cluster timer (+0x18 / +0x1c) is not armed by any
  implemented process.
- `ddai::think::Thinker`: the `cThinkFSM` owner. Fields and properties from
  vf03 0x007a82d0 (SysCondition +0x68, FreeCondition +0x70, Timer[0..3]
  +0x78, RandomTimer +0x88, Check Pos vector +0x90, ...). `frame()` is vf06
  0x007a9cb0: countdowns +0xa8 / +0xa5, the core step, then the running
  timers (run mask +0x74) gain `delta / fps` and RandomTimer counts down.
  The state-change hook is vf05 0x007a9da0 (clears SysCondition and the
  request arm byte +0x6c, sets +0xa8 = 2, clears +0xac..+0xb8; FreeCondition
  and the timers survive). The enemy's `cAICtrl` (unit +0x2E64) calls vf06
  every frame from the character's think (docs/FRAME.md, phase 90).

Variables implemented (getter addresses): SysCondition, FreeCondition,
Timer[0..3], RandomTimer, Attack (flag), Motion Loop Cnt, Check Target
Distance, ActionEx Fail Time, My AreaHit No (fields); Check Pos Length
0x007aba80 (actor +0xe0 to the check point plus sWorldOffset, x87 sqrt, 0
without an actor); Check Player Distance 0x007abe20 (to sPlayerManager
+0x99c, the first party slot); FSMFlag 0x007abcd0 -> 0x004b2810 (the
current stage's FSM flag word, MSB-first); FSMAreaStayFlag 0x005bae00
(sGameSys +0xb884c); SceNo 0x005baf00. Any other property is an error.

Processes implemented, with the callbacks registered by 0x005a8470
(`cAILevelAbstractFSM`), 0x007a74f0 (`cThinkFSM`) and the constructor 0x007495f0
(`cBaseFSM`):

| Process | update (entry) | state (each frame) | Effect here |
|---|---|---|---|
| CtrlAction | 0x005a8950 | 0x005a8990 | clear SysCondition 0x60000000; request `Action`; arm, then 0x20000000 when the actor's action +0x50 is set |
| CtrlMotion | 0x005a8890 | 0x005a8990 | same, request `Motion` with the enabled options |
| CtrlThinkTbl | 0x005a8a90 | 0x005a9240 | clear; each frame request `ThinkTable` (table, mode, class) and set 0x20000000 |
| SearchSetTarget | 0x005a8a90 | 0x005a8aa0 | nearest live candidate strictly within range: request `Target`, 0x20000000; none: 0x40000000 |
| SetEscapePos | 0x005a9170 | stub 0x007497f0 | request `EscapePos` (+ world offset when active) |
| SetFreeFlg | 0x005a9210 | stub | request `FreeCondition` (OR into the actor's FSM, unit +0x2e64 -> +0x4c) |
| Timer | 0x007a9de0 | none | Type 0 start from 0, 1 stop, 2 zero, 3 resume |
| Set Check Pos | 0x007aaee0 | none | store the check vector |
| Set Kill Owner | 0x007ab200 | none | request `KillOwner` |
| Finish / Kill | stub | 0x007497e0 / 0x00749800 | SysCondition bit 0 / 0x10 |

Every other process name is an error naming the process and state. Over the
35 goblin graphs (`gob`, `em0100`, `em0101` paths) 32 run 300 idle frames;
the rest stop at `Check Area Hit` and `Set QuestFlag`.

What the FSMs are: the enemy graphs are short scripts that configure the
native AI and then Finish. Combat itself runs in the think tables
(`CtrlThinkTbl`, e.g. table 22 of `uEm0100`), which are code, not graph
data. Example, q0007_gate2goblin0000_ai, from synthetic inputs (test
`gate_goblin_idle_approach_notice`): s0000 -> "6秒後" (Timer[0] runs; after
6 s, frame ~183) -> "フリーフラグ0on" (FreeCondition 1) -> Finish; restarted
far from gate point B: s0000 -> "ポジションチェック" -> "ダッシュ"
(EscapePos B, ThinkTable 22) -> Finish; at point B: FreeCondition 2 ->
Finish; restarted with FreeCondition 3 at point A: "ポジションチェック" ->
"発見～威嚇吠え" (found, threat roar: Action 72) -> Finish. The restart is
the unit's own (next section); the test uses `Thinker::controlled_frame`
or restarts by hand.

Assumptions marked in the code: actor +0xe0 is a world-space position
(**guessed** from the getters adding sWorldOffset); the target-search
comparison traced for group 1 is used for every group, and group membership
comes from the host; SetFreeFlg targets the actor's own FSM in the tests.

## Think tables (`ddai::thinktbl`)

The enemy FSMs hand combat to the unit's built-in think. CtrlThinkTbl
(0x005a9240) gets the unit's `cThinkMgr` (uEm vf 0x2f4, uEm0100 0x008c7770:
unit +0x6150) and a table (vf 0x2f8, 0x008eaf00: `0x017ebf78[index]`, 31
tables in `.data`). The unit runs the table from its own think: uEm0100
vf136 -> 0x008d2360, which first serves forced actions (damage, grabs),
then, when the current action's record +0x50 is set or unit +0x7360,
fills the target slots (0x008d2930), reloads the default table
(0x008b80a0), runs the interpreter 0x008b81a0, rewrites the result
(0x008d0fb0) and requests it as an action (vf132 0x008d11e0) if it differs
from the current one or +0x6ff5 is set.

**Records** (`cThinkMgrAct`, 0xa0 bytes; read at run time from the exe by
`Tables::read`, 1062 records reachable from the 31 tables). Kind 1 records
form AND chains (condition 0x008b9500 on each; code 9 ends a chain; then
each runs as a command, 0x008ba2a0). Kind 0 rows are tried against the
current target by 0x008b8bb0:
- +0x08 flags: 1 end (pop a call or finish), 2 needs target +0x60 bit 0,
  8 resume here next think, 0x10/0x20/0x40/0x80 compare +0x84 with
  +0xe84, 0x100 sets +0x1210, 0x200 jump to the base table, 0x400 needs
  +0x1211, 0x2000 call (else a jump), 0x4000 locks the table against the
  default reload, 0x8000 delay +0x80 frames, 0x80000 pop a call at the
  next row, 0x100000/0x200000 take a return, 0x400000 random block start,
  0x40000000 block end, 0x4000000 sets +0x120f, 0x80000000 only when
  resuming.
- +0x0c: bit 31 not the last result again, 0x10 re-request the action.
- +0x10 / +0x14: think flags required set / clear (0x008bbd30).
- +0x18/+0x1c distance [min, max), +0x20/+0x24 angle (degrees, wraps when
  min > max, 45..-45 is the back arc), +0x28/+0x2c height; FLT_MAX is no
  limit; scaled by +0x44 when +0x48 (min side by min(scale, 1)).
- +0x30 result: an action id, -1 none (continue), -2 return, -6 switch
  table without an action; +0x34 the jump/call/switch table.
- +0x38, +0x44, +0x50, +0x5c: four (op, a, b) triples. Op low 16 bits 1:
  unit check callback `a` (think +0x4c; uEm0100 0x008ea7a0 -> 0x008ea7e0
  below 1000, uEm 0x008ab100 from 1000), op high 16 bits 1 negates. Op 2:
  unit set callback (+0x50, 0x008ac270 -> 0x008abfa0) on commit. Op low
  byte 3: VM condition `op >> 20` as a check and VM command
  `(op >> 8) & 0xfff` on commit (0x2f03003 = "timer a free" + "start
  timer a at b frames").
- +0x68 / +0x70: per-think (+0x1250) / persistent (+0x124c) state bits,
  high dword require/forbid, low dword set/clear.
- +0x78: four byte rules for the think bytes +0x60..+0x63 (0x008b7ff0,
  0x008b8070).
- +0x80..+0x9c: copied to the think's +0x24..+0x40; +0x88 is the random
  weight in percent.

**Random blocks**: the first row with 0x400000 rolls `rand * 2^-32 * 100`
(0x00d00690); each row passes when the roll is in [sum, sum + weight).
A row with both flags is a single "weight percent" chance.

**Timers** (32 at +0xfac: two ids, a time in frames, flags): commands 0x30
(start counting down), 0x49/0x4a (count up), 0x32 (keyed by the row's
action, the per-action cooldown), 0x4d (stop); conditions 0x2f / 0x31
(free), 0x50..0x53 (compare the time). Ticked by the unit's delta
(0x008b7930 from uEm0100 vf08). A timer at 0 still counts as running.

**Inputs the unit supplies** (host side in `ddai`):
- target slots, 23 `cThinkMgrTargetData` (0x90 bytes) at +0x80, measured
  by 0x008b6de0 (`Slot::measure`: XZ distance, height difference, bearing
  `atan2(dx, dz) - yaw` in degrees). uEm0100 fills slot 0 with its target
  (grab target, else +0x60c4, else the sensor's pick 0x00ab52e0, else
  player 0), slot 1 with the escape point (SetEscapePos -> vf186
  0x008c75d0 -> +0x60e0), slots 2..4 from +0x5f40 / +0x6140 / +0x5fb4
  (0x008d2930);
- 32 think flags (unit +0x6154..), written by 0x008d1280 and 0x008d2930;
  flags 0..2 are the unit's alert state (0x008d1280, values 0/1/2; flag 2
  sends the default table into combat table 1). Other flags used by the
  melee rows (3, 8, 20, 21, 29) are not traced;
- the check/set callbacks. Ported: ids that read only the current action
  (0, 2..6, 12: walking, running, dashing, sitting, lying, guarding) and
  the chances 10/11. The rest read the world (paths 1002/1003/1016/1017 in
  0x008ab3b0 / 0x008ab5b0 / unit +0x5ca0, target state 16, ...) and are
  the host's.

**Goblin (uEm0100) tables**: default table 0x017e0858 (tables 0 and 2).
With alert flag 2 it switches to table 1 (0x017db5d8), which sets state
bit 2 (vf108 mode 2: 0x008d0fb0 then maps Wait 0 -> 1, turns 18..21 ->
26..29, PutWeapon 30 -> 31) and goes to 0x017dac78: turn to face (75..150
and 150..180 degrees each side), a 50/50 taunt (71 FindFinger / 72
ThreatHowl, 0x017da138), then 0x017d95f8 -> 0x017d9058 by range: under 300
(height +-150) 0x017d8798, under 700 0x017d8bf8, else 0x017d3838. Close
range with the melee flags: 0x017cccb8, 30% 104 NAttack12, else 50/50 101
NAttack1 / 102 NAttack2 (0x017cc7b8 adds 109 Swing, 106 LAttackV, 107
LAttackH, 105 NAttack123 under flags 10/16). Mid range 0x017d0098: 40% 108
JumpAttack at 350..450 within 15 degrees, 111 Thrust / 110 SwMove with
other flags. Approach: 0x017cb9f8 dashes (13 DashTrace) under 2000, else
runs (9 RunTrace); running counts timers 3/4 up and stumbles (11) after
240 frames, or after 80 with chance 10. A downed target is kicked (112,
0x017cbef8) with a 900-frame cooldown on timer 13 when flag 29 is set.
Action ids name classes through the unit's action table 0x018335d0
(0x44-byte records, DTI at +0x28, 237 entries).

Table 22 (0x017e7798, the gate goblin's "dash" state) selects slot 1 (the
escape point), queries a path every 30 frames (set 1002, timer 6), checks
special moves (0x017c68b8, callbacks 1009..1013, 1025, 1027, 13), then
dashes (13) when the point is within +-100 degrees, otherwise stops
(WalkEnd 7 / RunEnd 10 / DashEnd 15) or turns (18..21) via 0x017c6e58.

Tests (`tests/think_tables.rs`, the exe's tables with synthetic units)
cover table 22, the taunt, close-range attack odds, the jump attack, the
approach, the run stumble, the turns and the kick cooldown; the
interpreter's own tests use synthetic records. Not ported: the slot
orderings (0x008b71a0, commands 0x40..0x42), point targets (0x5c..0x60),
commands 0x4e/0x4f/0x61/0x63/0x64/0x7a/0x7b and conditions 0x37..0x3e;
they are errors when reached (the goblin tables do not reach them). The
live comparison (a goblin fight's action ids per frame) is queued with
claude-4 (board #1096).

## FSM finish and restart

`cAICtrl` (unit +0x2e64) embeds a `cAIFSMCtrl` at +0x44 (vtable
0x0157836c): +4 active, +5 finished, +8 the FSM. Its per-frame 0x0059f440
(cAICtrl mode +0x14 == 1, via 0x0059e070) runs the FSM; when the frame ends
with SysCondition bit 0 (Finish) it rewinds the FSM to its start state
(`cAIFSM` vf06 0x00e098a0 -> 0x00e09250: empty every layer's stack, push
the node at resource +0x84, force a state entry) and sets finished. Nothing
is unloaded and SysCondition stays set. 0x00aa78b0, the first call in
`uEnemy::vf90` 0x00aa96e0, clears finished once the unit has a current
action (+0x2dd4 != -1) whose record has +0x50 set (the same byte that
lets the think run); it is the only store that clears it (scan of .text).
The same frame's FSM update then enters the start state, whose entry hook
(`cThinkFSM` vf05 0x007a9da0, via 0x00749820) clears SysCondition;
FreeCondition and the timers carry over. So a finished enemy FSM restarts
by itself as soon as its action allows (`Thinker::controlled_frame`, test
`finished_fsm_restarts_when_the_action_allows`). Guessed: that resource
+0x84 is the initial state and that the hook's `this` is the FSM.

A second path reloads an FSM by name: `cLinkUnitFSM` slot +0x20 0x00764f20
queues `cScenarioArg_SetFSM` (callback 0x00765070), which loads through
0x005a1370 / 0x00aad8f0 / 0x00bb45e0 -> `cBaseFSM::setupFSM` 0x00749740
and sets SysCondition 0, active 1; a reload after a Finish still waits for
0x00aa78b0. Kill (bit 0x10), 0x00e095c0 and `uEnemy::vf72` stop and unload
instead. `sAICtrlManager` requests (0x00412910) are not involved.

## The goblin unit (`ddenemyrt::goblin`)

The unit side that runs the think and plays the actions (DDDA.exe
2017-12-13 Steam; traced statically, not compared live).

**Frame.** `uObjModel::vf08` runs vf90, vf91, vf92. In vf90
(`uEnemy::vf90` 0x00AA96E0 -> `uEm0100::vf136` 0x008D2280 ->
0x008D2360) the pending forced actions are served first (death +0x5B58,
then damage +0x5B74, ...), otherwise the think table runs when the
current action's done byte (+0x50) is set (or unit +0x7360), and its
result is requested only if it differs from the current action (or
+0x6FF5). In vf91 (`uCharacterBase::vf91` 0x0084BF60) the action manager
(0x007F1EA0) switches actions (old vf08, new vf05) and runs the current
vf06, so a requested action starts and updates in the same frame. The
hit callbacks (vf70/71/72) fill the pending slots that the next frame's
vf90 serves.

**Action ids.** Bits 24..31 pick an action bank, bits 0..15 the record
(`0x007F1EA0`): bank 0 the goblin's (0x018335F8, 237 records),
`uEnemy::vf05` (0x00AA8830) adds bank 1 (0x018635D0), 2 and 3 (0x01864B98,
the common damage and death actions).

**Motions.** Ids are bank `(id >> 8) & 0xF`, slot `id & 0xFF`
(0x00AB4900, with fallbacks for missing damage motions). Banks
(`uEm0100::vf05` 0x008CDC80, bound in load order by 0x008892B0): 0
`e0100_co`, 1 `e0100_at`, 2 `e0100_dm`, 3 `e0100_ex`, 4 `e0100_pl`.

| Action | Class, vf05 / vf06 | Motion | Moves | Done |
|---|---|---|---|---|
| 0 | Wait 0x008D4A10 / 0x008D4B90 | 0x0A (random start 0..179 on the first entry), idle variants 0x0F / 0x0B / 0x0C | no | every frame |
| 1 | WaitAtck 0x008D4DD0 / 0x008D5000 | 0 and 1..4 in two phases | no | every frame |
| 13 | DashTrace 0x008D6920 / 0x008D69B0 | 0x2C, then loop 0x2D | root travel; turns to its goal at 120 deg/s (+0x6008, ctor 0x008CD5B0) | once looping |
| 26-29 | Turn90 / Turn180 0x008D6E90 / 0x008D7190, vf06 0x008D6FA0 | 0x35..0x38 (0x39..0x3C outside combat mode) | 90 or 180 degrees over the motion | motion end |
| 101 | NAttack1 0x008DD200 / 0x008DC820 | 0x101 | root travel; tracks 3 deg/frame to frame 42 | motion end |
| 102 | NAttack2 0x008DCA10 / 0x008DCAC0 | 0x102 | tracks to frame 31 | motion end |
| 104 | NAttack12 vf06 0x008DCF60 | 0x101, at frame 59 0x102 from frame 16 | as above | motion end |
| 0x7A..0x7F | cEmActDmgShrink 0x0089F2D0 / 0x0089F420 | 0x200, 0x205, 0x204 front; 0x202, 0x208, 0x207 back | root travel (no knockback data) | motion end |
| 0x80 / 0x81 | cEmActActingDie 0x008A66E0 / 0x008A6930 | 0x239 / 0x23A | root travel | never |

**Hit reactions.** Stagger (`uEm0100::vf70` 0x008D3DA0 -> `uEnemy::vf70`
0x00AABAE0): with unit +0x2360 bit 0 (set by every standing action and
the attacks), the goblin's light set: 50% 0x7A, else 50% 0x7C, else 0x7B
(front, attacker within 90 degrees), 0x7D / 0x7F / 0x7E behind; without
it, bank 3 record 8 (0x3000008, the same class and motion 0x200). Death
(`uEm0100::vf72` 0x008D4080): the special deaths (break, gauge, burning,
poison) fall through to `cEmActActingDie` 0x80 front / 0x81 back, or the
knockdown deaths 199 / 227 when a knockdown is pending.

**Host answers** (*placeholders*, marked in the code): the think flags
2, 3, 8, 20 (alert and melee) are set while the goblin has a live
target; path check 1017 answers true (open ground); DashTrace's goal is
the target itself (the game steers toward the next point of its AI path,
+0x5C80 from 0x008AB5B0); the rand is a seeded generator.

**FSM.** A goblin may carry a quest FSM (`Thinker::controlled_frame`):
its `Action` requests become forced actions, `ThinkTable` hands the
built-in table to the think, `EscapePos` fills think slot 1, free flags go
back to the FSM. Test: q0002_goburin's action 1 (WaitAtck) comes first,
then the tables taunt (71), dash (13) and attack.

## Next step

- Compare a live goblin fight with `ddai::thinktbl` (board #1096, claude-4),
  then pin the melee think flags (3, 8, 20, 21, 29) and the world callbacks
  (paths 1002/1003/1016/1017, 9, 16, 17, 20) to their unit writers.
- A second table set: each enemy family has its own array behind uEm vf
  0x2f8, e.g. uEm0700 0x01751f3c, uEm2000 family 0x01751f08, uEm5100
  0x017218a0, uEm5200 0x016cd234, uEm5400 0x016a12bc, uEm5500 0x016af8f0,
  uEm6000 0x0166f760, uEm8300 0x0165d670; the interpreter is shared, the
  unit sides (callbacks, result rewrite, action table) are per family.
- The two missing goblin FSM processes (`Check Area Hit` 0x007aafd0, `Set
  QuestFlag` 0x007aad90).


## Wolf and harpy registration (enemy-families, in progress)

`ddai::thinktbl::families` reads the default wolf and harpy graphs with
one shared record walker. Wolf initialization `0x008f06f0` installs
`0x017ab188`; harpy initialization `0x0093cbd0` installs `0x01773ec0`.
Their think runners `0x008f3830` and `0x0093b470` also pass grabbed tables
`0x017c27b8` / `0x01779c20` to common selector `0x00aad940`.
The indexed placement override arrays are not yet exposed by this API.

Action registration calls `0x007f19c0` with 113 wolf and 76 harpy entries
(EDI verified in assembly at `0x008f0747` and `0x0093a754`). Its input
points to the DTI field, at `0x018386b0` / `0x01841370`, and advances by
0x44. The registration begins at that field; action creation `0x007f1ea0`
passes word +4 as the variant to vf13. Action records and
class names are read from the installed exe, not embedded.

Harpy callbacks `0x0093bd60` / `0x0093bda0` are translated and tested:
check 0 tests state +0x62b0 == 0; command 0 clamps its unsigned argument
to 0 or 1; check 1 compares float +0x5860 against an unsigned threshold
converted exactly by x87. That field's gameplay meaning remains unresolved.
World-facing IDs >=1000 still belong to the common host callbacks.

Installed-data tests parse both graphs and all registered classes, and
verify that parameterizing the walker preserves the goblin roots and its
grabbed graph. This is table/callback support only; flight, action phases,
combat integration and original-game comparisons are not yet complete.


The family tables additionally reach common think commands that the goblin
slice did not implement. Command 0x4f resets timer IDs/time and preserves
high flags (`0x008b76e0`); command 0x42 selects the nearest eligible masked
slot (`0x008bc3b0`, `0x008bc410`). Its stable distance ordering is the
second sort in `0x008b71a0`, called with EDX = think +0x70. Tests cover
selection masks, equal-distance order, nonselectable targets and timer flags.
Wolf local check 0 counts living same-class sensor results plus the querying
wolf (`0x008a9f00`, `0x008f7440`); it is a pack-size check. Sensor collection
itself is still host work. Check 6 consumes the pending-notice byte once.

The installed family bodies now load their own model/skeleton, three
co/at/dm motion banks and collision resource. Unlike em0100, both resource
lists register one rObjCollision for body and attacks, with no `_00` file.
An installed-data test confirms attack and hurt activations in the wolf
bite strike and harpy air-attack motions. Shared enemy status/stat loading
is factored without changing the goblin path. Actor scale 1 remains unverified.

Sourced action primitives cover the wolf bite sequence 0x100 -> 0x101 ->
0x102 (`0x008fe340`, `0x008fe390`) and harpy vertical distance damping
(`0x00937e10`); these have regression tests. A synthetic graph probe now
reaches wolf Howling/AttackRun/TraceDash and harpy GroundToHover. This is
not a fight acceptance: the harpy also reaches generated-point command
0x5f. This generated-point command is now translated from `0x008ba2a0`:
the row offset rotates by owner yaw plus selected-target bearing, and is
added to the owner's position. Fixed target slots and the optional +0x58
host notification are supported. Automatic slot 23 uses the old counter
returned by `0x008aa240`, increments with saturation at 22, and begins at
12. Command 0x61 (`0x008b5690`) resets slots 9..22 and that counter.
Regression tests cover coordinates, metadata, selection history, missing
owners/targets, saturation and reset. Commands 0x7a/0x7b assign category
targets through `0x008bc550` / `0x008bc5b0` / `0x008b6f20`; tests pin the
slot mask fallback, all-category bit, explicit mask and validity checks.
The initial category-map values remain unresolved and are represented as
unassigned until a traced command writes them.

Harpy AirAttack's phase-zero approach (`0x009453c0`, `0x00945490`) and
target-angle interpolation (`0x0093ca10`) now have isolated runtime helpers.
The approach steers only before frame 30 and beyond 400 cm, switches to
strike motion 0x102 at frame 8 below 400 cm, and exits after 60 frames when
moving farther away than its initial range, after 120 frames, or on ground
contact. Tests pin the strict boundaries, wrapped turns and damping. Ground
ray results remain supplied by the host. Full host sensing, remaining
actions, death and presentation integration are still in progress; this is
not a completed enemy fight or an original-game comparison.

Harpy takeoff (`0x0093f580` / `0x0093f5e0`) plays motion 0x29,
switches movement bits from 4 to 1 strictly after frame 18, enters motion
0x1f at frame 57 on completion, and marks the action done on its following
update. The first nine think flags now project from grounded/alert state,
the action class query (`0x009381e0`), prior movement bits and common
tension classification (`0x0093b470`). This explains the graph's grounded
path when flight bit5 was absent from the earlier synthetic probe. Sensor
collection, current tension and the other think bytes are still host
inputs, not invented defaults. Shared helper `0x008ec740` classifies
current +0x5f0c below/equal +0x5efc as band1, above/equal +0x5f04 as band2,
and between as band0; its low test runs first. Those thresholds come from
the family's Tension resource (wolf loader `0x008f7b80`). The helper and
its inclusive boundary behavior now have a regression test.

Wolf local check5 now has the sourced posed-joint4 horizontal cone helper
(`0x008f5cc0`). Its full angle is already radians, unlike check1; it
normalizes only XZ and rejects missing joints or coincident horizontal
points. Regression coverage includes facing, rear targets, yaw rotation
and vertical-only separation. Connecting sensed targets remains host work.

The wolf steering coefficient now resolves to the shared angle controller:
`0x007975d0` sets goal/rate, `0x00797470` steps the shortest wrapped angle
by rate times unit time, and `0x0084cc00` applies status-adjusted time.
Regression tests cover both wrap directions, snapping and the minimum rate.

An explicit per-owner Beast action runtime now executes the installed wolf
bite phases and harpy takeoff/standard grounded death. It advances root
motion, refreshes posed attack/hurt volumes and keeps fighter pools per
instance. An installed-data regression runs each action twice, checks the
motion sequences, owner IDs, attack activation and deterministic positions.
Unsupported actions fail without replacing the current action. These are
selected-action tests, not think-driven fights. The local cEm0200Die action requests 0x2ff
(`0x00908660`), absent in its installed damage bank; the no-motion/corpse
path is unresolved and remains unsupported rather than inventing a clip.
This is not the normal wolf death dispatch: uEmWolfBase::vf72 (0x008f6230)
selects common-bank action 0x03000042 for the ordinary grounded case.
Bank 3 is registered at 0x01864b98 by 0x00aa8830; record 0x42 names
cEmActActingDie and its primary motion field +0x14 resolves to 0x239.
The record copier 0x004c9e50 moves that field to action +4, which
0x008a66e0 starts if present. The selected-action runner now reads this
record from the installed executable and plays the installed wolf clip.
Its valid-owner update (0x008a6930) never sets the action done byte, even
after animation end. An installed regression checks motion selection,
deterministic end pose, dead attack suppression and that persistent state.
The shared Fight dispatches this ordinary wolf death before its next action
update (pending selection in 0x008f3830), including deaths from the status
pass. A mixed-family regression verifies the animation advances without
restarting, clears its captured target and keeps attack volumes empty.
Special knockdown, grab, subtype and status-death overrides, plus corpse
flags/timers, remain unconnected. All four installed beast regressions pass.
Harpy corpse removal, status-time integration, target sensing and viewer
dispatch also remain host work.

Selected-action coverage now also includes the normal uninterrupted wolf
howl (0x009026a0/0x009027c0) and AttackRun (0x008fe110/0x008fe160).
The latter loads range and turning rate from param/em0200 rows0/1, then
raises the next-think attack flag only inside the strict squared-distance
and 60-degree cone gate. Its done byte permits next-frame thinking. Tests
cover exact range equality and rear rejection using installed parameters.
Pack broadcasts and interrupted howl still require host integration.

The shared Fight now includes wolves and harpies in hit/status processing,
posed volumes, push-out, swing-history release, pawn target inputs and player
spell/shell targeting. Goblins and beasts allocate from one enemy-owner
counter; the existing fixed seat/enemy/shell ranges still need a general
allocator before large populations or high seat IDs are supported. Callers
explicitly select supported actions and captured target owners. Dead beasts
stop attacking; ordinary wolf death dispatch is connected, while damage
reactions and harpy/conditional death dispatch are pending.
An installed-data regression uses two player seats and mixed enemy families,
checks Ingle damage is attributed to seat 1 and leaves the distant beast
unhurt, and repeats each family run to compare deterministic digests.
All 35 ddenemyrt tests pass (one debug probe ignored). This verifies shared
combat plumbing; autonomous family fights and original-game comparison remain
open. Next: connect the family think host and harpy death selection.
# Wolf selected locomotion follow-up

The selected-action runtime also supports cEm0200Run and cEm0200Dash.
Run entry 0x008fc720 selects motion 0x1a at frame zero; update 0x008fc770
sets done when the prior motion phase reports end. Dash entry 0x008fca50
selects motion 0x24 at frame one; update 0x008fc960 sets done immediately,
allowing the next think while its authored root travel continues. Installed
action regressions verify the start frames, forward root displacement,
completion timing and deterministic repetition. Unit navigation/pack steering
and think-node +0x124 bookkeeping remain unconnected; TraceDash is not treated
as plain Dash.

Harpy death selector 0x0093c700 can override local actions 3/4 with common
0x0300005f / 0x03000060. Both installed common records name cEmActDieBreak,
with variants zero/one. Entry 0x008a7fb0 and update 0x008a8070 drive effects,
hide/cleanup and a 60-frame removal timer; this is not the ordinary grounded
death animation. Actor +0x27b0 is petrification progress: status-kind 12
handler 0x00853260 decreases it toward zero and restores one when removed;
0x0084df60 sets the death byte +0x2b51 when it reaches zero. The harpy
selector then chooses common 0x03000060 (break variant one). Its separate
+0x5cb4 condition chooses 0x0300005f and remains semantically unresolved,
so automatic harpy death dispatch is still pending.

## Prologue chimera: initial source survey

The stage-380 progression gate needs a real persistent kill of placement
group 10/index 0 (opening-prologue survey), so em5200 cannot be substituted
with a timer or a zero-HP generic actor. The DTI registry separates uEm5200
from uEm5200_00 and uEm5200_01, each with its own action/death classes.
The body think-table array is 0x016cd234; its death callback is 0x009a4dc0.
That callback sets a bit in actor+0x2e38 and tests it with +0x2e34 before
calling common enemy death. The masks' part-state meanings remain unresolved.
It then selects local action 0x3a or 0x52..0x58 according to current mode,
motion and another actor flag. This is more than one ordinary death action.

cEm5200ActDie entry 0x009aec20 selects internal states by its action variant,
uses motions 0x239/0x23b/0x23c/0x2b8, initializes a 180-motion-frame timer,
and references child units at body+0x72e0/+0x72e4. Update 0x009aef20 waits
for the initial motion, transitions to a death motion, counts down the timer
and enters cleanup through 0x00aad0c0. Child death, damage routing and the
persistent placement-kill writer still need tracing before implementation.
This survey is source evidence only; no chimera runtime or original-game
comparison is claimed.
