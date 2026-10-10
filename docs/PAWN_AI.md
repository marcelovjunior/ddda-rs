# Pawn AI

Research by claude-5 (priority think, inclinations, follow) and claude-9
(the battle goal, the plan, the sword combos and the runtime). All statements come from static analysis of the
2017-12-13 Steam exe. **T** = traced in code, **D** = from data,
**G** = guessed. The readers for `rAIPriorityThink` and `rAIGoalPlanning`
are in `crates/ddpawnai` (commit 75ba481): 99 priority tables and 186 GOAP
sets parse.

## Ownership (T)

`uCmc::vf115` (0x00B54520) loads three things into the AI object at
`uCmc+0x2E64`:
- the sensor target `AI\SensorTarget\cmc`;
- 69 GOAP resources at +0x68, from the (goal id, path) table at 0x014F3430;
- `cAIPriorityThink` at +0x70 (created by 0x0059E590), whose resource
  `AI\PrioThink\cmc` sits at its +8.

It also loads `AI\Character\Pawn\pawn_act_param`.

**Goal ids (D):** 0 Wait, 1 Follow, 12 HelpAction, 20 Goto, 37
BattlelAssist, 52-57 weapon attacks (sword, longsword, dagger, staff,
shield, bow), 88 MultiPlier.

## Priority think (T)

**Resource layout (0x00CA8740 / 0x00CA87C0):**
- +0x60 is the type;
- then 6 groups × 8 levels of entries at `res + 100 + g*0xA0 + l*0x14`;
- the groups, in order: QUEST, PL_Party, Situ_Personal, Enemy,
  Wait_Follow, Etc.

**Entries.** A `cPrioParam` (0x00CA7AD0) holds a sensor, a code, a
category, an object id, a list of `cPersonalValue` and a list of
`cOrderValue`.
- `cPersonalValue` has an S32 add, an F32 add, a stop flag and a list of
  `cIsPersonal`.
- `cIsPersonal` is an (inclination, required status) pair.
- `cOrderValue` is a (level, command) pair.

**The build (0x005AC240).** It runs at start, on a command change and when
a command expires. For each entry with type 1, starting at `lvl = l`:
1. Each `cPersonalValue` whose checks all pass adds to `lvl`, stopping
   after the first if its stop flag is set. A check passes (0x0057EF40) when
   the inclination is an active primary or secondary, and the status
   matches (0 = any).
2. If the current command (`situation+0x52C`) is not 8, a matching
   `cOrderValue` sets `lvl` to its level.
3. The entry goes into the list for (g, lvl), or for (g, l) if lvl falls
   outside 0..7.

**Order (comparator 0x005AC510).** Lower level wins, then the lower
group, then the lower index. So level 0 is the highest priority, and QUEST
beats PL_Party beats … beats Etc.

**Code to handler (dispatcher 0x005851D0):**

| Code | Handler | Notes |
|---|---|---|
| 4 | 0x00585D00 | follow the Arisen |
| 7 | 0x00585880 | |
| 12 / 52 | 0x00589C10 | gated by `sGameSys+0xBE2CC`, probably the battle state (G) |
| 28 | 0x0058C2B0 | |
| 54 | 0x0058E500 | |
| 55 | 0x0058E8B0 | requests Follow |
| 65 | 0x0058F560 | |

The handlers request goals through `0x00584110(situation, table, 0, goal,
target, …)`:
- **Code 12/52:** if the Arisen is in state 0x2020011 within 15 m, request
  Goto (20) on the sensor target (0x005B2670).
- **Code 65:** when pawn +0x354C == 9, request MultiPlier (88) on the
  nearest valid enemy.

**Target ranking (0x005B5020, comparator 0x005B5160):** by distance,
nearest first. Inclinations change which situation and goal wins, not the
distance weighting.

## Inclinations

| Index | Name (from `personalityName_eng` and the elixir item order) |
|---|---|
| 0 | Scather |
| 1 | Medicant |
| 2 | Mitigator |
| 3 | Challenger |
| 4 | Utilitarian |
| 5 | Guardian |
| 6 | Nexus |
| 7 | Pioneer |
| 8 | Acquisitor |

**Status (T, 0x0057FC50):**
1. Sort the 9 values in descending order.
2. Top value ≥ 750 → status 2; ≥ 600 → status 1.
3. Second value ≥ 600 → status 1.
4. At most 2 are active, plus a 3-entry history.

This matches the save: Eloyse has 700 and 660, both status 1. Values are
clamped to [0, 1000] (0x0057EE00). The binding to the pawn record is at
0x0057EC40 (`cCmcInfo+0x14BC + k*0xC`).

**Changes (T):** only for the main pawn (`cCmcInfo+0x7F4 == 1`) and only
while SceNo < 9000.
- **Elixirs** (items 1520-1528, k = 0..8; 0x0057F810):
  - if k is not active, `0x0057F050(k)` sets k to 650 and lowers the
    others (the exact branch constants are unresolved);
  - if k is active, k gains 100, and every other inclination ≥ 575 loses 25.
- **Neutralizing Elixir** (item 1529): all set to 500, history cleared.
- **Talk choice** (0x00519090 state 5): picks from two candidates
  (0x0057FA80).
- No combat-driven drift was found.

**Data effects (D; full dump in the fork's private scratch):**
- Guardian/2 raises follow (Wait_Follow-01 code 4) by 3.
- Scather/2 gets +5 on Etc-04 codes 54 and 55. That gives level 9, out
  of range, so the entries fall back to level 4: a data quirk.
- Nexus, Medicant and Guardian lower the PL_Party support codes (13, 15).
- Acquisitor lowers the gathering codes.

**Commands (T/D, S36 correction):**
- Value 8 means none. Native sender 0x00B768E0 and the installed cockpit
  labels establish 9 = Go, 10 = Come, 11 = Help; left/right both select Help.
- Path (T, 0x005927F0): the mailbox `uCmc+0x4ABC` (written by
  `uPlayerBase::vf98`) → pending (+0x53C) → applied after the delay
  +0x540 → expires after +0x530. Constructor 0x00581C20 initializes duration 600 actor frames
  and delay 0; the feedback window is 6 actor frames.

**Follow distance (T, 0x00598390; table 0x0152DAB8).** The mode depends on
the inclination and the command:

| Mode | Max / min (cm) |
|---|---|
| 0 | 500 |
| 1 | 750 / 500 |
| 2 | 1500 / 750 |
| 3 | 2500 / 1500 |
| 4 | 3500 / 2500 |
| 5 | 7000 / 3500 |
| 6 | 5000 / 3500 |

**Talk level (0x0057EFF0):** TALK value < 200 → 0, < 400 → 1, < 600 → 2,
< 800 → 3, else 4.

## The think walk (T)

`0x00583CF0` walks one (group, level) list; its entries run their code
handlers through `0x005851D0`. A handler returns 0 (not taken), 2 (it
requested a goal: the walk ends) or 1 (applicable: the loop takes the
generic battle path, below). Handlers request goals with `0x00584110` /
`0x00584250` (`cAIGoalPlanning::cGoalInfo`, queued by `0x005A32A0`).

The battle flag `sGameSys +0xBE2CC` gates codes 4, 12 and 13 (battle
only) and 5, 16 (outside battle). Its writer is not traced.

| Code | Entry | Handler | Effect |
|---|---|---|---|
| 4 | Wait_Follow-01 | 0x00585D00 | battle: follow (goal 1) when the Arisen is farther than the mode's maximum (`0x00597E40`) |
| 5 | Wait_Follow-06 | 0x00585FA0 | outside battle: follow (goal 1), ranges from `0x00598670` / `0x00597FF0` |
| 6 | Wait_Follow-07 | 0x00586200 | wait (goal 0) on the sensor target |
| 7 | Enemy-04 | 0x00585880 | battle: returns 1, then `0x005907F0` -> `0x0058FDE0` |
| 12 / 52 | Enemy-02 / -05 | 0x00589C10 | Goto (20) when the Arisen is in action 0x2020011 within 15 m |

**Follow (T, 0x00598390).** Mode from the inclination list (+0x118 count,
entries +0x120 / +0x128: inclination at +0, status at +8) and the
command: Pioneer/2: command 10 -> 2, else 3 (4 with command 9);
Guardian/2 or Medicant/2: 2 (3 with command 9); otherwise command 10 ->
1, else 2 (4 with command 9); a large battle target (`0x005985A0`)
makes it 5 outside the command-10 cases; area 0x262 makes it 6. The row
of the table above (pointer 0x017F4170 -> 0x0152DAB8): follow beyond
+0x00 (max). *Guessed*: the move ends inside +0x10 (min).

## From the battle entry to a sword swing (T unless marked)

1. `0x0058FDE0` reads the first target of sensor list 6 (nearest first,
   `0x005B5020` / `0x005B5160`) and its offered goals: the enemy's
   `rAIEnemyActionParameter::cFindResult` at target +0x23C0 (entries'
   goal id +0x74, usually 4; 0x10 when uCmc +0x26A0 bit 2), filtered by a
   vocation mask (+0xB0 vs uCmc +0x354C), situation masks and live parts.
   The goblin's own entries are not decoded.
2. `0x005A4B70` expands meta goal 4 by the pawn's weapons (type = high
   byte of uCmc +0x3554 / +0x3558): main 1, 2 -> 52 `WpnSwordAtk`; 3, 7 ->
   53; 4 -> 54; 5, 6 -> 55 `WpnWandAtk`; off hand 8, 9 -> 56; 10..12 -> 57.
   0x10 adds the support goals (37 BattlelAssist, 6, 7, 8, 79, 77, 36).
3. `0x005A2960` -> `0x005A3AA0` picks a goal: priority levels 8 down to 0,
   the first level with a feasible goal, then a weighted random pick
   (`0x005A4A00`, `0x00D00690() % sum`).
4. The plan (`0x005A3F20` / `0x005A41E0`) is a precomputed chain; for
   `WpnSwordAtk` one step (premise 0 -> 1) with twelve interfaces. A usable
   one is picked by weight (`vf16`; `cCmcComboActTemplate` 0x0054CEE0:
   50). Plans aimed at the nearest target are kept (`0x005A4820`).
5. `0x005A2A00` runs the step; `0x0056C940` creates the interface; each
   frame `uCmc::vf200` (0x00B537D0) runs it (`0x0056C9F0` -> vf06 -> vf18)
   and copies its virtual pad (stick angle, buttons) into the unit before
   `uPlayerBase::vf200`: the pawn plays the player's actions.

**Sword combos (`cCmcComboActTemplate`).** Constructors: `cCmcSwordCombo`
0x00539C20 (type 0, wait 60), `cCmcSwordComboDly` 0x00539D00 (type 1,
wait 60), `cCmcSwordComboL` 0x00539DE0 (type 2, wait 30). Combo table
0x0152E3B0 (command, steps, count; a step is an action id and a stop
chance): type 0: command 0xA, 0x01010000 / 0, 0x01010001 / 0.5,
0x01010002 / 0.5; type 1: command 0xA, 0x01010000, 0x0101000A..C; type 2:
command 0xE, 0x01010004, 0x0101000D. `vf18` (0x0054C8B0):
- approach (`0x00544300`) until the horizontal distance is within the
  command's range;
- then each press: the previous step's action must play (uCmc +0x2DD4),
  the range check `0x0054B0A0` (min - 50 .. max + target radius + 50, the
  `Enable` pair when larger) with the height check `0x00542580`
  (`RangeMinY` / `RangeMaxY` widened by 50) must pass (else fail), and the
  press (`0x00541A30` -> `0x00541D60`: command 0xA -> button 1, 0xE ->
  button 2) is taken only while the attack cancel slot executes
  (`0x00536A90`, unit +0x2588 + (slot + 1) * 0x1C == 2); a timer of
  `wait` frames runs between presses (fail at 0);
- an early stop (`0x00548240`, a proximity test for vocations 1, 4, 5, 7,
  then `0x00422FA0() <` the step's chance) is not ported.

**Ranges.** `0x0047ED00` picks the record by command id: 0xA..0x45 from
the bank at sPlayerManager +0x8D4 (*guessed*:
`AI\AIPlayerActionParameter\AIPlActParamSword`), record `command - 0xA`,
tier 0..2 (`_Lv1`, `_Lv2`, `_LvEX`, by `0x00780BA0`). Command 0xA, Lv1:
0..150 cm, height -100..250. A failed approach blocks the goal on that
target for 30 (`0x00542A60` -> `0x00594B50`, *guessed*).

## Runtime (`ddpawnai::pawn`, `ddenemyrt::pawn`)

Inputs -> state -> a virtual pad, one pawn per object, N pawns per fight:
- `ddpawnai::incl` (0x0057FC50, 0x0057EF40), `ddpawnai::think::build`
  (0x005AC240), `follow_mode` (0x00598390), `ddpawnai::plact` (the range
  records);
- `ddpawnai::pawn::Pawn::step`: when no goal runs, the walk takes the
  first of code 4 (battle follow), 5 (follow outside battle;
  *placeholder*: the battle table's row 2), 6 (wait) and 7 (battle: the
  nearest live enemy, the weapon goal, a weighted combo pick); other codes
  are not ported and are listed in `Pawn::unported`. The goal runs to its
  end (*assumed*: pre-emption is not traced). The combos are
  `cCmcSwordCombo` and `cCmcSwordComboL` (first step only):
  `cCmcSwordComboDly` and `0x0101000D` are actions ddsim does not play;
- `ddenemyrt::pawn`: `Fight::add_pawn` makes the pawn a ddsim player with
  the Fighter kit (*placeholder*: the Arisen's level-1 values) that the
  goblins also target; `Fight::pawn_inputs` returns its pad as ddsim
  input before the step. The battle flag is *placeholder*: a live goblin
  within 30 m of the pawn or its Arisen. The press gate reads ddsim's
  attack slot state.

Tests: `ddpawnai` unit tests (statuses, follow modes) and the build on the
install (`pawn_priority_build`); `ddenemyrt/tests/pawn.rs`: the pawn kills
a goblin with its sword combos, follows the Arisen first when it is 25 m
away, a Guardian/2 pawn fights first instead (the inclination effect), and
two runs give the same digests.

## Open questions

- Codes 7, 28, 54 and 65.
- `sGameSys+0xBE2CC`, state 0x2020011, pawn +0x354C.
- The goblin's `rAIEnemyActionParameter` entries (goal 4 or 0x10, masks).
- The staff (`cCmcMagicUserCombo`, 0x00557040..) and the skills.
- The branches of 0x0057F050.
- Full support-goal handlers, authored shout playback and chatter selection.
- `uPawnIntel` (0x005D1220).
- Chatter selection: not traced (scene talk 0x00CA7320 not reached).
- The battle flag's writer; when the think runs and pre-empts a goal.
