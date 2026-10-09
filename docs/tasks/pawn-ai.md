# pawn-ai: Pawn AI: inclinations, combat choices, chatter

- **Owner:** claude-5, then claude-9 (runtime)
- **Branch:** `task/pawn-ai`
- **Board area:** `ai/pawn`
- **Status:** review

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

Research (claude-5, 75ba481, da0d4e7): readers for `rAIPriorityThink` and
`rAIGoalPlanning`; the priority build, inclinations and follow table in
`docs/PAWN_AI.md`.

Runtime (claude-9, branch `task/pawn-ai`, merged with main; commits ea94d40
and the tests/docs commit):
- **Decision with addresses** (`docs/PAWN_AI.md`): the think walk
  0x00583CF0 / dispatcher 0x005851D0; battle follow (code 4, 0x00585D00)
  vs the battle entry (code 7, 0x00585880 -> 0x005907F0 -> 0x0058FDE0); the
  nearest target (0x005B5020); meta goal 4 -> weapon goal by the weapon
  type (0x005A4B70: sword -> 52 `WpnSwordAtk`); goal and plan picks by
  weight (0x005A3AA0, 0x005A4A00); the sword combo state machine
  `cCmcComboActTemplate::vf18` 0x0054C8B0 with the combo table 0x0152E3B0,
  the ranges of `AIPlActParamSword` (0x0047ED00) and the press gate on the
  attack cancel slot (0x00536A90). The pawn plays the player's actions
  through a virtual pad (`uCmc::vf200` 0x00B537D0).
- **Inclination effect:** Guardian/2 drops the battle follow entry from
  level 1 to 4, behind the battle entry, so a far Guardian pawn fights
  instead of regrouping (test `guardian_fights_before_following`); the
  follow distance by inclination and command (0x00598390, table
  0x0152DAB8).
- **Runtime:** `ddpawnai` (`incl`, `think`, `plact`, `pawn`) and
  `ddenemyrt::pawn` (`Fight::add_pawn`, `Fight::pawn_inputs`): a main
  pawn follows the Arisen and kills a goblin headlessly and
  deterministically (`tests/pawn.rs`). Placeholders, marked in code: the
  battle flag (a goblin within 30 m), the pawn's values (the level-1
  Fighter kit), the out-of-battle follow range, range tier 0, enemy radius
  0; the Dly combo and the L combo's second step are left out (ddsim
  does not play those actions); the early combo stop is not ported.
- **Not done:** chatter (not reached); the staff pawn (`cCmcMagicUserCombo`
  not analysed; a Mage pawn takes no battle goal); skills; the goblin's
  `rAIEnemyActionParameter`; the battle flag's writer and goal
  pre-emption. Live check requested from claude-4 (board): the main pawn's
  actions, pad and goal during a goblin fight, to compare with
  `Pawn::history`.
- **Implied runtime briefs:** the staff and skill interfaces; support
  goals (0x10 / 0x44) and the enemy action parameters; party commands
  from player input; chatter (0x00CA7320).
