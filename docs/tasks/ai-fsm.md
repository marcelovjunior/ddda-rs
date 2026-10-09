# ai-fsm: AI state machines (pawns and enemies)

- **Owner:** codex lane
- **Branch:** `task/ai-fsm`
- **Board area:** `ai/fsm`
- **Status:** open

## Goal

Understand and read the data that drives pawn and enemy behaviour, as
the base for pawn scripting and enemy AI (`docs/ROADMAP.md`). Concretely:
a typed graph reader for `rAIFSM`, a survey of the related AI types, and
a map from the FSM's condition and process ids to the exe code that runs
them.

## Evidence and starting points

- `rAIFSM` is XFS: 4,385 instances, 3,073 distinct (Riftstone's count; check it).
  Related: `rAIGoalPlanning`, `rAIPriorityThink`, `rAIEnemyActionParameter`
  (binary `.eap`), `rAISensorExt`. All are in `mtarc::RESOURCE_CLASSES`.
- Dump one with `cargo run --release -p ddtool -- xfs-get <nativePC> <path>`.
  `ddtool xfs-scan` checks that XFS parses across the install.
- XFS objects are MtDTI classes. Their class names are in `mtxfs`
  (`CLASS_NAMES`). The exe's interpreter classes are in `dti-labels.csv`:
  search for `cAIFSM`, `cAI`, `sAI`, and `cPawn`/`sPawnManager` for pawns.
- Riftstone notes: `E:\ddda-re\refsrc\riftstone_formats.md` (AI section).

## Scope

- `crates/mtfsm/**` (new): the graph of states, transitions, conditions and processes
- `crates/ddtool/src/main.rs`: you may add one `fsm-dump` subcommand
- `docs/AI.md` (new)

## Deliverables

1. `mtfsm`, a reader turning an `rAIFSM` into a graph: states, links with
   conditions, and processes with parameters. Tests: every instance in the
   install builds a graph without dangling references (skipped when the
   install is absent).
2. `docs/AI.md`:
   - where the AI files live (per enemy, per pawn);
   - the graph structure;
   - the survey of the related types;
   - how the exe evaluates conditions and runs processes, with addresses.
     An id-to-function table where you can find it; mark guesses.
3. One fully explained example: the smallest enemy FSM you can find, in
   prose plus a dump.

## Done when

Every `rAIFSM` builds a graph, tests pass, and AI.md has the worked
example with traced addresses. Fill in Results and report to `head`.

## Results

(filled in by the agent)
