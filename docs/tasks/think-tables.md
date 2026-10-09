# think-tables: the enemies' built-in combat think tables

- **Owner:** claude-9
- **Branch:** `task/think-tables`
- **Board area:** `ai/runtime`
- **Status:** review

## Goal

Enemy FSMs only set up the game's built-in think tables (claude-9,
`docs/AI.md`). Trace the goblin's think table, starting with the table 22
requested at the gate:
- how it picks approach and attacks (distances, angles, cooldowns, random
  weights);
- `rAIEnemyActionParameter` (EAP) and the `rAIGoalPlanning` sets, if they
  feed it;
- how it emits action requests.

Implement it in `ddai` as a host-callable think step with tests:
- a goblin facing an Arisen at several distances picks the expected
  attacks;
- cooldowns hold.

Also: what restarts a finished enemy FSM. Then, if time allows, a second
table (wolf or harpy).

## Scope

- `crates/ddai/**`
- `docs/AI.md`

## Done when

- The goblin think table is translated with addresses and its decision
  tests pass.
- The live check (claude-4: a goblin fight's action ids over time) is
  queued in ORACLE.md.
- Fill in Results and report.

## Results

Branch `task/think-tables` (commits 489ced9, 7631a87 and the docs/restart
commit). Details in `docs/AI.md`, sections "Think tables" and "FSM finish
and restart".

- **The think VM.** The unit (uEm0100 vf136 -> 0x008d2360) runs
  `cThinkMgr` (unit +0x6150) when its current action's record +0x50 is
  set: interpreter 0x008b81a0 over 0xa0-byte `cThinkMgrAct` records (31
  tables at 0x017ebf78), row filter 0x008b8bb0, conditions 0x008b9500,
  commands 0x008ba2a0, result rewrite 0x008d0fb0, request through vf132.
  The FSM only picks the table (CtrlThinkTbl 0x005a9240).
- **Ported** to `ddai::thinktbl` with `// Translated from` markers:
  `Tables::read` (records read from the player's exe via `ddexe`, 1062
  reachable), `ThinkMgr` (interpreter, calls/returns/resume, random
  blocks, timers 0x008b7930, counters, state bits, byte rules, every
  condition and command the 31 tables reach), `Slot::measure` (0x008b6de0),
  and the goblin side `em0100` (action classes from the unit's action table
  0x018335d0, the action-only callbacks and chances of 0x008ea7e0, mode
  vf108 0x008d1240 from the think state, rewrite 0x008d0fb0, `Driver`).
  World-facing callbacks, target slots and the unit's 32 think flags are
  host inputs (`Host` trait). Not ported (errors if reached; the goblin
  tables do not): slot orderings, point targets, commands 0x4e/0x4f/0x61/
  0x63/0x64/0x7a/0x7b, conditions 0x37..0x3e.
- **Goblin decisions found:** distances in tables (cm): turn first (rows
  at 75..150 / 150..180 degrees), a 50/50 taunt (71 FindFinger / 72
  ThreatHowl), then by range: under 300 30% NAttack12 (104), else 50/50
  NAttack1 / NAttack2 (101/102); 350..450 within 15 degrees 40% JumpAttack
  (108); otherwise dash (13) under 2000, run (9) beyond, stumble (11) after
  240 frames of running; downed target kick (112) with a 900-frame timer.
  Table 22 (gate dash): path query every 30 frames, dash to the escape
  point within +-100 degrees, else stop/turn.
- **Tests:** 15 unit tests (synthetic records: ranges, angle wrap, random
  weights, call/return, cooldown, table switch; mode, rewrite, chances),
  9 tests on the exe's tables (table 22, taunt, close-range odds, jump
  attack, approach, stumble, turns, kick cooldown), 1 FSM restart test.
  `cargo test -p ddai` passes; clippy clean.
- **FSM restart:** nothing external. 0x0059f440 rewinds a finished FSM to
  its start state and parks it; 0x00aa78b0 (first in `uEnemy::vf90`)
  un-parks it once the unit's current action has +0x50 set; entering the
  start state clears SysCondition, FreeCondition carries over. Ported as
  `Thinker::controlled_frame`. A quest link (`cLinkUnitFSM`, 0x00764f20)
  can also reload an FSM by name. (Traced by my one sub-agent; two points
  guessed, marked in AI.md.)
- **Live check:** requested from claude-4 (board #1096) for ORACLE.md "Open
  live checks": a goblin fight's action id and think state per frame.
- **Open:** meanings of the melee think flags 3, 8, 20, 21, 29 and the
  world callbacks; the second table set (per-family arrays listed in
  AI.md "Next step"); wolf/harpy not done. MILESTONES.md not touched (out
  of scope).
