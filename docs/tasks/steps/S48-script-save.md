# S48: Save and restore running quest scripts (the original's save of non-fix FSMs)

- **Branch:** `step/s48-script-save`
- **Files:** `crates/ddstory` (story save/load), `crates/ddhost` (save chunk),
  tests
- **Kind:** research + one feature; headless only
- **Unblocks:** S46 route checkpoints (`--opening-from`)

## Context

`Host::save`/`load` restore the story world (flags, quests' bits, SceNo,
places, economy, progression, Rift, NPC quest masks), but `Story::from_world`
starts with no running quests or tasks: "Running scripts are not saved by
this runtime yet (the original saves non-`fix` scripts, 0x00495150)". So a
save made during q0002 loses the rescue's running FSM, and the original
would not.

## Do

1. Trace the original's script save (0x00495150) and its load counterpart:
   which scripts it writes (non-`fix` quest FSMs and tasks?), what per
   script (path, current states, timers, free flags, owner/quest ids), and
   how loading re-creates them (re-boot then restore state, or direct).
2. Add that state to the story save (a versioned chunk; old saves without it
   load as today) and restore it in `Story::from_world`-style load so a
   reloaded q0002 continues where it was.
3. Tests: run the installed opening headless to q0002 field entry, save,
   load into a fresh host, and continue: the same quests/scripts are running
   with the same states, and the rescue path completes the same way.

## Done when

A headless save made inside q0002 and loaded into a new host resumes the
running quest scripts as the original does, with sources documented.

## Result (headless implementation complete; review)

Implementation commit: `7484fc5295df994fbe596bc82b23966fedd23792`, on
`step/s48-script-save` (`E:/ddda-rs-wt/s48-script-save`).

`0x00495150` is the **restore** function. The matching serializer is
`0x00493AA0`, which saves quests through `0x00484750` and restores them
through `0x00484910`. Quest records carry their ordered id/branch, flags,
log, QCT disabled masks and game-hour origin, plus their graph and owner.
Task records carry their resource path. The actual task-save predicate
excludes the case-sensitive substrings `stage_005` and `nosave`; it does
**not** exclude every `fix` script. Restoration finds an existing
automatically booted task or creates it through `0x005D0E10` before applying
its saved state. Native scenario families 0..10 retain disabled/fired bits.

`0x00E09620` / `0x00E08560` save graph state by unique node id, pending
transition and from-all history. `0x00E07F70` / `0x00E06B40` reconstruct it
and set first-entry due. Owner functions `0x005CC0A0` / `0x005CC140` copy
32 FreeIns, 32 FreeReal and the selected timer. They do **not** preserve
FreeFlags, state/FSM elapsed clocks, camera slots or per-process scratch.
The native baseline therefore re-enters saved states instead of claiming
a frame-identical continuation.

The host now writes shared, versioned `STOR` and optional `STCX` chunks.
`STOR` models that sourced baseline. `STCX`, approved by head in board
6120, is explicitly **this project's continuation extension**: it retains
the current runtime's clocks, process scratch, entry/transition latches,
camera/stage state, scenario detection state, story frame and `nosave`
tasks. It is not an assertion about the original save format. Without
`STCX`, restoration follows native state re-entry; without either script
chunk, old world-only saves still load as before. Native payload and
continuation must describe the same baseline; incompatible graph nodes,
hierarchy, transitions, process keys, duplicate identities, timer indices,
non-finite floats, versions, truncation and trailing bytes are rejected
before the host publishes a replacement story or changes frame/places.
Saved tasks survive the first matching `StageLoaded`; later area changes
retain their existing unload behavior. One host owns these shared scripts
independently of the viewing Arisen.

Focused checks actually exited zero: six native/continuation unit tests
(1.50 s compile, 0.03 s run) and two host tests (6.91 s compile, 30.45 s
run). The installed opening reaches q0002 field entry for one and two
seats, saves and loads a fresh host with identical script states, owner
registers, quest metadata and QCT masks, and advances camera12 through its
installed clock to q2:5. No elapsed-time victory is accepted. A separate
explicit combat-result fixture supplies three defeated authored enemy
targets and a living NPC intel; the **installed scripts** then produce
q2:20 and the installed exit task produces q2:94 equally in both runs.
That fixture is script-save verification, **not** combat or complete
original-route acceptance. Malformed script and extension chunks leave
the host unchanged, including deliberately different saved frame/places.

The complete required `cargo test -p ddstory -p ddhost` subsequently
**exited zero**, with **97 host + 45 story tests passed**, none failed or
ignored (27.28 s compile; summed test-runner times 215.47 s host and
29.75 s story). This includes the final malformed-extension/frame/place
checks. Private log: `full-story-host.log`. The shared core build lease was
released only after the actual exit (session73447, board6243). Downstream
viewer unit/build integration is delegated to codex-1's shared M5 review
batch; S48 itself requires no GUI capture.

Final review corrected the minimum quest-record size from 15 to 14 bytes;
otherwise a dense valid batch of quests without scripts/control tables
could be refused. A 12-quest regression covers this boundary. After that
one-line decoder correction, the **complete story suite passed 46 tests**,
none failed or ignored, actual exit0 (2.90 s compile, 30.28 s summed runner
time, session29148, `final-story-count.log`). Codex-1 independently passed
both installed host regressions on the earlier implementation (3.21 s
compile, 30.13 s run, actual15768 exit0); those results are distinguished
from the final count-bound correction.

Private source exports and test log stay under
`E:/ddda-re/notes/steps/S48/`; no game/save bytes or raw exports are in this
branch. Original-game save comparison has not been performed. External
combat bodies, NPC actors, pending presentation callbacks and viewer
resources remain their owners' restore/rebinding responsibility. The
current story interpreter only models its existing active-level from-all
history, not the native history of every inactive cluster. Next: head S46
uses this chunk to resume the route checkpoint and rebind those external
systems; add per-system chunks when a checkpoint requires their live state.
