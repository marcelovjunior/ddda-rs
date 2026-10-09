# stage-detection: field scenario admission

- Owner: codex-1; head assignments #2601 and #2617.
- Branch: task/stage-detection.
- Scope: ddstory scenario/Inputs/tests; ddhost observation feed; tiny
  viewer streamer/opening feed; ddworld parsed collision-cell metadata.

## Problem and evidence

The opening performance route reaches the actual Cassardis gate and loads
stage 100, then aborts on an unimplemented detection-range rule. Source
`cAreaHit::vf07` at `0x00742810` admits an area when its collision cell is
loaded or its anchor is within 15000 cm of the Arisen (inclusive squared
3D distance 225000000). `0x00443a60` only supplies the actor position;
the missing-actor fallback is zero. Other stages skip this range filter.
The co-op extension admits the area near any Arisen, in addition to the
existing any-Arisen shape detection.

`0x005072e0` computes the cell using the installed grid descriptor and
truncation toward zero, then calls `0x00c5f630`. That helper searches
uStageSplitCtrl +0x7c/+0x88 for matching filename N/M and status 3.
`0x00c5f680` proves this is the collision list; the model list in
`0x00c5f070` is +0x68/+0x74. Wanted cells, model presence and layout
presence are not interchangeable with completed collision resources.

Static scenario anchors follow `0x007450e0`: area vertex average (the
half-height added by `0x00fc7600` is subtracted), AABB bottom centre,
OBB translation with Y extent subtracted, sphere centre, cylinder first
endpoint. This does not implement actor-attached AreaHit transforms.

## Results

Implemented the field gate, preserving ordinary shape/facing detection.
Inputs carry an explicit optional loaded-collision observation. The host
copies it into story inputs; the opening viewer refreshes it before each
fixed host step. Missing observations do not invent loaded cells.

Terrain now records only successfully parsed collision cells. Streamer
keeps a separate completed-cell snapshot, updates it after completed
rebuilds and synchronous field entry, and clears it on stage changes.
Its requested build list remains unchanged. A separate read-only layout
request accessor supports the coordinated NPC adapter; that list alone
is not proof of NPC admission or successful layout parsing.

Verified: seven scenario unit tests (four new), installed q0002_event1
inside/outside detection, and an installed terrain test rejecting a
missing requested cell from completed metadata. Strict clippy found an
existing chunks_exact warning in unrelated ddstory/world.rs; verification
with that specific allowance passed. Release viewer build passed (two
existing viewer warnings and the coordinated layout accessor not yet used
by the NPC adapter).
No original-game comparison or full route acceptance is claimed yet.

### Clean route and null task dispatch follow-up

The clean route run on 2026-10-09 reached stage 100/start 2 through the
actual gate with this detection change, then stopped on task 13. Source
`0x00798ce0` returns a null callback outside task IDs 1 through 11;
`0x00798250` leaves the callback result zero in that case. The dispatcher
now preserves this no-action result, without setting terminal bits or
inventing a request. Known non-null callbacks that remain unsupported
still report an error. Eight scenario unit tests pass, including task
0/12/13/UINT_MAX status/request checks and the still-unsupported task 11.
Targeted strict clippy passes with the existing chunks_exact allowance;
dependency-wide strict clippy encounters an unrelated existing ddsave
field_reassign_with_default warning. The null-dispatch follow-up has not
yet been rerun through the viewer route.

Next: rerun the field route after camera-feel's higher-priority capture.
