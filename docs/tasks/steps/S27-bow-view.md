# S27: The bow, part 3: in the viewer

- **Branch:** `step/s27-bow-view` (after S26)
- **Files:** `crates/ddview/src/play.rs` (inputs, motions of bank 9),
  `main.rs` key binding, the follow camera's aim mode if traced, a shell
  drawing like the staff's
- **Kind:** one feature

## Do

Bind the bow-mode toggle and the shot to keys (config.ini names, as the
other bindings), play the bow motions (bank 9, `m0006_at`) on the Strider,
draw the arrows, and switch the camera to the original's aim view if the
bow action changes it (trace `uAimCheck` and the camera mode it requests).

## Done when

`--play --kit strider --goblin` lets a player draw the bow, aim and shoot
the goblin; a capture shows the aim pose and an arrow in flight; ddview
tests pass.
