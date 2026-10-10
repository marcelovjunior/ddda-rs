# S37: Experience from kills, level-ups in the fight, the level-up notice

- **Branch:** `step/s37-kill-experience`
- **Files:** `crates/ddenemyrt` (the kill's award), `crates/ddhost`
  (`award_exp` from fight deaths; levels feeding the fight's HP/stamina),
  `crates/ddview` (the level-up GUI), tests
- **Kind:** research + one feature

## Context

`Host::award_exp` (0x00443470 via `Progression::award`) and the per-frame
level rise (0x00B5BD20) exist and are tested (`ddhost/tests/session.rs`),
but nothing calls them in a fight: killing the opening's goblins gives no
experience, and a level-up would not change the Arisen's fight HP or
stamina. The tutorial levels the Arisen on its first kills.

## Do

1. Trace where an enemy death awards experience: the enemy's exp value
   (its stat/param record; the goblin's in the install), who receives it
   (party members present, the `Share` split already in `Progression::award`,
   co-op seats), and when (at death, at the death action's end?).
2. Award it from the fight's kill events through `Host::award_exp`.
3. On a level-up, apply the new level's stats (the `rPlLevel` row of the
   vocation held, docs/VOCATIONS.md) to the fighter: max HP / stamina
   and current values as the source does (full heal or not).
4. Viewer: the level-up notice (`uGUICharacterLevelUp` / `uGUILevelUpBase`,
   strings 0x01582C04 / 0x01582BDC; `uGUIJobLevelUp` for vocation rank)
   from the installed layout, like the cockpit in `hud.rs`.
5. Tests: killing goblins raises a level-1 Arisen's experience by the
   sourced amount, a level-up raises max HP, and each co-op seat gets its
   own share.

## Done when

In an installed host test the opening's goblins give the sourced
experience and a level-up changes the fighter's HP; the viewer shows the
level-up notice; the Result lists the sources.
