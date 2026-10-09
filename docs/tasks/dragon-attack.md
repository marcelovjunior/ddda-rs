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
