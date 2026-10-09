# player-attacks: the Arisen's attack actions from input

- **Owner:** claude-1
- **Branch:** `task/player-attacks`
- **Board area:** `fight/player`
- **Status:** review

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

Done (branch `task/player-attacks`).

**Input and actions (`ddsim::attack`, docs/MOVEMENT.md "Attacks").**
- `Input::light` / `heavy` (held; presses are rising edges). A player has a
  `Weapon` (sword or staff) and the weapon banks' motions
  (`Sim::weapon_motions`, banks 6/7 as `ddskill`'s
  `WeaponCategory::motion_banks`).
- The ground rows of the transition lists 0x014F83B8 (sword) and
  0x014F8898 (staff), scanned as `0x00ABA690` does:
  - sword: light S1_1 -> S1_2 -> S1_3, heavy LandL;
  - staff: light WandShot, heavy WandStrike1 -> WandStrike2.
- Chains run on the cancel slots of the motions' group 0 events
  (`0x00827F50` / `0x00828030`):
  - slot 1 buffers on bit 0x13 and executes on bit 0x12;
  - bit 31 closes the chain (check 22, uPlayer +0x361C);
  - bit 30 or the motion's end ends the action;
  - slot 0 (bit 0x10) lets the stick cancel.
- Each strike's vf05 plays its bank-7 motion (root motion), turns 45
  degrees a frame toward the target and sets push rank 14 (WandShot 13).
- WandShot plays start / loop / end. It fires a volley at loop frame 2 and
  refires on a light press before 5 volleys (from loop frame 7).
- Stamina: 0x1B8 (80) per sword attack and 0x1BB (30) per staff strike or
  volley, through mode 1 of `0x00B82060`. That is 0 unless the weapon's
  level (item +0x66) is 15 / 20 / 25+ above the Arisen's (x1 / 1.5 / 2).
  So the level-1 starting weapons attack for free. The cost is checked
  against the install with a gap of 15.

**Fight (`ddenemyrt`).**
- The scripted swing is gone: the fight poses the Arisen from ddsim's state.
- Kits: Fighter (Iron Sword) and Mage (starting staff); `Fight::equip`.
- A new attack frees the attacker's relation history.
- The target heading is the nearest live goblin (placeholder).
- The staff's shells come from `param\shl_param_07` group 0, entry 0
  (`ddenemyrt::shell`, COMBAT.md 9.3):
  - collision `m0007` sequence 1 (a 20 cm sphere, attack 1 of `m0007_00`);
  - speed 90 -> 80, dies after 2000 cm;
  - placeholders: spawn point, homing as straight re-aimed flight, die on
    first hit.
- `ddhost/tests/goblin.rs` now presses light instead of scripting the
  swing (head #1130). Its flinch count takes back flinches too: the S1_1
  lunge carries the Arisen around the goblin.

**Headless tests (`ddenemyrt/tests/attacks.rs`, with the install).**
- Sword light combo against a standing goblin:
  - S1_2 starts at S1_1 frame 13 and S1_3 at S1_2 frame 14;
  - each attack hits once from inside its hit window, for 105 (the worked
    example), 85 and 145;
  - a press after bit 31 restarts at S1_1.
- Sword heavy (LandL): 125.
- Staff strike chain: Strike2 starts at Strike1 frame 25; hits of 49 and 69.
- Staff light: two WandShot volleys. Each shell hits the goblin for 27 and
  is attributed to the Arisen.
- Stamina per attack: 80 sword light, 80 sword heavy, 30 staff strike,
  30 per volley (weapon 15 levels up); none at level 1.
- ddsim unit tests (synthetic events) cover:
  - buffering and lost presses;
  - the bit-31 restart and the heavy cancel;
  - the stick cancel;
  - stamina tiers and the stamina > 0 gate;
  - WandShot refire timing.
- ddsim 53, ddenemyrt 16 and ddhost goblin 4 tests pass.

**Push-out.** `0x004797E0` is translated (`ddenemyrt::pushout`, COMBAT.md
9.2) and replaces the 70 cm placeholder.

**Not done / open.**
- Air attacks.
- Skills, and the augment rows (S1_2_1, LandL_1).
- WandShot's turn phase and its aim layer.
- The alternative sword motions (+0x206D bit 3).
- The stamina exemptions and augment factors.
- The target choice `0x00B79090`.
- The shells' real flight and death (`cShlAct*`).
- Where an action's end flag hands back to the wait.
- Keyboard wiring (after m3-lan).
