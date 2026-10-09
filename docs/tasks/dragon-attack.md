# dragon-attack: The dragon attack on Cassardis (st200 ev00 to ev05)

- **Owner:** codex lane
- **Branch:** `task/dragon-attack`
- **Board area:** `story/dragon-attack`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

q0000 from stage 200: event ev00, the dragon camera (GSF 520), the scripted fight "Challenge the Dragon" with its checks (`EnemyCheck`, GSF 378/379, `mCntState > 17` / `> 43`, `AreaStayFlag[1]`), event ev01 (heart taken), ev05, TimeJump and weather, then q0001. The dragon is an enemy in the general runtime, with its own think table, actions and hit volumes for this encounter; the scripted loss comes from the quest data, not special-cased code. Trace which dragon unit and set the stage uses.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddstory/**`
- `crates/ddenemyrt/**` (coordinate with codex-1)
- `crates/ddai/**`
- `crates/ddhost/**`
- `docs/QUESTS.md`
- `docs/AI.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

From stage 200 the fight plays, the scripted loss triggers the heart-taken event, and the state matches the original's q0000 end (quest words, GSF, SceNo 1100), then q0001 starts as on main. Fill in Results and report to `head`.

## Results

(filled in by the agent)
