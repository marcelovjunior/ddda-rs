# AI resources and state-machine graphs

Evidence checked 2026-10-09 against the player's installed archives and the
2017-12-13 Steam executable (identity in METHODOLOGY.md). Addresses below are
virtual addresses in the unpacked analysis copy. Ghidra was read-only.
Implemented: the headless graph reader and dump command. Tested: the entire
installed rAIFSM corpus and synthetic regressions. **No AI simulation has
been implemented or compared with a running original game.**

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

## Next step

Trace owner binding and variable-property lookup for one enemy or pawn route,
then translate only that route's necessary callbacks into headless,
per-entity simulation with an original-game oracle. Cover null operands,
type coercions, process lifecycle and external transitions before treating
this structural graph as an executable AI implementation.
