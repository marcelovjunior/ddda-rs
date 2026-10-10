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

1. Resume from `E:/ddda-re/notes/checkpoints/route/RoadWalk.ddhs`
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
