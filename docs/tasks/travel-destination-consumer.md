# travel-destination-consumer: Destination queue through arrival selection

- **Owner:** proposed for Codex lane; await head assignment
- **Branch:** `task/travel-destination-consumer`
- **Board area:** `formats/travel-rules`
- **Status:** proposed

## Goal

Close MAP.md's gap between the actor's selected destination and stage/arrival
selection. Explain allowed/failed travel and consumption ordering. Deliver an
explicit actor/destination data contract for later host-authoritative core work,
not a teleport implementation or a global player abstraction.

## Evidence

Read MAP.md, COOP_DESIGN.md and `ddmap`. Confirmation disassembly0x00691580
queues a built-in row or3+occupied Portcrystal slot in actor+0x3e0c;0x00b77be0
transfers it to active-use+0x3e00. Throw update0x00b06c30 calls generic use
0x00b77130; item1724 is exempt from decrement. Portcrystal use0x00b12c20 saves
stage/position in ten slots;0x00b12fd0 clears them on pickup. The downstream
role of0x00401160 and actual stage arrival remain unproven.

## Scope

- `crates/ddmap/src/travel.rs` if a pure selector can be established
- Minimal `crates/ddmap/src/lib.rs` export only
- `crates/ddmap/tests/travel.rs`
- `docs/TRAVEL.md`
- This brief's Results

Coordinate quest/stage and inventory owners. No edits to world streaming,
ddsim/ddworld/ddview, item internals or oracle tooling.

## Work and original-game evidence

1. Trace the queued selector through resource/object action to concrete stage
   and arrival coordinates/orientation. Prove the role of0x00401160 rather
   than assuming its remapped argument is the teleport destination.
2. Resolve built-in availability/order, placed-slot validity, stage restrictions,
   stale destinations and failure/cancel paths. Document when consumption occurs
   relative to validation and stage change; preserve Eternal exception.
3. Request existing recordings or read-only observations from claude-4 of ordinary
   user-driven travel and placement/pickup: destination selector, saved slot,
   stage/position, item counts and action phase. Do not request teleport commands,
   input injection, game calls, inventory changes or other process writes.
4. Translate only proven pure selection/validation into source-marked helpers and
   regression tests. Represent unsupported states explicitly; propose host checks
   without inventing co-op travel policy or implementing replication.

## Done when

At least one built-in and one placed destination have complete source-backed
selection-to-arrival traces. An available ordinary travel observation verifies
the predicted selector/stage/count relation with limits stated. Report unavailable
observations as blockers; no false comparison claim. No live teleport or core
implementation. Fill Results and report codex-1; head alone merges. Never push,
write the install/process, delegate such writes or commit private captures.

## Results

(filled by assigned agent)
