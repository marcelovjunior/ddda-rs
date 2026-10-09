# viewer-controls: fix ddview's controls and the wall z-fighting (maintainer's report)

- **Owner:** codex lane (PRIORITY 1)
- **Branch:** `task/viewer-controls`
- **Board area:** `play/controls`
- **Status:** controls ready for review; moving-camera wall validation pending

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

Implemented on `task/viewer-controls` by codex-17:

- Preserve short WASD presses (including press/release in one render frame)
  until a fixed simulation tick consumes them. Manual movement/jump/attack
  now takes over `--demo`, which previously overwrote it. The held-WASD
  failure reported by the maintainer was **not reproduced** in the current
  source: the basic key-to-seat mapping already worked. These are specific
  sampling/demo fixes, not a proven explanation for every reported failure.
- Direct mouse orbit without a button, flipped horizontal/vertical input,
  with the play-visuals limits corrected to 60 degrees up / 30 down. Existing
  0.004 radians/pixel sensitivity is explicitly a viewer choice.
- Viewer-only Resume/Quit menu: Esc toggles it, focus loss opens it, cursor
  hidden/locked in play and visible/free in the menu. Offline simulation and
  animation pause. Network sessions keep pumping and advancing, with neutral
  local input and no demo input while in the menu. Held keys/menu clicks are
  cleared across transitions. `--paused` starts here for inspection/capture.
- Stage rasterizer depth bias is mapped from forward Z to Bevy reverse Z.
  The initial minimum positive bias of two for blended overlays was removed
  in cutscene-visuals: it was unverified tuning. Only the imported rasterizer
  bias remains, with its sign converted for reverse Z.
- Minimal `net.rs` edits were agreed with codex-18; `take_prejoined` is now
  crate-visible for that agent's M5 integration.

Validation:

- `cargo build --release -p ddview` passes. `cargo test --release -p ddview`
  passes 11 tests. New tests cover mouse signs/limits, short-key buffering,
  and the actual pause ECS system (Esc, focus loss, Resume, Quit, cursor
  options, menu visibility and clearing a Resume click).
- Install-backed held-W/A/S/D regression uses Bevy input mapping and the
  loaded collision/scenery terrain. At the default ref001 position, each
  moves 331.8-347.5 cm over 30 frames. With `DDDA_TEST_SAVE` pointing to the
  maintainer's backed-up save, its position moves 329.9-342.1 cm. This tests
  the input/simulation path, not physical keyboard delivery through winit.
- Timed CLI screenshot runs succeeded for `--play`, `--play --save --paused`
  and both ref001/ref002 poses (before/after). Captures remain in OS TEMP,
  `viewer-menu-save.png`, `viewer-ref00{1,2}-{before,after}.png`; none committed.
  The pause screenshot shows the labelled viewer menu, Resume and Quit.
- Static reference images show no obvious wall speckling before or after;
  differences affect 2,430 pixels (ref001) / 758 (ref002) at 960x540. Thus
  these captures **do not prove temporal z-fighting eliminated**. A bounded
  scripted moving-camera check is the remaining action. No fresh original
  game comparison was made. Native UI automation stopped when instructed;
  subsequent validation used CLI screenshots and app/headless tests only.

Follow-up: codex-18's 41c9fe4 and head's af425c8 are merged into the
cutscene branch. `--unattended`, `--demo`, `--shot` and `--shots` keep the
cursor visible/ungrabbed, require RMB for mouse look and do not pause on
focus loss. Normal manual mode retains grabbed mouse look and Esc pause.
Head corrected the horizontal mouse sign. The inherited integration's
13 viewer tests pass here; no physical keyboard delivery claim is added.
