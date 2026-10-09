# ai-fsm: AI state machines (pawns and enemies)

- **Owner:** codex lane
- **Branch:** `task/ai-fsm`
- **Board area:** `ai/fsm`
- **Status:** review

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

Implemented mtfsm (headless typed graph, ordered states/links/conditions,
nested clusters and heterogeneous named process parameters), fsm-dump
CLI and a reproducible read-only corpus survey. AI.md documents format,
related types, interpreter/operator/process registration addresses and
three measured enemy examples, including the smallest enemy FSM.

Verified 2026-10-09: cargo test -p mtfsm -- --nocapture passes six synthetic
tests and one whole-install integration test: all 4,385 archive occurrences,
3,087 normalized paths, zero graph/reference failures. Exact-byte survey
finds 3,073 distinct payloads, resolving the original count discrepancy:
5,770 clusters, 40,942 states, 42,828 links. The 27 disabled links retain
sentinel ids; 24 archive occurrences have explicit null operands, preserved.
Tests cover missing active references, nested/entry/initial validation,
disabled sentinels, duplicate first-match ids, unknown operators, null
operands, heterogeneous parameters and bad input. cargo check -p ddtool
passes; fsm-dump was run on smallest enemy and NPC GameHour examples.

Executable evidence: ordered link evaluation 0x00E05800; any-state/timer
policy 0x00E06710; frame/process driving 0x00E092A0; state and condition
lookup 0x010C9290/0x0117AAA0; operator dispatcher 0x0117BD30; named process
registration 0x005A8470 through 0x00E088B0. CtrlAction/CtrlMotion map to
0x005A8950/0x005A8890 and share state callback 0x005A8990.
The only translated logic (first-id lookup) has source comments and a
duplicate-id regression test.

Smallest enemy path: ai\fsm\enemy\st100\quest\q0007_goblin0000_ai,
1,128 inflated bytes, one initial state and no actions/links. Nonempty
examples q0002_goburin and q0003_em5000 explain named actions and transitions.
NPC schedule location: ai\fsm\npc\st100\action\go002_morning, conditions
24/25 test GameHour ranges [6,8) and [8,12).

Unknown: null-operand runtime semantics, full coercion/empty-expression
behavior, complete process lifecycle, owner binding and per-pawn resource
loading, numeric action/flag meanings, related binary EAP_/SNR2 formats.
No simulated AI or original-game behavioral comparison claimed. No game
or private-research files were copied or committed; Ghidra read endpoints only.

Next: trace variable/property and owner binding for one concrete enemy/pawn
route, then derive its callbacks with an original-game oracle.
