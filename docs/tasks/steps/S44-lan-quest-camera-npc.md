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
