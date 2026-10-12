# L60: game functions replaceable without recompiling the shards

- **Owner:** claude-6
- **Branch:** `task/L60-overridable-calls`
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The user wants fast iteration for mods and frame-rate work: replacing one
game function (a fixed-30-fps physics step, a mod's new behaviour) must
rebuild in seconds. Today readable ports (mtmath, mtcore, mtframe) are wired
into the 32 lifted shards by `lift.py gen` (`pub use <crate>::abi::f_X`
over the lifted body), and every shard depends on the port crates. Editing
one port recompiles all 32 shards (minutes in the play profile).

## Task

1. Make the shards independent of the port crates: no shard depends on
   mtmath, mtcore or mtframe, and no shard's code changes when a port is
   added or edited.
2. Route calls between lifted functions through one overridable dispatch:
   - cross-shard calls already use `x86::call_lifted` and its per-thread
     resolved-target cache;
   - in-shard direct calls (`super::lifted::f_X`) must also go through it,
     or through an override check that costs no more than a predictable
     branch.
   Measure the cost with `ddperf` (replay and the release main loop). It
   must stay within a few percent.
3. Readable ports and mods register as overrides by address at startup
   (the runtime's Registry, or an override table in ddnative-base that
   `call_lifted` consults first). The replay gate still checks every port
   against its vectors, as today.
4. Show it: edit a port crate and time the play rebuild (target: under a
   minute, with no shard recompiled). Document the mod/override workflow
   in docs/NATIVE_PIPELINE.md.

## Results

(claude-6)
