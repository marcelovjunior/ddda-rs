# story-order: ddstory in the traced frame order

- **Owner:** claude-7
- **Branch:** `task/story-order`
- **Board area:** `story/runtime`
- **Status:** review

## Goal

Reorder `Story::tick` to the order traced in `docs/FRAME.md`:
1. quest manager and QCT;
2. the stage mode (events);
3. scenarios (trigger volumes);
4. scripts on line 0.

Rerun the opening ladder test. Add a read-only `Script::state_path()` (or
similar) that `ddhost` uses in its replay hash. This is claude-7's #970
proposal.

## Scope

- `crates/ddstory/**`
- `docs/QUESTS.md`

## Done when

- The opening test passes in the new order. Explain any change in the
  ladder.
- The accessor has a test.
- Fill in Results and report.

## Results

**What changed.** `Story::tick` now runs in the order of FRAME.md 6.2:
1. host reports;
2. quests and QCT (0x00482dd0);
3. stage mode, which takes three steps that used to run elsewhere:
   - the event end step 0x0050c7a0, which used to run with the host
     reports;
   - the vocation menu's close (GSF 372);
   - the pending event request becoming the playing one;
4. scenarios (0x0049a0e0);
5. scripts, stage then quest (line 0; their order there is not defined).

The doc comment on `tick` and QUESTS.md ("Frame order") cite the
addresses.

**Opening test.** Passes unchanged:
- the ladder is 100 ... 1400 and the boots are 28, 0, 31, 1, 2;
- the q0001 words equal the live save;
- the event-player tests also pass.

The ladder did not change. Beyond the asserts, the `opening` example's
output (every request with its frame) is identical line by line for the
old and new orders. None of the opening's QCT rows depends on a
scenario write in the same frame, and an event end is still handled in
the frame its report arrives.

**Accessor.**
- `Script::state_path()` gives the state ids, root first.
- `Script::state()` and `Story::script_states()` give a `ScriptState`
  snapshot: path, running, states, SysCondition, FreeFlags,
  FreeIns/FreeReal, mCntState/mCntFSM, timer register, event step.
- `ScriptState` is `Debug + Clone + PartialEq`, ready for ddhost's
  `StateHasher::debug`.

**Accessor test** (`script_states_replay`):
- two runs give equal snapshots;
- stage scripts come first and quest scripts after;
- `state_path` equals the current states;
- the snapshot moves as the scripts move.

**Not modelled.** The script gate 0x0044D430 (stop flags, talks, stages
0x321-0x324).
