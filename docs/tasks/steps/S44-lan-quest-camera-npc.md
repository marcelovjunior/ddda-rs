# S44: LAN: quest cameras and NPC combat state for guests

- **Branch:** `step/s44-lan-camera-npc`
- **Files:** `crates/ddnet` (mirror/proto, protocol bump), `crates/ddview`
  (guest presentation), tests
- **Kind:** one feature
- **After:** S40 (quest cameras, host side) and S34 (NPC combat body, HP,
  gauge)

## Context

Requests #5225 (codex-61) and #5242 (codex-62). The host now plays the
authored quest cameras (camera 12 at the roadside, input blocked for
every roster seat) and has Reynard as an NPC combat body with HP and a
gauge. A LAN guest receives neither: its `NpcBody` carries only id and
position, and nothing about the active quest camera.

## Do

1. Quest camera: replicate stage, camera id, generation, the authored
   clock and the input-lock state; the guest plays the same installed
   camera from its own install and never decides completion itself (the
   host's real end is authoritative).
   Carry the explicit end edge (active = none) too, so the guest starts the
   installed return blend when the host's camera ends; the generation
   identifies replacements; the guest's input prediction honours the
   replicated lock before its sim step (codex-62, #5331).
2. NPC combat: replicate per NPC its HP / max HP, gauge visibility
   (SetGaugeDisp), death stage and the motion/pose fields the guest needs
   to draw Reynard as the host does (the S32 motion token: bank/slot/
   frame), with masked deltas like S33's enemy snapshots.
3. Bump the protocol and document it; round-trip tests for both; an
   installed host+guest test where the guest's view of the roadside
   camera and Reynard's HP follows the host.

## Done when

On a LAN pair the guest sees the camera 12 close-up with the host and
Reynard's gauge/HP move as on the host, with tests and the protocol
documented.


## Result (codex-72, source checkpoint, acceptance pending)

- Branch `step/s44-lan-camera-npc`, initial implementation `15c12c3`,
  host getter `4935051` (same two-line source as S51 `201ec44`), reviewed
  main `3ecd13e` integration `70d02f4`, centimetre skeleton transform fix
  `b44c5bf`, stage hook `888f820`, stage prediction/interpolation fix
  `2865b70`, permanent retirement `10ddfd4`, first-join collision
  readiness fix `5faead3`. **Core and viewer suites passed; LAN visual
  comparison remains pending.**
- Protocol 9 projects the host's stage, camera stage/id/generation/seconds
  and authored input lock. Every snapshot carries the active camera or an
  explicit absent camera. The guest never advances the ownership clock or
  emits CameraFinished: its installed resource exhaustion cannot release
  the host's owner. Client lock is applied before pending command replay
  and subsequent prediction. Stage changes suspend prediction until the
  new installed collision is ready; the held commands replay once. Remote
  interpolation clamps to the new stage boundary while old snapshots are
  retained as wire baselines. Ordinary terrain refreshes do not replay
  already predicted commands. The S40 source remains 0x005C4600,
  0x007B77E0/0x007BA2D0; the replication policy is a co-op decision.
- NPC snapshots use groups for position, lifetime/yaw/scale, gauge,
  motion token (command/launch/bank/slot/frame/blend), and HP/max/death
  phase/alpha/retirement. New lifetimes send all groups; partial lifetime
  replacements, missing baselines, duplicate IDs, unknown masks/phases,
  nonfinite values, truncated and trailing bytes are rejected. Ordinary
  scheduled/parked NPCs retain position-only defaults when no actor state
  exists. Values come from S32's canonical host layer and S34's ordinary
  body (0x00534BC0 gauge, 0x00B988B0/0x00B98AE0 death phases).
- Guest NPC model/motion resources come from its own install. Host tokens
  hold the exact sampled frame; a frame update does not restart blending,
  while a new launch or recreated body does. Permanent retirement removes
  the rendered entity but keeps the received intel lifetime. The existing HUD bounds,
  camera pull/projection, atlas, fill width and per-camera owners are reused.
  Root UNIT conversion remains separate from the skeleton's body morph.
- Head #6629 approved the related presentation-only stage handoff because
  the old guest stayed in Cassardis while receiving a field camera. The
  stage payload/client-ground replay and opening presentation hook are
  implemented after S51's file handoff. Guest stage changes load scenery
  and collision using Streamer::enter at received positions; locally
  disposable host actor adapters are bypassed on subsequent guest frames. No guest StageLoaded, quest advance or party
  placement is authorized by this adapter.
- Tests cover whole/delta camera/NPC state, explicit camera end,
  malformed payloads, position-only/no-story defaults, native camera12 and
  Reynard actor state over UDP loopback, prediction suppression/release,
  installed token sampling and centimetre-root transforms. Focused library
  acceptance: actual session40284 exit0, 9.24s compile, 10PASS. Full core:
  actual session94820 exit0, 41.14s compile, **98 ddhost +25 ddnet =123PASS**,
  no failed/ignored. All eight new network regressions ran, including the
  installed UDP camera12/HP/gauge/motion fixture. The HP-loss fixture is
  explicit and does not claim a natural-hit original-game comparison.
  Private full log: `E:/ddda-re/notes/steps/S44/full-core.log`.
  Full downstream viewer: actual session35333 exit0, fresh22.65s compile,
  **92PASS, 2 existing ignored, 0 failed**, 11.67s tests. All three new
  viewer regressions executed by their exact source names: centimetre-root
  transform, installed remote token/frame and guest camera ownership.
  Private full log: `E:/ddda-re/notes/steps/S44/full-viewer.log`.
  Release build: actual session59507 exit0, 35.84s. Before releasing
  `build/m5`, copied private pin
  `E:/ddda-re/notes/steps/S44/ddview-s44-lan-5faead3.exe` (94,590,976 bytes),
  SHA-256 `E74881A3B272C6AFFF89BC6B47024F66A0B5A4ADC879CC864782694DE5D286A1`.
  Both build leases are released; no GPU lease or owned process remains.
- Open: S47 appearance replication is a separate schema extension;
  the original visual/LAN comparison, broader NPC specialized body banks,
  corpse alpha presentation (also absent on the local host view), and
  guest dialogue/menu and door state replication are not claimed by this checkpoint.
- Follow-up (head #6837): `clock::GameClock` retains the initial
  `args.stage` across both host and guest opening stage handoffs. Its
  installed lighting lookup therefore remains on that initial stage;
  lighting-stage synchronization and host clock/weather replication are
  inherited presentation gaps outside this camera/NPC snapshot change.
- Capture preflight: the guest can request the actual camera12 midpoint
  and production gauge screenshots. Its disposable local story does not
  reach host q2:5, so the existing camera helper cannot terminate the guest
  automatically; use a bounded paired CLI harness that records both owned
  process exits. Do not resume the host checkpoint independently on the guest.
- Next action: independent source review of `5faead3`, then one bounded
  nearest-checkpoint LAN camera/gauge capture from the private pin.
- Parent review (2026-10-10): source checkpoint accepted and merged by head;
  paired visual acceptance remains open. Approved bounded x1 capture used the
  same `5faead3` pin and frozen ECONv1 RescueApproach checkpoint (SHA-256
  `EB56492CDE6B56C86EADDEE6C971757FE8087964040D6899EA880C3998F9A608`).
  Private valid output: `E:/ddda-re/notes/steps/S44/pair-5faead3-r2`.
  Actual harness exit0; owned host31444 exit0 after30.957s and guest28016
  exit0 after63.104s. Both owned processes were absent before releasing
  `live/ddview`; original DDDA23408 remained running. These are execution
  results, not an FPS comparison.
  Host camera12 ran09:57:53.240..56.375UTC, midpoint PNG saved54.821UTC.
  Parent inspected the PNG: terrain, Reynard, goblins and the blue NPC gauge
  render during the actual authored camera. Separate gauge screenshot was
  skipped by Bevy's duplicate-render-target check on that same frame.
  Guest prejoin obtained stage100, but its resource bootstrap did not present
  that stage until09:58:05.257UTC, after the host exited09:57:57UTC. No guest
  camera/gauge PNG exists; a short camera delivered before resource startup
  does not establish paired visual playback. Next action: coordinate a real
  quest trigger after both viewers finish bootstrapping, then capture both
  views with one screenshot request per render target.
  Earlier `pair-5faead3` output is rejected: a refused normal lease was
  accidentally followed by a launch. Only those private owned processes were
  stopped; no head/original process was touched. The private paired harness
  now checks normal claim success internally before launching either job.
