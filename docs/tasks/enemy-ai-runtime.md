# enemy-ai-runtime: A headless AI interpreter running an enemy FSM

- **Owner:** claude-9
- **Branch:** `task/enemy-ai-runtime`
- **Board area:** `ai/runtime`
- **Status:** review

## Goal

A runtime for the AI graphs read by `mtfsm`, using the interpreter addresses in `docs/AI.md`: ordered link evaluation `0x00E05800`, any-state and timer policy `0x00E06710`, the process driver `0x00E092A0`, the operator dispatcher `0x0117BD30`, and named process registration `0x005A8470`. Put it in a new crate `ddai`, with conditions bound to an input snapshot (the actor's own state, targets and distances, the world clock) and processes emitting action requests (CtrlAction, CtrlMotion, ...). Start with the goblin's FSMs (e.g. `q0002_goburin`, and the smallest example in AI.md). Unimplemented operators or processes return explicit errors. `ddstory` already runs cFSMOrder scripts on a related interpreter; share or align with it, and agree with claude-7.

**Shared world state.** `crates/ddstory` is owned by claude-7; request API
changes on the board. The game host is being designed by claude-10
(`ddhost`, `docs/adr/0002-game-host.md`): keep your crate's interface as
inputs → state → requests, so the host can call it each frame. Design for
N players.

## Scope

- `crates/ddai/**`
- `docs/AI.md`
- this brief's Results

## Done when

A goblin FSM runs from synthetic inputs (idle → notice the player → approach → attack request) with the state sequence asserted. Every operator and process it touches is translated with its address. Fill in Results and report to `head`.

## Results

claude-9, branch `task/enemy-ai-runtime`. Details: `docs/AI.md`, section
"Runtime (`crates/ddai`)".

**Built:** `crates/ddai`, interface inputs -> state -> requests:
- `cond`: condition trees (0x0117c040 / 0x0117bd30 and the typed operator
  routines), ported from `ddstory::cond`.
- `core`: the cBaseFSM core stepper (0x00e092a0, 0x00e06710, 0x00e05800)
  generic over an `Owner` trait (state-change hook, variable lookup,
  update/state callbacks with a host environment), ported from
  `ddstory::script`. Agreed with claude-7 (board #957) that ddstory may
  move its cFSMOrder owner onto it later.
- `think::Thinker`: the cThinkFSM / cAILevelAbstractFSM owner: vf06
  0x007a9cb0 per frame, vf05 0x007a9da0 state-change hook, the variables
  from vf03 0x007a82d0 that goblin graphs use, and the processes CtrlAction,
  CtrlMotion, CtrlThinkTbl, SearchSetTarget, SetEscapePos, SetFreeFlg,
  Timer, Set Check Pos, Set Kill Owner, Finish, Kill (each with its
  registered callback addresses). `Inputs` carries the actor, the
  players, the world offset, flag words and target candidates; `Request`
  is Action, Motion, ThinkTable, EscapePos, FreeCondition, Target,
  KillOwner. Anything else is `Error::Unimplemented`.

**Newly traced:** process registrations of cAILevelAbstractFSM (0x005a8470),
cThinkFSM (0x007a74f0) and cBaseFSM (ctor 0x007495f0); the meaning of
SysCondition bits 0x20000000 / 0x40000000 (request done or target found /
no target); the setting-bit-2 rerun rule (a state reruns in the same frame
until a transition fires); empty operations are true (0x0117c040);
`mFSMAttribute` bit 2 = per-cluster work memory (0x00e09750). The enemy
graphs are short scripts that configure the native think tables and then
Finish; the approach and attack themselves run in the think tables.

**Tested:** `cargo test -p ddai`: 3 unit tests (operators; synthetic
graphs for the transition delay, absent conditions, from-all priority and
the rerun bound) and 4 install tests (skip without the game): the q0007
gate goblin from synthetic inputs, idle ("6秒後", 6 s on Timer[0]) ->
approach ("ダッシュ": EscapePos + ThinkTable 22) -> notice and threat
("発見～威嚇吠え": Action 72), with the state sequence asserted;
q0002_goburin's CtrlAction completion; q0003_goburin's target search;
32 of the 35 goblin graphs run 300 idle frames, the other 3 stop at
explicit errors (Check Area Hit, Set QuestFlag). Clippy clean.

**Not done / guessed:** native think tables; who restarts a finished FSM
(the test restarts it); actor +0xe0 as world position (guessed); target
group membership and the search rule for groups other than 1; nothing
compared live.
