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
