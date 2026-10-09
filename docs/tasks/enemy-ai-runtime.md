# enemy-ai-runtime: A headless AI interpreter running an enemy FSM

- **Owner:** claude-9
- **Branch:** `task/enemy-ai-runtime`
- **Board area:** `ai/runtime`
- **Status:** open

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

(filled in by the agent)
