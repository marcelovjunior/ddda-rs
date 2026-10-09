# viewer-controls: fix ddview's controls and the wall z-fighting (maintainer's report)

- **Owner:** codex lane (PRIORITY 1)
- **Branch:** `task/viewer-controls`
- **Board area:** `play/controls`
- **Status:** open

## The maintainer's report (2026-10-09)

1. **WASD does not work** in `ddview.exe` (`--play`, `--play --save`).
   Find out why (input routing after the `p1_input` / net split in
   `play.rs`, focus, or the demo flags) and fix it.
2. **Heavy z-fighting on wall textures.** The terrain overlay layers sit
   on the same triangles as the base mesh (`docs/SHADING.md`, the
   overlays' `DSZTest` LessEqual). Fix the depth so the layers don't
   flicker: depth bias for the overlay passes, the order between base and
   overlay, or the projection's near/far precision. Check against
   ref001/002, where walls are visible.
3. **Mouse look without holding a button.** The mouse moves the camera
   directly, with the cursor grabbed and hidden while playing.
   - Moving the mouse right turns the camera right (the current direction
     is inverted).
   - Vertical: moving the mouse up looks up, unless the original's
     default is the other way.
   - Respect the pitch limits from play-visuals.
4. **Esc opens a simple pause UI:** release the cursor so the maintainer
   can leave the window, offer Resume and Quit, and Esc again resumes. A
   plain Bevy UI overlay is fine; label it a viewer UI, not the game's.

## Scope

- `crates/ddview/src/play.rs`, `main.rs`, `stage.rs` (the depth fix only)
- the shader files, if needed
- a new `crates/ddview/src/pause.rs`
- README's "How to play" (update the controls)

## Done when

- `--play` and `--play --save` move with WASD.
- Mouse look works without a button, in the right direction.
- Esc pauses with a visible menu and a free cursor, and resumes.
- The z-fighting is gone on the ref001/002 walls (describe before and
  after).
- `cargo build --release -p ddview` passes. Put a timeout on every GUI
  run.
- Fill in Results and report to `head`.

## Results

(filled in by the agent)
