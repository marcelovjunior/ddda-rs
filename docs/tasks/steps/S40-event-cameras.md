# S40: Event cameras (the close-up on Reynard and the goblins)

- **Branch:** `step/s40-event-cameras`
- **Files:** a new `crates/mtcamera` reader or the existing camera crate
  for the authored event camera resources, `crates/ddhost` (who owns the
  camera, input freeze, EndCamera), `crates/ddview` (playing it), tests
- **Kind:** research + one feature

## Context

Many quests briefly take the camera to show something: the maintainer
remembers the roadside scene where the camera moves close to Reynard
while the goblins attack him. The data agrees: q0002's event1, on first
entry into its Area, requests **camera 12**, sound 30 and q2:25, then
waits for `EndCamera` before setting q2:5 (docs/tasks/q0002-roadside.md,
sequence 3). Today `ddstory` emits `Request::Camera { cam_id, simple,
quest }` (orders.rs 0x... `Camera` order) and the opening answers it with
a timer (`ScriptedHost` `camera_frames` -> `CameraFinished`): no camera
moves, nothing is shown. `Request::CameraEnd { set_pos_type, start_pos }`
is emitted too.

## Do

1. Trace the Camera order's runtime: where camera id 12 for quest 2 is
   defined (an event/quest camera list resource; the path and DTI), what
   one entry holds (keyframes/path, targets, FOV, duration, whether it
   plays a show-motion or just moves the camera), and how `simple` and
   the stage mode (0x0050DCD0 fade) differ.
2. What the player and the world do meanwhile (input frozen? characters
   keep acting, so the goblins really attack Reynard on screen?) and what
   ends it (duration, skip input), then EndCamera and CameraEnd's
   `set_pos_type` / `start_pos` (where the follow camera resumes).
3. Play it in the viewer through the existing camera module and report
   its real end to the story (no timer). Co-op: decide (and document)
   whether every seat sees it, following the source where it says.
4. Test: the installed q0002 event1 camera loads and plays to its end,
   EndCamera then advances q2:5; a capture of the close-up.

## Done when

Walking into the roadside scene in `--opening` plays the authored camera
12 close-up, then control returns; the story advances on its real end;
the Result names the resources and functions.
