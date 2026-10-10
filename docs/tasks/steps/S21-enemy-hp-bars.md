# S21: Enemy HP bars above their heads

- **Branch:** `step/s21-enemy-bars`
- **Files:** `crates/ddview/src/hud.rs` (and a new small module if
  cleaner), `crates/mtgui` if the GUI data needs a reader extension
- **Kind:** research, then one feature

## Why

The tutorial's goblin fight needs the enemy health bars the original shows
over each enemy's head (small enemies), and the large-monster bar for
bosses like the cyclops. The viewer has no enemy HUD at all: `hud.rs`
draws only the party's bars (codex-22's game-hud, original GUI layouts).

## Do

1. Find the original's enemy gauge: the GUI resource and class (search the
   `rom\gui` archives and exe strings for the enemy/target gauge, as
   game-hud did for the party bars), what it shows (HP, the red "recoverable"
   part, the HP layers ("bars") of big monsters), where it anchors (a head
   joint plus an offset, or a screen position), and when it appears (always,
   after first hit, when targeted, within some distance).
2. Draw it in the viewer for the fight's enemies, reading HP from the
   host's fight (`ddenemyrt::fight::Fight`, goblins' `fighter.victim`), using
   the original layout and textures like the party HUD does.
3. Label anything untraced.

## Done when

A `--play --goblin --goblin` capture shows a bar over each goblin that
drops when the Arisen hits it, matching the original's look and anchor as
traced; ddview tests pass; the Result lists the GUI resources and rules.
