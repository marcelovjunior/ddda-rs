# S57: The three base vocations' starting skills, complete and equipped

- **Branch:** `step/s57-base-skills`
- **Files:** `crates/ddenemyrt/src/skills.rs` (+ `skills/`), `crates/ddprogress`
  (learned/equipped state at the first vocation choice), `crates/ddsim` only
  for a skill action that has no runtime yet; tests (headless first)
- **Kind:** feature (a must-have of the current goal: "the 3 base classes")

## Context

`SkillSet::load` supports four base-level skills (sword and mage) from an
explicit support list. docs/VOCATIONS.md "Learning rules" traces the
starting skills: weapon skills with cost 0 at rank 0 (ids 40, 104, 152,
210, 220, 234, 272, 310, 350, 359, 402). The six active slots per weapon
category live at +0x120 + cat*0x18; slots 0..2 belong to the primary weapon,
3..5 to the secondary. Strider bow mode exists (S25-S27); daggers, the bow's
skills and the shield's are not all in the runtime.

## Do

1. Trace what the first vocation choice (0x00673740, `ddprogress::change`)
   and the new-game setup leave **learned and equipped** for Fighter
   (sword + shield), Strider (daggers + shortbow) and Mage (staff): which of
   the eleven starting ids, in which slots. Check it live on the running
   original (read-only, oracle rules: back up the save, never write it) if
   the source leaves doubt.
2. Make every one of those skills run: action class, motions (the per-skill
   `rMotionList` banks, 0x00b5b400), stamina (the weapon stamina tables),
   hit volumes and damage through the existing fight. Reuse the four
   existing skills' path; no stand-in motions or timers.
3. Seat each Arisen's equipped skills from the traced state (per profile,
   co-op), and the skill inputs as the original binds them.
4. Tests: per vocation, each equipped starting skill starts, plays its
   installed motion to its end, costs its stamina and has its **native
   effect**: damage skills hit a goblin; Anodyne (220) heals with the white
   cap and no attack damage; Shield Summons (272) provokes as the source
   says. No skill gets an effect it does not have (codex-81 #8501).

## Done when

All three base vocations start with the original's equipped skills, each
works in a headless installed test, and the Results list every skill with
its source addresses. One bounded GPU check from the route-v2 FieldSettle
checkpoint per vocation (`DDVIEW_DEMO_JOB`), batched into one run.

## Head scope note (2026-10-10, #8501/#8504)

Starters traced by codex-81 (0x006749A0/0x00674A60): Fighter 40 + 272,
Strider 152 + 350, Mage 210 + 220. Authorized narrow edits: ddsim skill
modes (dagger/shield/bow), ddenemyrt fight.rs hookup, and the host's
progression-to-equipped sync; viewer changes come to head as a patch.
