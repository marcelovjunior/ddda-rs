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
