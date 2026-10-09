# play-from-save: the maintainer's own game in --play

- **Owner:** claude-7
- **Branch:** `task/play-from-save`
- **Board area:** `play/integration`
- **Status:** review

## Goal

`ddview <nativePC> --play --save <DDDA.sav>` starts the maintainer's game:
- **The world:** the stage and cell from the save, at the saved position
  (`ddsave`). Lighting and weather come from the save's clock
  (`--clock` / `--weather` logic).
- **The party:** the Arisen and the main pawn dressed from the save
  (`ddlook` + `character::spawn_assembled`, morphs), with weapons, moving
  with the sim's states and blending.
- **Fighting:** keyboard attacks: the light attack on the left mouse
  button or J, the heavy attack on the right mouse button or K (pick
  sensible keys; the camera orbit stays usable). Through `ddsim::attack`.
- **The pawn:** follows and fights through `ddpawnai` / `ddenemyrt`.
- **A goblin:** `--goblin` spawns one nearby, driven by `ddenemyrt::Fight`
  (`ddai` think tables), and you can kill it.
- **Co-op:** the same with `--host` / `--join` (net.rs). The guest's
  Arisen comes from its own `--save` if given, otherwise a stand-in.
- **The HUD:** a minimal one, with HP and stamina bars from the sim. The
  GUI formats exist (`mtgui`), but a simple overlay is fine for now;
  label it.

Keep `--demo` and the old flags working.

## Scope

- `crates/ddview/src/play.rs`, `main.rs`, `character.rs`, `net.rs`
- `crates/ddhost/**`: additive only
- `docs/CHARACTERS.md`, plus a short "How to play" section in README.md

## Done when

- A screenshot from the save shows the dressed party in the lit world at
  the saved spot.
- A scripted `--demo` run kills a goblin with the attack keys, with the
  pawn fighting.
- A two-window co-op run works with it.
- Fill in Results and report.

## Results

Branch `task/play-from-save`; commits 8f92870, 53eecd8 and the docs
commit.

`ddview <nativePC> --play --save DDDA.sav [--goblin] [--demo] [--kit fighter|mage]`:

- **Spot.** The save gives the stage, the cell, `mPlPos` / `mPlAng`, the
  main pawn's `mCmcPos[0]` / `mCmcAng[0]`, the clock and the weather (slot
  `mPlayerDataManual`). The maintainer's save starts in st100 cell 58m49n
  at 12:35, fine weather.
  - Lighting comes from the data for that clock and weather (the
    `resolve_lighting` path).
  - `--clock` / `--weather` override it. A plain `--play` keeps Bevy's
    lighting.
- **Party.** The Arisen and the main pawn are dressed and morphed
  (`character::spawn_look`), with sheathed weapons. They play ddsim's
  states with blends: the common banks, and the weapon's `_co` / `_at`
  lists for attacks.
- **Fight.** It steps at a fixed 30 Hz in `ddhost::Host::tick`'s order:
  hits, pawn pads, sim, attacks and shells, enemies, push-out, volumes.
  - Light attack: J or the left mouse button. Heavy attack: K or the middle
    mouse button. Right mouse still orbits.
  - The kit follows the save's primary weapon: a staff is the Mage, the
    rest the Fighter (`--kit` overrides).
- **Pawn.** `Fight::add_pawn`, ddpawnai through ddenemyrt, on sim seat 4.
- **Goblins.** `--goblin` (repeatable) adds one 6 m ahead.
  - The goblin is driven by `ddenemyrt` (ddai think tables).
  - It is drawn with its own motions.
  - Its materials' 8x8 `d_` placeholder maps are swapped for the full maps
    (*viewer choice*).
- **HUD.** Arisen HP and stamina, the pawn's HP and the nearest goblin's
  HP, drawn as a labelled viewer overlay.
- **Co-op.** `--host` moves the fight into `Host::fight`.
  - ddhost, additive: `Host::tick` merges the pawns' pads, and ddhost
    re-exports `ddenemyrt`.
  - Joining seats are added to the fight.
  - The guest wears its own `--save`, or a stand-in.
  - Not sent to the guest: goblins and HP. Each side sees the other's
    Arisen as a stand-in.
- **Fix.** `character::apply_morph` now writes back only the bones under an
  edit. Before, it garbled the goblin's skeleton.

**Checks:**
- **Screenshot from the save:** the dressed party at the saved spot in the
  lit canyon, the pawn engaging a goblin, and the HUD.
- **`--demo --save … --kit fighter --goblin --goblin`:** both goblins die
  in about 9 s. At the first kill the Arisen had landed 11 hits (1195
  damage) and the pawn 2 (250).
- **With the save's own Mage kit:** the Arisen's staff shots hit for 27
  each and the pawn does most of the damage.
- **Two windows,** `--host 47811 --save … --goblin --demo` and
  `--join 127.0.0.1:47811 --save …`:
  - the guest joined as seat 1;
  - each side saw seat 0, the pawn (seat 4) and seat 1;
  - the host's demo killed the goblin;
  - both screenshots were saved.
- **Old modes:** `--demo` without a save still runs (two base bodies).
- **Tests:** ddview `saved_spot_reads_the_manual_slot`; ddhost
  `the_host_moves_the_pawn_and_it_fights`; ddhost and ddview suites pass.

**Open:**
- Goblins and HP in the net protocol (ddnet, outside this scope).
- The female Arisen plays the male motion lists.
- Weapons are not drawn into the hand during attacks.
- The pawn's inclinations are not read from the save (neutral 500).
- The guest needs the host's place (its own `--save` spot or the same
  `--stage` / `--cell`).
