# dragon-attack: The dragon attack on Cassardis (st200 ev00 to ev05)

- **Owner:** codex lane
- **Branch:** `task/dragon-attack`
- **Board area:** `story/dragon-attack`
- **Status:** in progress (source-backed AI bridge checkpoint)

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

q0000 from stage 200: event ev00, the dragon camera (GSF 520), the scripted fight "Challenge the Dragon" with its checks (`EnemyCheck`, GSF 378/379, `mCntState > 17` / `> 43`, `AreaStayFlag[1]`), event ev01 (heart taken), ev05, TimeJump and weather, then q0001. The dragon is an enemy in the general runtime, with its own think table, actions and hit volumes for this encounter; the scripted loss comes from the quest data, not special-cased code. Trace which dragon unit and set the stage uses.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddstory/**`
- `crates/ddenemyrt/**` (coordinate with codex-1)
- `crates/ddai/**`
- `crates/ddhost/**`
- `docs/QUESTS.md`
- `docs/AI.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

From stage 200 the fight plays, the scripted loss triggers the heart-taken event, and the state matches the original's q0000 end (quest words, GSF, SceNo 1100), then q0001 starts as on main. Fill in Results and report to `head`.

## Results

### AI bridge checkpoint (2026-10-09, codex-25)

- Implemented general `cAILevelAbstractFSM` processes `CtrlActionEx`
  (0x005a89e0), `SetTargetPos` (0x005a90e0), and `cThinkFSM` world writes
  `Set GSFFlag` (0x007aad10), `Set QuestFlag` (0x007aad90), `Set LotFlag`
  (0x007ab650). Extended actions preserve every signed, unsigned and float
  argument; world writes emit host requests with the original bounds.
- Added motion feedback getters (0x007ab900 / 0x007ab8c0), including the
  two-frame state-change suppression of Motion Frame.
- Installed `AI\FSM\Enemy\st200\quest\Dragon_OP` runs its eleven states
  against explicit synthetic action, motion and area feedback. It sets
  quest bits 1, 20, 21, 5 through its own processes and ends with Kill,
  not Finish. No fixed-delay quest-bit injection is used in this test.
- Tested: `cargo test -p ddai`, 32 tests pass, including three new tests
  for the installed FSM, argument preservation/offset, and flag bounds.
  This validates the interpreter bridge, **not** simulated dragon actions
  or a comparison against the running original.
- Recovery after the loose SATA cable: worktree `git fsck --no-dangling`
  passed; source edits were intact and the complete AI test suite passed.
- Follow-up: the real FSM controller 0x0059f440 disables and unbinds a
  killed FSM, clearing SysCondition, whereas Finish rewinds and pauses it.
  `Thinker::controlled_frame` now preserves that distinction. An installed
  Dragon_OP regression verifies no subsequent updates or action-driven
  restart after Kill, and explicit restart still works. All 33 AI tests pass.

### Encounter evidence and remaining work

- Added `DragonAiData::opening`, reading 52 reachable think records from
  the sequence-4 root and all 93 action classes from the installed exe.
  Descriptor stride and count come from 0x00a3f89f / 0x00a3f8a4 and
  0x007f19c0. The shared `Tables::read_roots` walker is transplanted from
  codex-1's e9cfa51; existing goblin traversal remains unchanged. The
  installed reader regression passes, including all six entrance action
  class identities. This loads definitions, not simulated actions.
- The same runtime reader now preserves all 57 resource descriptors
  registered by `uEm5800::vf05` 0x00a49ed0 through 0x00889de0, including
  paths, reflected classes and flags. The installed regression verifies
  five motion lists and three collision resources. Bank binding and body
  construction are separate work; descriptor order is not asserted to be
  a bank index.
- Follow-up source trace found that flags in the file are uninitialized:
  static initializer 0x01363d50..0x0136412c enables binding for the first
  49 descriptors and leaves the final eight preload-only. The reader now
  translates that initialization rather than exposing misleading zero
  flags. Both installed-resource and boundary/high-bit preservation tests
  pass. The binder 0x008892b0 selects flag bit 0 and passes per-class
  indices to `uObjModel::vf79` 0x00889510.
- Added `DragonAiData::load_body` using those bound resources and a
  caller-supplied actor scale. It loads the skeleton, five motion banks
  and three collision files; existing combat volume activation filters
  by each file's resource ID. The installed regression checks that all
  eight motion IDs selected by the six entrance actions are available.
  Codex-1 independently ran all three dragon tests, passing. This is
  body-data loading, not yet action stepping or an original-game comparison.
- Added opening `DragonData` and owner-specific fight-component
  construction using the shared enemy-stat loader (exact dependency
  transplant from codex-1's 1749553, originally d25e02f). Region kind 0 is
  sourced by EAX=0 at 0x00a49f2d before 0x00770910; common parameters come
  from `em5800_cmn` at 0x00a49f3d. The installed regression compares every
  pool's id/current/max HP, defenses and object size with the parsed game
  records, and verifies independent mutable pools per owner.
- Sequence 4 sets collision +0xCFC bit 0 at 0x00a4a55a. The new fighter
  therefore starts with HP damage disabled, verified through `apply_hp`.
  Loaded stats do not imply that the dragon is ready for damageable
  encounter integration; its phase-93 inputs still require host wiring.
  All three dragon tests pass after this construction change.
- Traced the clearing path: dragon vf93 0x00a40c20 calls enemy vf93
  0x00aa9bf0. When cAICtrl +0x0C is zero, it projects actor +0x2738
  condition bits into collision flags 1/2/4/8, preserving other bits.
  Added local `sync_collision_flags` with explicit controller and actor
  inputs, no inferred defaults. The regression covers the guard, each
  mapping and unrelated-bit preservation; codex-1 independently ran all
  four dragon tests, passing. Runtime action/phase integration remains.
- Added HoverUp's sourced control state (0x00a6ba90/0x00a6bb40/
  0x00a6bc80): scale-Y times 35 velocity, live think-target or explicit
  height, inclusive completion without position snapping, and cancellation
  stopping velocity without manufacturing completion. Added the shared
  hover-transition completion predicate from 0x00a6be00: primary motion
  end or model flag bit 31. Six dragon tests pass, including both new
  regressions. These are action controls, **not** a complete scheduler:
  motion/layer binding, attached-actor notifications, air flags and the
  host's integration phases remain required.

- `scr\st200\etc\st200_00m00n_e00` contains placement id 1,
  `cSetInfoEnemy5800`, sequence mode 4, with the Dragon_OP FSM. The quest's
  EnemyCheck target is kind 2, group 0, index 1.
- `uEm5800` initialization 0x00a49f00 selects think root 0x01699518 for
  sequence 4. Bulk table records must be loaded from the installed exe.
- Previously untraced GSF 379 writer: `aStage200::vf12` 0x0051ccc0. Once
  GSF 378 enables its dragon branch, it captures the first life-region HP
  once, then sets GSF 379 only after HP is strictly below that baseline
  minus 4.0. Independently, a valid Arisen with region HP <= 1 (or no life
  region) and positive effective maximum HP sets the flag. The maximum
  getter is 0x00b845d0 -> 0x00782040. This is original stage-specific code,
  not a timeout or an invented quest rule.
- Implemented that check in `ddstory/src/dragon.rs` with explicit actor
  snapshots and persistent baseline capture. The co-op extension lets any
  valid Arisen satisfy the low-life branch. Three pure regression tests
  pass (compiled directly with `rustc --test` while the shared Cargo target
  was allocated elsewhere), including strict threshold, missing actors,
  missing regions, independent player branch and unordered float inputs.
  The module export and host tick remain coordinated integration work.
- Stage-gate integration is supplied by codex-26's 4a3d654, after event
  processing and before scenarios, with frame-stop guard and stage-reset
  baseline. Integrated regression passed on that branch.
- Added an explicitly limited `Dragon` actor prefix for action 52: entry
  validates support and motion before mutating state; phase 91 performs
  HoverUp control and explicit host-resolved velocity integration, phase 92
  uses the body root-motion driver, and phase 93 updates collision flags.
  Pass-C volume activation is a separate explicit host call with the
  resolved pose, after terrain correction and phase 97; the prior collision
  snapshot persists until that call.
  Common action bookkeeping, motion binding/blend, air control and attached
  message 4 are typed effects that the host must apply. Unsupported actions
  return an error rather than synthetic completion. The six dragon tests
  pass, with installed regression coverage for next-frame height completion,
  owner-specific messages, collision projection and rejection before mutation.
  This prefix has not been compared against the original or wired into the
  host. Transitions 53/54 and TargetFly remain the next supported subset.
- Review of 0x0105b740 confirmed root travel uses actor axis scales and
  +0xDA0/+0xDA4/+0xDA8 multipliers with orientation. The prefix now requires
  an explicit host-resolved root-translation transform, rather than silently
  assuming unit scale and yaw only. A nonuniform rotated-transform regression
  passes; all seven dragon tests pass. Single-track translation is supported;
  animation blending and root rotation remain host work.
- Added transitions 53/54 to the actor prefix. Action 53 binds motion 121
  and returns a typed layer-2/flight-state reset effect (0x00a6bd80 and
  0x00a49480). Action 54 binds 103, captures speed/acceleration parameters
  and sets its yaw controller (0x00a6bf00). Repaired the decompiler's hidden
  atan2 arguments using disassembly at 0x00a6bfca: normalized X/Z are passed
  as doubles in XMM0/XMM1. The local rotator translates 0x0084cc00 and
  0x00797470 with explicit enable/status inputs, slow factor and strict
  arrival comparison. Transition completion reads prior-frame motion end
  or model bit 31; neither transition emits HoverUp's attached message.
  Action-54 exit clears velocity/yaw and emits acceleration-clear bookkeeping.
  Eight dragon tests pass, covering gates, shortest arc, exact-step equality,
  transition resets/completion and actor exit. TargetFly, full animation
  blending and live host effects remain unimplemented.
- TargetFly scheduling is now traced: vtable 0x015d42f0 has no-op vf06
  (0x005f35c0), movement vf07 (0x00a61590), and exit 0x00a62150.
  `uCharacterBase::vf97` 0x0084c800 dispatches action vf07 in pass C,
  after terrain correction and before collision activation, not phase 91.
  Flight reads normalized row 2 of `vf22(-1)`'s cached model matrix
  (`uModel::vf22` 0x00ff9a20 returns actor +0xB0). It does not substitute
  the quaternion it just updated for that cached forward vector.
- Added only `TargetFlyTravel`, an isolated translation substep requiring
  explicit cached forward and turn-lag state. Disassembly at
  0x00a62098..0x00a620fc repairs misleading decompiler stack aliases: the
  strict arrival-radius comparison uses displacement saved before movement.
  Tests cover this boundary, snap/no-restart, speed ramp/slowdown floor and
  the zero-speed initialization distinction. Nine dragon tests pass, also
  including exact yaw wrap boundaries and configured-mode 54 rejection.
  Action 10 remains **Unsupported**: quaternion steering, side-turn layers,
  rider effects, pass-C orchestration and final collision timing are still
  required before enabling it in the actor prefix.
- Still required: actual dragon movement/actions,
  table callbacks, combat volumes and actor feedback; host-owned encounter
  facts and flag request application; playable ev00 -> fight -> ev01 ->
  ev05 acceptance and original-state comparison. Existing ScriptedHost
  opening shortcuts have not yet been replaced.
- Next action: trace async resource binding and the reachable dragon
  actions. PressAttack (0x00a6aed0/0x00a6af70) reads fall parameters from
  `cCharParamEmDragon`; BreathWide (0x00a56a30/0x00a56d10) advances from
  motion 263 to 264 and gates its breath collision by motion frames.
  These dependencies must be implemented before claiming a playable fight.

### Pass-C review follow-up (codex-1)

Recovered codex-25's uncommitted boundary fix after its session ended.
`Dragon::step` now publishes only phase-93 collision flags; the explicit
`finish_pass_c` requires a host-resolved pose after terrain correction and
phase 97. A regression uses actual installed hurt volumes, retains their
prior snapshot through another step, then supplies matrices translated
123 cm vertically and verifies only the final boundary publishes that
correction. This does not supply a host phase scheduler or implement the
remaining dragon actions.

Verified seven dragon unit tests and two installed tests. Clippy passes
with the existing `combat.rs` callback `type_complexity` warnings allowed;
exact executable angle constants are marked deliberately, and the touched
root-travel loop was simplified without changing arithmetic. Release
viewer build passed, with two existing viewer warnings.

### S24 encounter map (2026-10-10, codex-55; branch `step/s24-dragon-map`)

This is a source/data map, not a played fight. The new installed regression
`dragon_opening_reachability.rs` follows the sequence-4 root's table
references and Dragon_OP's enabled state links. It finds **18 commanded
action IDs**: 13 in the think tables and six in the entrance FSM, with 21
shared. World/actor predicates still decide which table rows execute.
The test does not manufacture those predicates or claim every candidate
occurs during one playthrough.

The 52 records reachable from `0x01699518` include four end rows. Their
`+0x30` words are labels, not action requests: `cThinkMgr::think`
`0x008b81a0` checks row flag bit 0 before selecting an action
(`0x008b8528`). Filtering only by a nonnegative `+0x30` would incorrectly
include 1112, 52931, 56710 and 81630. The installed test excludes these and
asserts the 13 table IDs `{0,4,6,19,21,26,27,28,29,34,72,74,86}`, the six
FSM IDs `{10,21,33,52,53,54}`, and every commanded descriptor's class from
the installed `0x0185bcf8` table. Bulk records remain runtime reads.

#### Ordered actions and dependencies for follow-up briefs

First implement the actual **Dragon_OP entrance** in this order. State IDs
are installed data; dispatch is `CtrlAction` `0x005a8950` /
`CtrlActionEx` `0x005a89e0`. Motion IDs below are source bindings, encoded
as `bank << 8 | slot`, not invented animation choices.

| Order / FSM state | Action / class | Motion and source | Have / missing |
| --- | --- | --- | --- |
| 1 / 4, rise | 52 `cEm5800HoverUp` | 100; entry/update/exit `0x00a6ba90/0x00a6bb40/0x00a6bc80` | **Have:** tested local height/velocity/completion prefix. **Missing:** host scheduling, live feedback/effects. |
| 2 / 5, change to glide | 54 `cEm5800HoverKakku` | 103; `0x00a6bf00`, completion `0x00a6be00` | **Have:** tested local yaw/speed/control prefix. **Missing:** host flight/layer integration. |
| 3 / 1, fly | 10 `cEm5800TargetFly` | 126; entry `0x00a60b70`, pass-C movement `0x00a61590`, exit `0x00a62150` | **Have:** isolated travel substep. **Missing:** supported action, steering, side-turn layers, rider effects, phase-97 orchestration. |
| 4 / 3, hover | 53 `cEm5800KakkuHover` | 121; `0x00a6bd80/0x00a6be00` | **Have:** tested prefix and reset effect. **Missing:** host applying flight/layer reset. |
| 5 / 0, land/press | 33 `cEm5800PressAttack` | 298 then 299; `0x00a6aed0/0x00a6af70` | **Missing:** falling/ground contact, parameter-driven acceleration, motion transition, landing consequences. |
| 6 / 2, fire | 21 `cEm5800BreathWide` | sequence-4 263 then 264; `0x00a56a30/0x00a56d10`, exit `0x00a57270` | **Missing:** frame-gated breath collisions, emitter/joint targeting, damage and effect state. |

The state path is `4 -> 5 -> 1 -> 3 -> 0 -> 7 -> 8 -> 2 -> 9 -> 10 -> 6`.
The first four transitions await action completion; landing's state 0
awaits `FSMAreaStayFlag[0]`, then state 7 waits for motion frame >= 10 and
state 8 for motion end. State 2 sets stage LOT 80 and q0 bit 20; state 9
starts timer 0 and waits for >= 5.3 seconds; state 10 sets q0 bit 21;
state 6 sets q0 bit 5 and **Kill**. State 1 already sets q0 bit 1.
GSF 282 is cleared at rise and set at landing's force-point state 8.
The existing synthetic-feedback ddai regression covers this path and its
Kill lifecycle (`0x0059f440`), but does not provide the actor inputs.

Next supply the native unit driver and **conditional combat-table actions**.
`uEmDragonBase::vf136` `0x00a40f20` calls vf196 `0x00a40f70`, which handles
pending action requests, senses/selects targets, builds think slots, runs
`0x008b80a0 -> 0x008b81a0`, and dispatches its result through actor vf132.
The opening root branches into `0x01697f38` and `0x01698b18`, the latter
also calling `0x01695d78`. **Have:** installed definitions and general
ThinkMgr interpreter. **Missing:** dragon-specific callbacks, target/slot
production, think/action handoff and driver guards. Do not substitute an
always-true callback host or assume the FSM's Kill despawns the enemy.

| Table candidate | Source motion bindings / routines | Runtime status |
| --- | --- | --- |
| 0 `cEm5800Wait` | primary 0 and layer-3 motion 3; `0x00a4dca0/0x00a4ddc0` | Missing |
| 6 `cEm5800Walk`, 86 `cEm5800TargetWalk` | 20; `0x00a50360/0x00a50570`, `0x00a691b0/0x00a69270` | Missing |
| 19 `cEm5800Turn` | ground turn 50..56, flight 101; `0x00a52350/0x00a52890` (other-family bindings 150/151 are not opening-dragon choices) | Missing |
| 21 `cEm5800BreathWide` | 263 -> 264 in opening sequence; sources above | Missing |
| 26 `cEm5800HabatakiAttack` | 269; `0x00a5a100/0x00a5a1e0/0x00a5a2d0` | Missing |
| 27 `cEm5800Humitsuke` | 270 or 271, actor-state dependent; `0x00a5b540/0x00a5b5b0` | Missing |
| 4 / 28 `cEm5800Scratch` | 272 or 273, descriptor parameter dependent; `0x00a5bdd0/0x00a5beb0/0x00a5bf70` | Missing |
| 29 `cEm5800Catch` | entry 276 or 312; subsequent 277..281 and 313/314, conditional; `0x00a5c030/0x00a5c1a0/0x00a5ce20` | Missing; requires captured-actor ownership, constraint and release logic |
| 34 `cEm5800Huriotoshi` | 267/268/306 depend on attached occupants; `0x00a5ff00/0x00a60100` (307 is other-family branch) | Missing; rider/attached-actor state |
| 72 `cEm5800Houkou` | 286; `0x00a76d50/0x00a76db0` | Missing |
| 74 `cEm5800HabatakiBreath` | entry 71, then 262 or 264; `0x00a5a680/0x00a5a820/0x00a5ad00` | Missing; flight/breath continuation |

There are additional **conditional action-to-action continuations** outside
the commanded set. These are source leads, not a proof that every guard
is satisfiable in sequence 4 before ev01:

- PressAttack update `0x00a6af70` can dispatch **60 EscapeFly** after its
  150-frame branch, repositioning the actor 500 cm above its resolved pose.
  `cEm5800EscapeFly` `0x00a73980/0x00a73bc0/0x00a74180` binds 100, then
  conditional 103/120/121, and dispatches **16 Landing**, whose entry/update
  `0x00a54c90/0x00a54d40` bind 100 then 61. These actions are missing.
- Catch update `0x00a5c1a0` can dispatch **31 Magic** if actor +0x679A is
  zero, action parameter +0x14 is nonzero and actor +0x637C is nonzero.
  `0x00a5d710/0x00a5e320` starts motion 287, with a conditional 289
  selection. Magic/emitter behavior is missing; the guard is not asserted
  true for the default opening Catch descriptor.
- HabatakiBreath update `0x00a5a820` dispatches **77 BreathWide**, another
  descriptor of the same class, after binding 262 or 264. Its distinct
  parameters must be read from the installed table; implementing only
  descriptor 21 does not cover this continuation.

Finally wire **per-owner encounter facts and original frame phases**:
resource readiness/binding (`0x008892b0/0x00889510`), common entry/exit
(`0x00a4db70/0x00a76c90`), condition projection (`0x00aa9bf0`), motion
feedback (`0x007ab900/0x007ab8c0`), requests for quest/GSF/LOT
(`0x007aad90/0x007aad10/0x007ab650`), and actual final-pose combat volumes
after terrain and vf97 (`0x0084c800`). **Have:** readers, local prefix,
typed effects, explicit `finish_pass_c`, generic request interpreter and
stage GSF379 check. **Missing:** authoritative dragon actor in ddhost,
effects/callbacks connected to its state, combat damage/results, viewer
body/animation and real ev00 -> fight -> ev01 -> ev05 route. Every player
target and captured occupant needs an owner identity for co-op.

#### Fight end, event prerequisites and NPC actors

The installed `quest/q0000_b00` distinguishes the stage HP trigger from
the quest's other exits. After ev00 finishes it raises SceNo **1025**
(requested start position 3), runs camera 17 with GSF520, then reaches
state 16. Its `EnemyCheck` targets the dragon's persistent killed-state
flag, not a guessed "HP is zero" predicate (order `0x005bef10`, group
count `0x005bf4d0`, per-target predicate `0x005bf3d0`). State 16's condition 20 starts
ev01 for **GSF379 OR FreeFlag[0] (EnemyCheck) OR NOT AreaStayFlag[1]**.
Otherwise condition 8 changes to state 5 only after `mCntState > 17`.
State 5 repeatedly sets GSF378 and its condition 22 adds the independent
**`mCntState > 43`** exit. Thus there is a roughly 17+43-second fallback
from entry into state 16, as well as damage/death and leaving-area exits;
it is not a single mandatory scripted hit or an HP-only fight end.
Times use `0x005bac30`'s `max(delta,0.01)/fps`, with the counter reset at
state entry (`0x00749820`), so the 43-second counter starts anew in state 5.

The separate stage writer remains `aStage200::vf12` **0x0051ccc0**:
while GSF378 enables the dragon branch, capture initial first-life-region
HP and request GSF379 only for **HP < baseline - 4**, strictly. The valid
Arisen's life HP <= 1/missing-life-region with positive effective maximum
HP is an independent branch. **Have:** sourced/tested stage rule and
quest expression interpreter. **Missing:** actual dragon/player snapshots
and authoritative EnemyCheck/area feedback. The current ScriptedHost's
20-second enemy death and fixed q0-bit injections are explicit stand-ins.

At the quest level the player can satisfy the damage/life/area predicates
or let the state timers expire: no particular attack command or successful
hit is required. States 16 and 5 issue EnemyCheck/Flag orders, not a forced
player attack. **Have:** general per-player locomotion and basic sword/
staff attack stepping (`uPlayerBase::vf136` `0x00b6d510`, sword ground
entry/update `0x00b3c570/0x00b3cc20`) in ddsim. **Missing evidence:** the
exact original player kit/input restrictions during SceNo1025, including
whether climbing, guard or skills are admitted here. Generic implemented
combat controls are not proof of those encounter-specific permissions.

The q0 heart-taking state 1 requests **ev01**, whose end raises SceNo
**1050**. State 19 then clears stage LOT0/80 and GSF520 and waits for
`!IsEvent` (condition 13) before state 2 requests **ev05**, end SceNo
**1100**, placement type 2/start position 2. After ev05 comes 08:00
TimeJump, weather, the flag/setup states and q0001. Event completion must
come from the event player (`0x00435fb0`), not treating action completion
or GSF379 as proof that ev01/ev05 finished. This quest/event map was
independently surveyed by codex-1 from the installed graph and interpreter.

q0 initially revives **NPC704..710**. Their installed placement is group
3, records 0..6 of `scr/st200/etc/st200_00m00n_n03`, gated by stage LOT1;
q0 state 9 enables LOT1, and state 8 clears it during cleanup. The NPC
ledger (`etc/item/npclist`, loader `0x00ccac90`) names them Wyndar, Nicolas,
Cortese, Jorgen, Esme, Nessa and Hera respectively. Their authored
`AI/FSM/Npc/st200/quest/q0000_n704_AI` through `n708_AI` and shared
`q0000_n709_n710_AI` are **cAIActionFSMDemo** graphs: Wait, SetMotion,
GotoEx and SetDamage, with no Attack process found. These are scripted
fleeing/reaction victims, not evidence for an invented soldier fight.
They set q0 bits 10..13 as readiness signals and wait for q0 bit 1.
**Have:** general NPC layout/GPL readers and FSM interpreter.
**Missing:** real demo movement, authored damage/reaction/death callbacks
and readiness produced by those actors. Their coordinate/motion records
must continue to come from the player's install. Source reader boundaries:
LOT `0x00cc1790/0x00cc1f60`, NPC ID copy `0x007a32f0`, common NPC GPL
gate `0x0075faa0`. Codex-1 surveyed all NPC LOTs with exact consumption;
group 3 has no authored scene/hour restriction, disabled gate or enabled
unported condition in the checked GPL fields.

The dragon's sole `e00` record/id **1** is `cSetInfoEnemy5800`, sequence
**4**, scale `[1,1,1]`, position approximately
`[14217.957,401.787,-2243.004]` cm and yaw **-0.909578** radians. These
coordinates are an evidence summary, not constants to add to the port:
read `scr/st200/etc/st200_00m00n_e00` at runtime. Dragon_OP first targets
height 1500, then the authored flight point approximately
`[8315.152,1365.787,1401.075]` cm with speed parameter 115 and arrival
radius 300. Both are installed FSM parameters, offset by the general
SetTargetPos process (`0x005a90e0`); an arbitrary beach placement or fixed
flight delay would bypass the source behavior. Grounding/landing still
needs the original area's terrain/contact feedback.

**Verified:** the new installed ddenemyrt regression passed, 1 test, against
the maintainer's actual DDDA.exe/nativePC (compile 0.91 s, execution
2.88 s); `git diff --check` is clean. This batch changes only this Results
section and the installed test. **Not verified:** live original action
selection, player combat controls in this phase, satisfaction of the four
conditional continuation guards, and a playable/repeated/co-op fight.
The listed motion bindings cover sourced entries and the identified
continuations, not every possible nested emitter/magic animation.

**Next briefs:** host resource/phase/action ownership and TargetFly first;
PressAttack with real landing feedback next; BreathWide, authored NPC
demo callbacks and q0 flag/area feedback next; then the dragon combat
driver/callbacks and conditional table actions. The final gate must run
the actual quest/event path, exercise both damage and timed/area exits,
and compare flags/SceNo against the original. No feature completion is
claimed by this research map.
