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

## Results (ready for review; ordinary camera12 capture completed)

**Implemented:** `mtcamera::quest` reads indexed `cCamExParamAnim` entries
from installed `rCameraParamList scr/stNNN/etc/stNNN`. Stage loader
**0x00501020** requests this class (DTI **0x01993FF8**) into stage+0x964
at **0x00501F80/87**; the resource loader is **0x007D01B0**. Empty Anim
sequences have no pose (native initialization **0x007B76B0** ends them);
the reader preserves their array slots as unsupported, rather than
rejecting the entire stage list. Keys are never copied into the repo.

Camera 12 of stage100 is a world-space sequence with two eye/target keys,
last time **3.0000005 seconds**, FOV **55 degrees**, entry interpolation
**15 frames** and return **30 frames**, both type3/frame-type0. These
values are read from the installation. Camera149, requested by q0002
after leaving Cassardis, uses the same supported class with no input
block. Sampling translates the eight independent channels and tangent
construction **0x007B9F20 / 0x007BA2D0**; equal tangents take the native
linear shortcut, including equal zero tangents. The final time is still
active; exhaustion is strictly after it (**0x007B77E0**). Type2 world
position and type3 shortest-angle/target/distance interpolation follow
**0x007E3C70 / 0x007D6150 / 0x007DE430**, with the installed 8-point ease
curve sampled by **0x00D4EC50**.

The headless `OpeningWorld` owns supported authored camera playback and
advances its resource clock before story completion checks. Supported
camera12/149 requests never reach `ScriptedHost`'s 60-frame timer;
unowned camera timer callbacks are discarded. Resource exhaustion supplies `CameraFinished`, the
existing order slot observes EndCamera, and the authored task sets q2:5.
Native ownership/end sources are **0x005C37C0 / 0x005C41D0 / 0x005C4340 /
0x005C4600 / 0x007DFD40**. A stage handoff invalidates the old camera's
identity and releases its controls, reporting one completion.

Camera12's order has `mSimpleEv=false`, `mForceTimer=-1`, `StopSelf=false`;
its camera has `mIsCantActPlayer=true`, `mCantActFlag=3`, no world slow.
**0x007E0D50** applies the authored action range through two native actor
domains (**0x00437480 / 0x004374D0**, bit indices6/7). Further consumer
tracing found **uPlayerBase::vf90 0x00B61A20**, checks **0x00B62303–9B**:
actor+0x206D bit1 tests mask0x02F80000 (including flag6), while bit2 (Pawn)
tests0x01F40000 (including flag7). These skip new action arbitration at
**0x00B623C4–0x00B62400**; the base character/world phases keep running.
The adapter
blocks movement/attack/interact input for every occupied roster seat;
the host's world/actor phases keep advancing. This is the explicit co-op
extension of the original stage-global camera and input flags: every
local seat shares the shot; the host blocks remote seats too. **Remote
camera pose replication is still open** in the LAN protocol.

The **f96f9de** checkpoint only gated external seat inputs. The subsequent
Pawn-domain fix (head approval5294) also gates generated Pawn pads using
the same authored policy, after ticking their AI. Its installed regression
has two Arisen and two separately owned Pawns: all four bodies remain
still for camera12's first30 motion frames, Pawn AI history and enemy
frames advance, and both
Pawns resume movement after the real camera end. The action adapter still
does not port every skipped native arbitration call.

The event is a camera move, with no LCM show-motion or new actor animation.
The existing NPC/enemy adapters continue their authored actions. A
non-simple end clears the slot (**0x005C4790**) and preserves party
positions. Its end order does **not** emit the simple-event CameraEnd
placement request. Simple-event mode is different: **0x00517050** enters
the stage4 **0x0050DCD0** loading/fade flow, and its ending can apply
`set_pos_type/start_pos`; those fades/placement/skip paths remain unsupported.
Camera12 has no simple-event skip path. Gameplay resumes with the current
follow-camera target/position and saved projection through the authored
return transition.

The viewer applies the host sample with its stage origin, real followed
body target, roll and FOV. `DDVIEW_EVENT_CAMERA_CAPTURE=<private png>`
optionally captures camera12's real midpoint and exits only after the
authored q2:5 plus the return blend; it injects no camera request, position,
combat outcome or quest flag. Ordinary opening uses the same playback.
Reviewed S39 RescueApproach **8dc80e3** is integrated: production demo
input walks into the installed event1 Area and waits for real q2:25.
Reviewed S34 source checkpoint main **10da58b** and body materialization
**ff89282** are integrated, preserving NPC combat observations beside the
new camera hooks. Root independently passed its installed materialization
fixture in the final integrated **ff32ed95** source tree.

**Tested:** core command `cargo test -p mtcamera --test quest -p ddhost
--test quest_camera` exited0: **4 host + 4 reader tests pass**, compile
5.18s, suites8.12s/3.54s. Tests cover installed camera12/149, native curve
branches, unsupported blend/input-domain rejection, both-seat input
blocking, stale timer suppression, installed stage200 handoff, and the
installed q0002_event1 task's q2:25 → actual exhaustion → cleared camera
slot → q2:5. That task test begins at its admitted-task/QCT95 boundary;
it does not claim to walk the original Area. The separate S39 installed
driver regression supplies normal movement toward that Area.

Final integrated **ff32ed95** viewer tests exited0: **71 passed,
2 existing ignored** (compile32.05s, tests12.48s); the release build exited0
in43.74s. The final cinema-policy reader regression passed4 tests
(compile11.62s, tests5.87s). A hidden ordinary `--perf-route` capture used
the privately pinned final binary, a600-second watchdog and exclusive
viewer lease. It **actually exited1 after86.3s**: Cassardis SceNo1300
door interaction requested an unsupported simple-event camera before
camera12. That earlier run supplied no camera12 capture or ordinary-route q2:5.

Head approved retaining the explicitly labelled pre-existing
`ScriptedHost` stand-in for simple/unsupported prerequisite cameras
(board5193). Installed `scr/st200/fsm/sontyoHouseHatudeCam` starts simple
camera24; its native dispatch **0x005C41D0 → 0x00517050 → 0x0050DCD0**
uses the separate fade/loading mode. The adapter logs once per stage/id,
tracks a serial legacy owner and accepts its timer only after that
stand-in's own due frame. Supported Anim12/149 never fall back, including
ownership contention, and legacy callbacks cannot finish a real camera.
Whole simple-event fade/placement/skip fidelity remains open. A regression
covers the early legacy completion followed by camera12's actual end.
Compatibility checks exited0: **5 host + 4 reader tests passed**
(compile6.89s, suites6.09s/3.33s), including rejection of concurrent owners.
The compatibility viewer passed **71 tests, 2 existing ignored**
(compile38.88s, tests11.59s), and its default release build exited0
in45.24s. The private binary's SHA256 is recorded on board5277.
The later Pawn-domain checks passed the same **5 host + 4 reader tests**
(compile2.91s, suites6.68s/2.59s), with the expanded AI/world/resume fixture
described above. The Pawn checkpoint **a168fa3** default release build
exited0 in47.56s; its private binary is pinned with its exact source/hash
on board5356. Root independently passed the same5 host +4 reader tests
and reviewed the Pawn action gate (board5359).

**Ordinary route capture:** the exact **a168fa3** private binary ran
hidden with `--perf-route --players 1 --size 1600x900`, ordinary demo-fight
inputs and a600-second owned-process watchdog. The real Cassardis
NPC/event flow set GSF381 and activated q2, and the gate handoff played
installed camera149 with its authored return. Normal RescueApproach
movement entered the installed event1 Area; camera12 generation2 started
at **05:50:00.600 UTC**. Its real midpoint screenshot was saved at
**05:50:02.223**, resource exhaustion advanced **q2:5 at05:50:03.709**,
and the return to gameplay completed at **05:50:04.606**. No position,
quest flag, camera request or combat outcome was injected. Private
evidence: `E:/ddda-re/notes/steps/S40/camera12-midpoint.png` and
`capture-a168fa3.stderr.log`. The inspected1600x900 image shows Reynard
and the goblins in the authored close-up; existing actor deformation and
material artifacts are prominent. It is camera behavior evidence, not
an original-game appearance match. Root independently inspected this
same image, log, pin hash and process exit.

Wrapper session64857 actually exited0 after306.9s; its owned PID32104
completed `WaitForExit` and a read-only process inventory confirmed it
gone before the viewer lease was released. The wrapper's native
`Process.ExitCode` field was blank: **numeric OS exit0 is not established**.
This is distinct from the actual wrapper exit0 and confirmed process exit.

**Compared against the original:** static executable/data tracing only.
No original-game visual comparison or live camera recording was performed.

**Limits:** only non-simple, world-target2 Anim sequences with supported
type2/3 positive blends and frame-type0 are admitted. Partial actor-domain
flags1/2 and CantActPlayer-only blocks are rejected instead of collapsed.
Actor-relative variants, templates, world slow, camera-request force
timers, simple-event fade/skip/placement, collision along transitions,
keep-angle resume options, cinema framing/FOV policy, and overlapping
camera-owner identity completions remain open. Unsupported modes retain
only the explicitly labelled legacy stand-in described above; ownership
contention fails explicitly. The native action-arbitration pipeline is
not fully ported, and the visible actor/material fidelity issues remain.
The next action is head's review of the final checkpoint and the separate
S44 LAN camera/NPC replication milestone; this brief's ordinary camera
close-up, real q2:5 and return-to-control gate is evidenced.

Further FOV tracing: **0x007B77E0** multiplies the key FOV by 16/9 and
`sCameraExt+0xD04`; **0x00420910** sets that factor to0.5625 normally,
or0.475 for its cinema branch. This is a cinema policy, not an inferred
inverse viewport aspect. Both installed 12/149 have `mIsCinesco=false`;
cinema requests are rejected until that policy is ported. The reflection
registration of this field is **0x007B6080** (+0x20C).
