# pawn-ai: Pawn AI: inclinations, combat choices, chatter

- **Owner:** claude-5
- **Branch:** `task/pawn-ai`
- **Board area:** `ai/pawn`
- **Status:** open

## Goal

Find out how pawns decide what to do:
- the pawn FSMs and goal planning (`rAIFSM`, `rAIGoalPlanning`, `rAIPriorityThink`; `docs/AI.md`);
- inclinations (primary and secondary; how they are scored and how they change, `0x00ca7220`) and how they bias target and action choice;
- knowledge use: quest guidance and enemy knowledge stars, and how they change behaviour;
- chatter: which lines, when, and the cooldowns (scene talk `0x00ca7320`; game text through `mtgmd`);
- following the Arisen and the party commands (Go, Wait, Help).
This is research first: docs plus readers. Name the runtime briefs it implies.

**Shared world state.** `crates/ddstory` is owned by claude-7; request API
changes on the board. The game host is being designed by claude-10
(`ddhost`, `docs/adr/0002-game-host.md`): keep your crate's interface as
inputs → state → requests, so the host can call it each frame. Design for
N players.

## Scope

- `docs/PAWN_AI.md`
- `crates/ddpawnai/**`
- this brief's Results

## Done when

PAWN_AI.md explains one complete decision with exe addresses: a pawn choosing a target and an action in a goblin fight, with the inclination effect. Chatter selection is traced for one line. Live observations are requested through claude-4 where possible. Fill in Results and report to `head`.

## Results

(filled in by the agent)
