# Pawn AI (partial: brief paused 2026-10-09)

This is a draft from the pawn-ai brief (claude-5, plus a tracing fork),
paused to save usage. All statements come from static analysis of the
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

**Commands (D/G):**
- Value 8 means none. The data uses 9, 10 and 11; G: 10 = Go, 11 = Help,
  9 = Wait.
- Path (T, 0x005927F0): the mailbox `uCmc+0x4ABC` (written by
  `uPlayerBase::vf98`) → pending (+0x53C) → applied after the delay
  +0x540 → expires after +0x530 (about 20 s, G).

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

## Open questions

- Codes 7, 28, 54 and 65.
- `sGameSys+0xBE2CC`, state 0x2020011, pawn +0x354C.
- The internals of 0x00584110: how goals become GOAP plans and weapon
  attacks.
- The branches of 0x0057F050.
- Command delay and duration values.
- `uPawnIntel` (0x005D1220).
- Chatter selection: not traced (that fork was stopped).
