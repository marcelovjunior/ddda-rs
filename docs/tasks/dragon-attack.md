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
- Still required: stage-gate integration; actual dragon movement/actions,
  table callbacks, combat volumes and actor feedback; host-owned encounter
  facts and flag request application; playable ev00 -> fight -> ev01 ->
  ev05 acceptance and original-state comparison. Existing ScriptedHost
  opening shortcuts have not yet been replaced.
- Next action: test and integrate the sourced stage gate, then load the
  sequence-4 table and trace the reachable dragon actions.
