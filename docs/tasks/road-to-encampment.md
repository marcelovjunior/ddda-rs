# road-to-encampment: q0003: the road to the Encampment

- **Owner:** codex lane
- **Branch:** `task/road-to-encampment`
- **Board area:** `story/q0003`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Trace q0003 (SceNo 1500 -> 2100, event st210 ev00 at 2000) and the route out of Cassardis. Build field streaming from STREAMING.md (terrain and collision 5x5, layouts 3x3, the three separate decisions) so the walk is continuous. Enemies on the road use the runtime, wolves and harpies included (enemy-families). End on arrival and the st210 event.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddworld/**`
- `crates/ddview/src/stage.rs` (claim)
- `crates/ddstory/**`
- `crates/ddhost/**`
- `docs/STREAMING.md`
- `docs/QUESTS.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

Two players walk from Cassardis to the Encampment without a load break or teleport, the st210 event plays, and the quest state matches the original's. Fill in Results and report to `head`.

## Results

(filled in by the agent)

### First q3 entry checkpoint (2026-10-09)

- Traced installed q3 graph and seven-row QCT: arrival/Rift contact,
  NPC camera sequence, cyclops target20:0, report, st210ev00, Barn. The
  event is after the encounter/report, not simply arrival; full route
  acceptance remains open.
- AreaHit act5 party targets translated from5c2d70/7643d0 with current
  player/pawn observations and explicit any-Arisen co-op semantics.
  Unknown non-party positions fail explicitly.
- Task11 NPC archive callback79d730 now respects its authored half-open
  scene interval; active resource ownership remains unsupported. This
  lets Encampment's later hydra resource scenario remain inactive at1450.
- Verified:9 scenario unit tests,1 all/any-target regression,1 installed
  four-combination q3 entry regression, strict ddstory clippy (existing
  chunks_exact_to_as_chunks allowance). No original comparison yet.
- Next: bounded real doorway/door-interaction route, then field walk and
  stage210 handoff; full cyclops/Rift/NPC systems are separate prerequisites.

### Doorway and party residency checkpoint (2026-10-09)

- The demo no longer treats SceNo1300 plus elapsed time as leaving the
  house. Its goal crosses the far support plane of the installed doorway
  OBB, and completion requires the actual actor outside that shape.
  This is labelled demo navigation geometry, not an original quest rule.
- Blocked demo movement can issue ordinary interaction at an unlocked
  closed door. Planning treats such a door as an interaction transition;
  real movement retains the leaf collision. Locked or unresolved doors
  remain obstacles. No completion flags or collision bits are forged.
- Collision/layout residency unions every actor's sourced window. Local
  opening honors --players and reuses the second keyboard movement input;
  LAN preparation keeps one local seat and lets peers join via Server.
  Second-seat conversation interaction and live LAN guest stage handoff
  remain unsupported.
- Verified: five viewer doorway/navigation tests, four ddworld streaming
  tests, strict ddworld clippy and release viewer build. A bounded hidden
  run physically crossed the starting house door after ordinary interact.
  Its later street blockage exposed another closed-door planning issue;
  a second two-seat run found a 196-waypoint exit path, then exposed a
  path-discard loop on door interaction. Interaction now retains that
  path; a fresh bounded run is checking it. P2 is idle in these
  automated runs: this is not two-player route acceptance or an
  original-game comparison.
