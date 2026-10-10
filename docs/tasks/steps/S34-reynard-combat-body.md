# S34: Reynard as a combat body, with his HP gauge

- **Branch:** `step/s34-npc-combat-body`
- **Files:** `crates/ddenemyrt` (an NPC fighter), `crates/ddhost` (binding
  NPC304 to it, NpcCheck/NpcDieCheck observations), `crates/ddview/src/hud.rs`
  (the `npc_gauge` layout), tests
- **Kind:** research + one feature
- **After:** S32 (Reynard's authored motions) for the viewer part; the
  research and the host part can start now

## Context

In q0002 the goblins of group 23 attack Reynard (NPC304) on the road. The
quest's event1 and main states read his HP and death (NpcCheck 0x005c0420 /
0x005c0820, NpcDieCheck 0x005c09f0; docs/tasks/q0002-roadside.md), and his
script turns his gauge on (SetGaugeDisp, S23). Today he has no HP, cannot be
hit, the goblins only target Arisen seats (`Fight::enemies` builds targets
from `self.arisen`), and nothing draws his gauge. The head's route demo
already reaches q2:20 (rescue success) because the goblins die before they
can matter: with a real Reynard the rescue can also fail.

## Known (head, 2026-10-10)

- `uGUINpcGauge` (vtable 0x01584950, ctor 0x0069F360, 0x2A0 bytes) is
  created by 0x00BB8C10 into uNpc +0x6FD8 with +0x270 = the NPC; layout
  `id\cockpit\npc_gauge` (vf58 0x0069F540).
- vf09 0x0069F5A0, the fill: ratio = HP (uNpc[0x746] -> +8, when
  uNpc[0x743] != 0) / max (uNpc vf 0x148); hidden when ratio <= 0 or uNpc
  vf 0x1D4 is true; width = ratio x +0x274 (the bar node's width, read in
  vf05 0x0069F410 from node +0xD0), at least 3 px.
- vf11 0x0069FB00, the place: an NPC matrix (vf 0x58 with -1) times the
  local offset uNpc +0x2B70..+0x2B78, minus the body's +0xE0, plus
  0x0067C870; projected by 0x0069FA10; 40 px up. Hidden during events
  (0x018FA4CC +0x2E20) or when uNpc +0x2F86 is 0.
- vf16 0x0069FD70 drops the NPC when its unit state is not 1/2.

## Do

1. Trace where NPC304's HP, max HP, defence and hurt volumes come from
   (uNpc +0x1D18 holder at [0x746], vf 0x148, the NPC's collision), and
   whether goblins pick NPCs as targets (the goblin sensor/target choice;
   Invincible/NoDamage from S23 gate damage).
2. Give the fight an NPC fighter for a bound NPC (owner id range like
   goblins), hit by enemies and blocked by NoDamage/Invincible; let goblins
   target it where the source says they do.
3. Feed NpcCheck/NpcDieCheck from it, so q0002's success/failure branches
   read real HP.
4. Draw `npc_gauge` from vf09/vf11 in the viewer (like `Binding::Enemy` in
   hud.rs), driven by the SetGaugeDisp request.

## Done when

A headless installed test shows goblins damaging Reynard (and NoDamage
holding while set), NpcCheck reading his HP, and the viewer showing his
gauge after SetGaugeDisp. The Result lists each value's source.
