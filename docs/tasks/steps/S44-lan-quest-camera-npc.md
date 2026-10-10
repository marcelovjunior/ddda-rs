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
  `b44c5bf`. **No Cargo acceptance yet; this is not Ready.**
- Protocol 9 projects the host's stage, camera stage/id/generation/seconds
  and authored input lock. Every snapshot carries the active camera or an
  explicit absent camera. The guest never advances the ownership clock or
  emits CameraFinished: its installed resource exhaustion cannot release
  the host's owner. Client lock is applied before pending command replay
  and subsequent prediction. The S40 source remains 0x005C4600,
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
  while a new launch or recreated body does. The existing HUD bounds,
  camera pull/projection, atlas, fill width and per-camera owners are reused.
  Root UNIT conversion remains separate from the skeleton's body morph.
- Head #6629 approved the related presentation-only stage handoff because
  the old guest stayed in Cassardis while receiving a field camera. The
  stage payload/client-ground replay and opening presentation hook are
  implemented after S51's file handoff. Guest stage changes load scenery
  and collision using Streamer::enter at received positions; locally
  disposable host actor adapters are bypassed on subsequent guest frames. No guest StageLoaded, quest advance or party
  placement is authorized by this adapter.
- Prepared tests cover whole/delta camera/NPC state, explicit camera end,
  malformed payloads, position-only/no-story defaults, native camera12 and
  Reynard actor state over UDP loopback, prediction suppression/release,
  installed token sampling and centimetre-root transforms. Test results
  and release binary pin will be added only after actual process exits.
- Open: S47 appearance replication is a separate schema extension;
  the original visual/LAN comparison, broader NPC specialized body banks,
  corpse alpha presentation (also absent on the local host view), and
  guest dialogue/menu authority are not claimed by this checkpoint.
- Next action: full ddnet/ddhost and
  downstream ddview tests plus a release build under the existing leased
  targets; request one nearest-checkpoint LAN camera/gauge capture.
