# player-attacks: the Arisen's attack actions from input

- **Owner:** claude-1
- **Branch:** `task/player-attacks`
- **Board area:** `fight/player`
- **Status:** open

## Goal

Replace the scripted swing in the goblin fight with real player attacks.

**Input:** light and heavy attack buttons in `ddsim::Input`.

**Attack actions per weapon** (the `cPlActWpn*` classes, through
`ddskill`'s catalogue):
- the chains of the sword and the mage's staff (the maintainer plays a
  mage): next-attack windows from the motion events, motions from the
  weapon banks (`m000N_at`);
- stamina use per attack;
- hit volumes and attack data through ddcombat / ddenemyrt.

**Also:** trace the character push-out rule (0x004797E0), replacing the
70 cm placeholder.

## Scope

- `crates/ddsim/**`
- `crates/ddenemyrt/**`
- `crates/ddskill/**`: API additions only
- `docs/MOVEMENT.md`, `docs/COMBAT.md`

Not `crates/ddview/play.rs` (claude-10). Wiring the attack keys into
`--play` comes after m3-lan.

## Done when

- Headless tests show:
  - a light combo on the sword and on the staff, with their frame
    windows and damage on the goblin;
  - a heavy attack;
  - stamina use per attack.
- Push-out is translated.
- Fill in Results and report.

## Results

(filled in by the agent)
