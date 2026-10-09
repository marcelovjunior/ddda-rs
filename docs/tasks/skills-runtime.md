# skills-runtime: Vocation skills in combat

- **Owner:** codex lane
- **Branch:** `task/skills-runtime`
- **Board area:** `fight/skills`
- **Status:** review

## Goal

Make equipped skills usable: the `ddskill` catalogue (action ids, the cPlActWpn* classes, motion lists), stamina costs, and effects, starting with the Fighter (sword skills) and the Mage (staff spells: Ingle, Anodyne, and Levin if reachable). Spells are shells (`shl_param`), like WandShot.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs must not grab the mouse or pause on focus loss.

## Scope

- `crates/ddsim/**`
- `crates/ddenemyrt/**`
- `crates/ddskill/**`
- `docs/VOCATIONS.md`
- `docs/COMBAT.md`
- this brief's Results

Shared viewer files (`play.rs`, `main.rs`) need a board claim and
coordination with the agent that holds them.

## Done when

Two skills per vocation work headlessly against a goblin with traced costs and damage, and from keys in `--play`. Fill in Results and report to `head`.

## Results

Implemented base-level Broad Cut (41), Tusk Toss (43), Ingle (210) and
Levin (212). `ddskill::magic` parses all four installed magic action
lists; source-labelled phase/cost helpers and per-player `ddsim::Skills`
drive installed skill motion banks. `ddenemyrt::SkillSet` and `SpellWorld`
load collision, attack and shell parameters at runtime. Ingle's traveling
shell creates its impact child; Levin emits delayed strikes. Fight
integration uses the casting player's body and attributes shell hits to
its shooter. Root codex-1 supplied reviewed Fight hooks, integrated as
c2809c7/43871bd. Sword push ranks and action interruption are in fd9273e.

Offline viewer keys: P1 digit 1–6, P2 Numpad 4–9. Saved Arisen slots retain
their order; unsupported skills are not replaced. No-save preview slots
1/2 provide the chosen kit's pair (`--kit fighter` or `--kit mage`). Skill
ID is part of the animation selection/cache. Pause, opening and network
sessions do not accept these keys. The viewer carries the coordinated HUD
checkpoint 49c4c6e (codex-22's 86f981c), with the old overlay retained.

Verified: full `cargo test -p ddenemyrt -p ddsim -p ddskill`: 26 + 57 + 26
passed, one existing debug test ignored. Core library Clippy with
`-D warnings` passed. Viewer release tests: 24 passed, including three
skill tests (save slot order/sentinels, distinct seats/press edges,
actual digit bindings through all four installed actions and costs).
Release viewer build passed. A bounded `--play --kit mage --unattended`
launch loaded slots 210/212, produced a screenshot and exited normally;
this was a startup/idle check, not a visual spell comparison.
The Fight regressions damage installed goblins with each skill, test
caster seat 1 and unchanged seat 0, and retain all basic-attack tests.

Limits: no original-game comparison; Tusk Toss's stationary target is
placed from installed attack geometry and a target 100 cm straight ahead
is still missed. Base levels only; equipped augments and weapon-level
penalties are not applied. Anodyne, manual aim/lock selection, chant
sharing, cancel windows, shell VFX/audio, wall impacts and LAN skill
messages remain open. No game install/process writes were performed.

Next action: compare Tusk Toss targeting/root motion and base spell
release timing against the original under the live-game protocol, then
connect skill levels/augments and network commands. Detailed sources and
limits are in `docs/VOCATIONS.md`.
