# enemy-spawns: the world's own enemies by area, time and story

- **Owner:** claude-9
- **Branch:** `task/enemy-spawns`
- **Board area:** `world/enemies`
- **Status:** review

## Goal

Replace `--goblin` with the original's enemy placement. This takes over
the Codex brief `enemy-placement.md`; read it for the evidence pointers.
- Enemy-group layouts per stage cell.
- Layout-flag gating: claude-7 found the flags at `sGameSys+0xadee8`; it
  is open which group field names its flag.
- Day and night variants, respawn after days, story-progress variants.
- The spawner (`sSetManager`, which FRAME.md lists in aGameMain's
  managers).

Readers plus a headless runtime that, given the stage, cell, clock, day
and flags, yields the enemies to spawn. Then wire them into ddhost's
fight for the enemies we can run (goblins first; others as placeholders
with their ids).

## Scope

- `crates/ddspawn/**` (new)
- `crates/ddhost/**`, `crates/ddenemyrt/**`: additive
- `docs/SPAWNS.md`

## Done when

- One area near Cassardis is fully explained, with exe addresses: which
  groups, which conditions, the respawn timing.
- `ddhost` spawns that area's goblins from the data in a test.
- `--play --save` shows them, gated by the save's clock and flags.
- Fill in Results and report.

## Results

Branch `task/enemy-spawns` (bb28864, d5cad52, 0254fe5 and the docs
commit). Details in `docs/SPAWNS.md`.

- **Data:** the enemy group lists `scr\st<S>\etc\st<S>_e`
  (`rLayoutGroupParamList`, loader 0x00CC6140 / `cGroupParam::vf09`
  0x00CC4690, area shapes 0x00744960) parse byte-exact on all 199 lists of
  the install (my one sub-agent traced the shape layouts). Group N's units
  are the layouts `st<S>_<m>m<n>n_e<NN>` (format 0x01562268) of the cells
  it lists.
- **Gating (T):** `cLayoutSetCharaBase::vf12` 0x0075FAA0: disabled bit,
  two layout flags (+0x2C low / high u16; flags at sGameSys +0xADEE8,
  `0x0075C110`), the SceNo window (+0x24 / +0x28), the hour window
  (+0x38 / +0x3A). This answers the open question: the flag is the group's
  +0x2C (gpl scalar 17), enabled by +0x20 bit 1 (a second flag by bit 3).
  A few further conditions are named and reported as unported.
- **Respawn (T):** kill records (sSetManager +0x18A98; saved as
  `mGroupDataEnemy`): a kill copies the group's respawn word (0x004A7AD0 /
  0x004A7C10) and marks the unit; each day 0x004A8150 counts types 1, 5,
  6 down and clears them at 0 (type 1 also returns dead units early by a
  75/50/25 % mask); type 0 never returns.
- **Cassardis outskirts (64m53n, next to the Ferrystone arrival in
  64m54n):** by day group 250 (3 goblins, 4-20 h), by night groups 9 and 12
  (7 goblins) and 252 (3 wolves), all before SceNo 7800, back 1 day after
  a kill; post-dragon group 132; flag-7 and SceNo 4700 / flag-150
  variants (table in SPAWNS.md).
- **Runtime:** `crates/ddspawn` (gpl, gate, spawns, respawn, save inputs
  from `DDDA.sav`), `ddhost::spawns` (`spawn_cells`, `spawn_from_save`:
  goblins join the fight, other kinds are placeholders). Tests: the
  install's lists, the outskirts by day / night / story / flag, kill and
  next-day return, the host fight with the area's goblins thinking, and the
  maintainer's save (SceNo 3400, 12:00 -> group 250's goblins).
- **--play --save:** the hookup is play.rs (claude-7's file); I sent
  claude-7 the snippet (board #1257): build
  `ddhost::ddspawn::save::SaveState::from_xml(&save.xml)` and call
  `ddhost::spawns::spawn_from_save(fight, vfs, stage, &state,
  seat0_pos)`, then show `out.goblins` like the `--goblin` ones. Not yet
  seen in the viewer.
- **Placeholders / open:** cell streaming radius (3x3 here), facing from
  `mAngle` y, the save slot choice, unported conditions, respawn types 3 /
  4, NPC and object lists.
