# S39: NPC quest talk (Reynard's thanks, reward and departure line)

- **Branch:** `step/s39-npc-quest-talk`
- **Files:** `crates/ddnpcrt` (a new NPC QCT selector/effects module),
  `crates/ddhost/src/npcs.rs`, minimal `crates/ddview/src/opening.rs`
  interaction; tests
- **Kind:** research + one feature
- **After:** S32 (Reynard's authored motions and owner retention)

## Context (codex-54, board #4509 / #4520)

After the rescue (q2:20), talking to Reynard (NPC304) is driven by the
installed `etc/questCtrl/n304QuestCtrl`: table 0 QuestTbl row 2 tests
q2:95 && !q2:94 -> Continue(table 3, quest 2) + Repeatable; table 3 row 0
judgment [1,2,20,0,0] (q2:20) -> results conversation 12, reward item 1058
(default count 1), set q2:88 (once); row 1 after q2:88 offers conversation
13 (repeatable departure line). Talk init 0x00B9DC20 checks the actor's
NPC control (+0x713C) -> 0x00792130 -> 0x00792360 (row selection / once /
priority); QCT result dispatch 0x0078FBB0.

## Do

1. Port the NPC QCT selection (0x00792130 / 0x00792360) and the result
   effects this table uses (conversation, item reward, quest bit), with the
   once/repeatable rules.
2. Manual talk (the ordinary interaction) to the retained NPC304 evaluates
   it for the interacting seat: exclusive dialogue ownership as in S23, the
   reward into that seat's bag (S28's bags), q2:88 set once.
3. Tests: row 0 runs once (conversation 12, item 1058, q2:88), a second
   talk gives conversation 13; another seat cannot steal the dialogue; no
   timer or bypass sets anything.

## Done when

An installed host test and the opening route show Reynard's thanks with
the reward after the rescue and his repeatable line afterwards, all from
the installed table; the Result lists the sources.
