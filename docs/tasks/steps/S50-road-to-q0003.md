# S50: The road past the rescue: q0002_event2 -> q2:10 -> q0003

- **Branch:** `step/s50-road-q0003`
- **Files:** `crates/ddview/src/opening.rs` (demo RoadWalk/planner), streaming
  or story code if a real blocker shows; tests
- **Kind:** diagnose + fix; GUI checks only from S46 checkpoints
- **After:** S49 (so the rescue outcome is stable)

## Context

After the rescue and Reynard's talk, the demo's RoadWalk heads for the
authored `q0002_event2` volume (q2:10, boots q0003; the road toward the
Encampment). It never arrives: earlier runs stalled ~120 m short (no planner
path across unloaded cells), then the planner's partial path helped, but the
latest runs still time out (300 s) with the Arisen replanning in place near
(36086, 31978, 142079). The original's player simply walks the road.

## Do

1. Resume from `E:/ddda-re/notes/checkpoints/route-v2/RoadWalk.ddhs`
   (`DDVIEW_OPENING_FROM`, `DDVIEW_OPENING_STEP=RoadWalk`, x4) and find why
   the demo can't progress: collision/terrain gaps on the road, cells not
   streaming in ahead, planner limits, or a real game blocker (a gate, an
   NPC, an event).
2. Fix the real cause (a missing collision piece is a renderer/streamer
   bug, not a planner tweak); if it is only the demo planner, give it a way
   to follow the road (e.g. waypoints from installed navigation data, as
   S17's navigation provider does for the cyclops) without teleporting.
3. Reach event2: q2:10 set, q0003 booted, by the authored scripts.

## Done when

From the RoadWalk checkpoint the demo reaches q0002_event2 and q0003 boots,
with the cause documented.

## Results (codex-76, ready for review)

**Implemented, tested and walked:** the v2 RoadWalk checkpoint reaches the
authored event2 Area; q2:10 becomes true and q0003 boots. The ordinary viewer
exits 0. Original-game appearance/path comparison and the rest of q0003's
Encampment/cyclops route are not claimed. Head's source/current save container requires
the regenerated ECON-v2 checkpoint; the earlier `route/` file is incompatible.
The v2 RoadWalk SHA-256 is
`F19945A9F9A075282A3B4A375696C7259A6453AE4007394E4C3A647C8705A5A9`.

### Diagnosis

- The earlier `d5567f8` capture starts one incremental collision search at
  09:29:52 UTC and remains in that search past 09:31:58. No completed-path
  log occurs. Its 1-ms-per-render-frame budget accounts for why an extensive
  search takes minutes in the viewer; the driver is not repeatedly finding
  an empty path.
- With head's normal `build/core` lease, a private standalone diagnostic
  extracted the unchanged planner from base `e83c344` and linked the
  existing shared core libraries. Current terrain/sweep source hashes were
  checked against the accepted S30 worktree. No Cargo, new target, renderer
  build, game process or game file mutation was involved. The executable
  actually exited 0; the core lease was released before the S41 GPU pair.
- Replay starts at the real v2 checkpoint position
  `(35764.04, 31966.314, 141997.23)`. Collision/scenery residency uses the
  installed grid and the unchanged native 5x5/3x3 predicates. The target
  supplied by the installed scenario is
  `(19497.533, 32701.79, 137001.86)`.
- That CPU search reaches its existing cap after 34.89 seconds:
  60,001 visited keys, 60,372 nodes, a 345-waypoint partial path, ending
  `(20080.717, 32182.264, 137049.5)`, 585 cm horizontally from the target.
  The path repeatedly cycles through the **same physical positions under
  different grid keys**. Keys advance on a regular grid while each point
  advances from the previous capsule's slid position; accumulated slide
  drift makes the key cease to identify the intended location. This is a
  demo-planner bug, not evidence of missing collision or an original gate.
- The initial completed collision set does **not** include cell63m51n,
  which contains the event2 centre. The partial endpoint is just inside
  cell63m52n's western edge. This is the native distance predicate's normal
  residency boundary, not proof of a missing resource or blocked scenery.
  Walking the safe partial route should change residency and admit the
  centre's cell. Exhaustively searching the first window delays that walk.
- `demo_input` also returns while that search is pending before checking
  its existing RoadWalk 300-second deadline or actual quest outcome. Thus
  the intended timeout cannot bound the stalled search.
- The newer main-7ea18c9 rescue capture adds another cause: direct steering
  toward the distant centre cuts across the road's embankment **before**
  planning begins. At 11:01:00.754 UTC the body is at Y31367; by
  11:01:01.246 it falls to Y30551, and subsequently stalls near
  `(25928, 30350, 138664)`. Planning only starts at 11:01:04.839 and returns
  170 waypoints at 11:02:35.167, after the demo deadline. Thus waiting for
  a collision stall before planning can strand the body below the road.

### Changes and validation

The demo search anchors candidate X/Z positions to its initial grid origin
and uses the installed event2 `Shape::hit` predicate for completion rather
than requiring arrival at its centre. Actual quest detection, movement,
collision and flags remain host-owned. The road deadline/outcome can clear
a pending search before its budget return, nearby combat or door input.
Successful RoadWalk completion requires **both** q2:10 and a loaded q0003;
the timeout reports a failed application exit instead of a successful route.
With head's #7294 approval, an actual completed-cell snapshot also lets the
demo return a safe partial route at the boundary toward an unloaded target,
retaining the existing 10-m-progress condition. Ordinary movement and native
streaming admit the next window before a fresh search. No cap, movement speed, collision
threshold, quest bit, HP or XP is tuned or manufactured.
RoadWalk starts its search before its first direct heading and replans
when a partial path ends. A missing/empty safe route waits and retries;
it does not substitute straight movement over an embankment. Pending
searches refresh actual completed residency, including the first host tick
after a checkpoint resume.

Five regressions **pass**: entry into a trigger
whose centre has no walkable ground; the recorded RoadWalk position over
installed collision/scenery, geometrically replaying native residency at
partial endpoints and checking actual Area entry without repeated wall
positions; early partial-route return while unexplored loaded cells remain;
and timeout/native-outcome handling during a pending search (including
q2:10 without q0003, which must keep waiting) without
manufacturing quest success; and the first RoadWalk heading with no safe
ground, which must wait without placing the actor or inventing flags.
This geometric search regression is not a
simulated walk or a game-process acceptance run. The separately observed
ordinary-input route below supplies that gate.

- Source commits: `824c1f0`, `6c90b6e`, `1e43ba9`, `ad098e1`, `c53d4d9`.
  Reviewed main dependencies are merged on the owned branch at `ee24b63`
  (main7ea18c9) and `b644636` (main5a76f19). Relative to reviewed main,
  feature edits are only this brief and `crates/ddview/src/opening.rs`.
- After narrow release cleaning of the viewer and shared S55-affected
  economy/progression/host/network packages, `cargo test --release -p ddview
  -- --test-threads=1 --nocapture` actually exited 0 (session79570):
  **114 passed, 2 existing ignored**, fresh compile35.58 s/tests81.79 s.
  All five new road regressions are explicitly present and passed. The
  installed geometric replay visits3,796 keys/4,096 nodes and returns135
  waypoints ending inside the Area at `(21591.287, 31996.15, 138431.81)`.
- `cargo build --release -p ddview` actually exited0 (session52693),
  40.11 s. The frozen private `ddview-s50-b644636.exe` is95,030,784 bytes,
  SHA-256 `7BBC77DB1802513806C88369B29B9B97196ACACC3B4A54E5A103A5C0DC878763`.
  The shared M5 lease was released after that actual exit and pin copy.

### Actual checkpoint walk

One normal `live/ddview` lease launched that private default-renderer pin,
with the unchanged real v2 checkpoint above, `DDVIEW_OPENING_STEP=RoadWalk`,
x4 virtual time, ordinary demo combat/input, one Arisen, 1600x900 and
`--perf-route --shots <private directory>`. Negative diagnostic flags were
removed rather than set to empty strings. No original game control,
teleports, placement fallback, quest bits, HP, curatives or XP were injected.
Curative use, if needed, retains the existing host-owned inventory action.

- The first safe search begins at11:37:31.707 UTC and finishes with135
  waypoints at11:37:34.298. Actual walking changes native collision/layout
  residency; ordinary blocked-path replans finish with93 and56 waypoints.
  The body follows the safe detour instead of taking the direct embankment
  drop seen in the old capture.
- At **11:38:07.971 UTC**, the actual driver logs
  **`q2:10 true, q0003 booted true`**, then RoadWalk -> Done with quests
  `[28, 31, 1, 2, 3]`, stage100/SceNo1410. The endpoint is
  `(21555.422, 31997.69, 138440.47)` and the host writes a real `Done.ddhs`
  checkpoint, SHA-256
  `CD9CDD775A66F73B112637D59DD87E971CF8266E1573BB40E3941F58508EB900`.
- Viewer PID240 and the enclosing harness session9009 **actually exit0**
  after55.606 s, naturally before the120-s wall exit/180-s retained-handle
  watchdog. The owned viewer is absent before lease release; the original
  DDDA PID23408 remains running. No FPS improvement is claimed: screenshots
  and x4 make this a route check, not a timing sample.
- Both actual PNGs were viewed. The endpoint `7-q0002.png` shows the
  dressed Arisen standing on the road with terrain, rocks, the palisade and
  the HUD listing q0003; HP480 is visible. The initial `1-newly-arisen.png`
  is a sky/HUD-only startup frame and is **not** a geometry/appearance gate.
  Neither is a comparison against a matched original-game reference.

Private evidence is under
`E:/ddda-re/notes/steps/S50/real-v2-roadwalk-b644636-r1/` (logs, recorded
arguments/exit/provenance, PNGs and checkpoint), with full test/build logs
beside it. The private launch plan performs checkpoint/pin/harness SHA
checks and refuses a failed normal lease before starting a viewer. It uses
the accepted S41 safe retained-process-handle harness, not the old PID-stop
helper. No private evidence or game files enter the repository.

### Authored navigation evidence

Head approved installed WAY-v26 guidance as a fallback. Native loader
`0x011FD5C0` reads a 36-byte header, then variable-length nodes with a u16
node ID, XYZ floats, associated-record count/IDs and links. Each link holds
a u16 destination node, associated IDs and three floats; node shape and
additional IDs follow, then the spatial tree. A private decoder agrees with
the header's node/link/associated counts for eight nearby installed WAY
resources (10–636 nodes each). The points are already in the stage frame.
The native `rAIPathConsecutive` BBS-v5 loader is `0x010ADA50`; stage100
attaches it to FieldWayPoint in `0x0051B8E0` via `0x010AC600`.

The nearest road graph node to the replay position is 64m53n node103,
approximately 6 m away. A route within that graph has 36 authored nodes,
but includes links with substantial vertical changes. These are guidance,
not proof that every NPC graph edge is walkable by an Arisen. No raw graph,
game data, native resource or speculative cross-cell connection is copied
into the repository. The smaller collision/grid/Area correction succeeds
in the genuine route; a wider navigation-provider port was unnecessary.

Next action: head reviews/merges this branch, then the next q0003 route brief
uses the real Done checkpoint to advance toward the Encampment/cyclops.
S52 supplies the pawn-message queue that event2 subsequently waits on.

### Head follow-up: the continuous run (2026-10-10)

The first full x4 run after the merge froze at RoadWalk: the body never
moved for 300 s while the planner kept returning 134 waypoints. Resuming
from the RoadWalk checkpoint reproduced it, so it was not leftover talk
or camera state; every input gate was clear and the sim alternated
Walk/Wait. A headless probe at that position found the installed ground
at Y 31958.9 against a body at Y 31941.9: the Arisen was 17 cm under a
rising slope, and the capsule sweep moved it 0.0 cm in all eight
directions. `Fight::push_out` had added a goblin's push to x/z after the
sim's sweep; the original applies it before the scenery sweep
(0x0076D6B0). Fixed in cbc5b85 (`Sim::push_player`, regression
`crates/ddsim/tests/push.rs`). The next uninterrupted run reached q2:10 and
q0003 (exit 0, 2 min 24 s wall); route-v2 was regenerated from it.
