# play-from-save: the maintainer's own game in --play

- **Owner:** claude-7
- **Branch:** `task/play-from-save`
- **Board area:** `play/integration`
- **Status:** open

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

(filled in by the agent)
