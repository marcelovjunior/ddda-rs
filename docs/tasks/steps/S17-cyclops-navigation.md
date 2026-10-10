# S17: Cyclops navigation provider (world sets 1002 / 1003)

- **Branch:** `step/s17-cyclops-nav` (after S14, merged)
- **Files:** `crates/ddenemyrt/src/cyclops.rs`, the think-table host
  interface in `crates/ddai`, tests
- **Kind:** one feature

## Context (S14 Result)

The cyclops think table reaches world set 1002 and needs 1003. Setters
**0x008AB3B0 / 0x008AB5B0** go through the optional FieldWayPoint and
NavigationMesh services dispatched by **0x0059F8E0**; absent or disabled
services return 0. Set 1003 can install target slot 7 while the
interpreter runs, which the generic `Host` trait can't express yet.
Common checks 1016/1017 read the resulting path flags; check 1027 reads
actor +0x5E2C.

## Do

1. Extend the think-table host interface so a world set can install a
   slot during execution (the smallest change that expresses 0x008AB5B0).
2. Implement sets 1002/1003 and checks 1016/1017/1027 with a navigation
   provider trait. Give it two implementations: "no services" (the sourced
   return-0 path) and a straight-line provider over the stage collision
   (`ddsim::Ground`), labelled *viewer choice* until the FieldWayPoint /
   NavigationMesh data is traced.
3. Find out whether stage 100 has FieldWayPoint/NavigationMesh data
   loaded at the q0003 cyclops cell (resource names, the service objects),
   and record that in the Result even if you don't parse it yet.

## Done when

The installed cyclops table runs past set 1003 in a test with each
provider, ddai/ddenemyrt tests pass, and the Result says what stage 100's
navigation data is.

## Result (codex-52)

Implemented **32a4dfc** on `step/s17-cyclops-nav`. S17's table/provider
gate passes; the runnable actor, layered WalkTrace motion and fight remain
S18's work.

- `Host::set_with_manager` defaults to the existing `set` callback. Both
  row commits and interpreter command0x79 now pass their executing manager;
  the cyclops callback wrapper forwards it. A regression covers both
  dispatch paths, nonzero callback results and selection of the newly
  installed slot before another command executes.
- `NavigationWorld` wraps a per-actor `NavigationActor`, a
  `NavigationProvider`, and an unrelated-callback fallback. Unsupported
  callbacks retain explicit errors. Host observations supply actor +0xe0
  position, +0x5c90 destination, model yaw, +0x206c bit0x80 and +0x5e2c.
  Positions share the host's stage frame; the original subtracts/re-adds
  `sWorldOffset` at its service boundary. No party seat is assumed.
- Sets1002/1003 translate **0x008ab3b0 / 0x008ab5b0**, with request/poll/
  cancel/configuration service boundaries **0x0059f840 / 0x0059f8e0 /
  0x0059f7a0 / 0x005ad120**. Both setters return0. The adapter preserves
  partial byte writes: status3 writes `[0,1]` to +0x5ca0/+0x5ca1; status4
  sets +0x5ca0/+0x5ca2 but leaves +0x5ca1 alone; status0/1 in set1003
  clears only +0x5ca1; status2 leaves its prior target/flags alone.
- Set1003 installs a point in slot7 during execution. Status3 measures the
  waypoint, stores its absolute heading and passes its **3D** length to
  both distance fields, as the source does. Register/stack inspection of
  **0x008b72a0** establishes that the literal0x10 is **slot +0x7c**, not
  the category. Category is zero and its categorization flag stays clear.
  Status0/1/4 instead copies slot0's position, distance, masks and +0x60
  into slot7 with radius/angle/object/category zero, preserving +0x7c.
  The source's self-position getter **0x004c6b50** reads actor +0xe0.
- Checks1016/1017 translate **0x008aac60 / 0x008aaca0**: the relevant
  path-result byte must be nonzero and actor +0x206c bit0x80 enabled.
  Check1027 is the opaque +0x5e2c byte via **0x008ab100**; its writer is
  not guessed.
- `NoNavigationServices` implements the sourced absent/disabled return0
  path. `StraightLineNavigation` is explicitly **viewer choice**, over
  `ddsim::Ground::ray`, with a host-supplied cast height. It supplies one
  clear segment; obstruction stays pending and invents no detour. It does
  not approximate capsule clearance, step heights or a ground projection.
  Its waypoint arrival comparison retains the source trace's inclusive
  distance threshold (**0x010c5620**). It is not the original path finder.

The installed default cyclops table, mode2 with an explicit target1500 cm
ahead, runs through set1003 for both implementations. Missing services
choose **action1** with direct pointslot7 and path bytes `[0,0,0]`; a clear
straight segment chooses **action85 `cEm5000ActWalkTrace`**, with waypoint
slot7 (`obj=0`, category0, uncategorized, +0x7c=16) and `[0,1,0]`. These are
controlled headless observations, not an original-game fight comparison.

### Stage100 navigation data and services

An installed archive-index survey finds **309 rAIWayPoint occurrences**
and **53 rNavigationMesh occurrences**. At the q0003 cyclops's cell63m52n:

- `scr\st100\etc\st100h_63m52n_mrg00`, `rAIWayPoint`, is in
  `rom\stage\stage100\split_way\m60\n50\st100_63m52n_way.arc`.
  Inflated size35,279 bytes, `WAY\0`, version0x26. Adjacent63m51n and63m53n
  have the same resource family in their own split-way archives.
- `scr\st100\etc\st100h_`, `rAIPathConsecutive`, is in `stage100.arc`;
  size2,501 bytes, `BBS\0`, version5. The stage100 initializer
  **0x0051b8e0** loads this resource, looks up **`FieldWayPoint`
  (`cAISvOpenField`)**, and attaches it through **0x010ac600**. That helper
  stores the resource at service +0x5c and enables service byte +0x1dd.
- Stage bootstrap **0x00501020**, calls at **0x00501ff6 / 0x0050200d**,
  registers both named service objects: `FieldWayPoint` (`cAISvOpenField`)
  and **`NavigationMesh` (`cAISvNavPathFinding`)**. The generic loader also
  requests `scr\st%03d\etc\st%03d_nav` and attaches any available NAV data
  through0x010929c0. There is **no stage100 rNavigationMesh resource** among
  the53 installed NAV occurrences. Dispatch0x0059f8e0 uses FieldWayPoint
  first when enabled; an enabled NavigationMesh service then overwrites
  the result. Named service registration alone does not establish enabled
  data or a live per-actor trace object.

Thus the tutorial field has authored WAY/BBS navigation data; selecting
the no-services provider for all stage100 actors would not be faithful.
The WAY graph and BBS loading/streaming remain unparsed. Actual service
state at the q0003 encounter was **not measured live**. A bounded read-only
oracle snapshot found the original paused in **stage200**, with active0,
bg0 and frame36462 unchanged before/after. Vtable scan candidates were not
promoted to proven named objects. No calls, input, writes or resume occurred.

### Validation and remaining work

Shared exclusive target: `E:/ddda-rs-wt/item-use-review-target`.

- `cargo test -p ddai -p ddenemyrt -- --test-threads=1`: **ddai35 passed;
  ddenemyrt84 passed, 1 existing ignored**, exit0. This full run preceded
  the final0x10 argument-field correction above.
- Final `cargo test -p ddai -p ddenemyrt --lib`: **17+26 passed**, exit0;
  `cargo test -p ddenemyrt --test cyclops_navigation -- --nocapture`:
  **3 passed**, exit0, after that correction. The installed assertions
  retain action1/action85 and verify final pointslot metadata.
- Final `cargo clippy -p ddai -p ddenemyrt --no-deps`: exit0, only two
  existing combat.rs type-complexity warnings. `git diff --check` passes.

Private traces, corpus index and the read-only snapshot are under
`E:/ddda-re/notes/steps/S17/`. No game data, decompiler exports, recordings
or private files enter the branch. No GUI, install writes or process
mutations. All build/live claims were released.

Next: S18 consumes this provider boundary, drives the sourced layer1
WalkTrace start/loop and physical movement, and tests the q0003 encounter.
An original FieldWayPoint decoder/provider should follow from the WAY/BBS
data and trace-object callbacks; the straight segment remains a declared
viewer choice until then.
