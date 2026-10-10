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

## Results (codex-76, work in progress)

Current source changes are **untested**, and the authored q2:10/q0003
walked gate remains **open**. Head's source/current save container requires
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

### Prepared changes and validation

The demo search anchors candidate X/Z positions to its initial grid origin
and uses the installed event2 `Shape::hit` predicate for completion rather
than requiring arrival at its centre. Actual quest detection, movement,
collision and flags remain host-owned. The road deadline/outcome can clear
a pending search before its budget return, nearby combat or door input.
With head's #7294 approval, an actual completed-cell snapshot also lets the
demo return a safe partial route at the boundary toward an unloaded target,
retaining the existing 10-m-progress condition. Ordinary movement and native
streaming admit the next window before a fresh search. No cap, movement speed, collision
threshold, quest bit, HP or XP is tuned or manufactured.

Four regressions are prepared but **not executed**: entry into a trigger
whose centre has no walkable ground; the recorded RoadWalk position over
installed collision/scenery, geometrically replaying native residency at
partial endpoints and checking actual Area entry without repeated wall
positions; early partial-route return while unexplored loaded cells remain;
and timeout/native-outcome handling during a pending search without
manufacturing quest success. This geometric search regression is not a
simulated walk or a game-process acceptance run. Full viewer tests, release build and
the real v2 walked route are pending the coordinated build/GPU queue.

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
into the repository. The grid/Area correction is tested first; a wider
navigation-provider port is unnecessary if the genuine route succeeds.

Next action: execute the installed collision/Area regression after the
current S41 captures, then build and walk the v2 checkpoint to authored
q0002_event2 and q0003 with a retained-process-handle watchdog.
