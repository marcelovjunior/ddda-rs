# S47: Goblins look right: equipment and armour by placement (maintainer report)

- **Branch:** `step/s47-goblin-equipment`
- **Files:** `crates/ddenemyrt/src/goblin.rs` (equipment state), `crates/ddview`
  (goblin body: visible mesh groups, attached equipment units), tests
- **Kind:** research + one feature; headless tests + ONE capture at the end
  (see "GPU checks" in README)

## Report

The maintainer: our goblins look like advanced, armoured goblins; the
goblins at the start of the game wear no armour. The viewer spawns
`model\em\e01\e0100\e0100` with every mesh group visible (`character::spawn`
passes `|_| true`); e0100 has ~60 mesh groups (equipment variants among them).

## Known (head, 2026-10-10)

- `cSetInfoEnemy0100` (vtable 0x015A15B4, DTI 0x01995FA8): `mEquipType` at
  set-info +0x110, `mIsLeader` +0x114. vf08 0x008AEBB0 copies the equip
  type to unit +0x6078; a leader (+0x114) keeps it only if allowed for its
  kind (kind 5: 0, 1, 4, 5; kind 6: 0, 1, 3, 4), else 1.
- `uEm0100::vf33` 0x008CDD80 turns (kind +0x2D, equip type +0x6078) into an
  equipment bit mask: kind 5 table 0x008CF3F8 (type 1 -> 5, 2 -> 8,
  3 -> 0x10 + flag +0x616D, 4 -> 0x81, 5 -> 0x85, 6 -> 0x88, 7 -> 0x90,
  other/0 -> 1); kind 6 table 0x008CF430; kind 7 0x008CF414; kind 0x8E by
  +0x6080. Each bit creates an attached unit: 0x1 -> 0x008EB460, 0x4 ->
  0x008EB700, 0x80 -> `uEm0100_20`, 0x20/0x40 -> other classes (kind 6/7),
  each bound to the goblin (+0x2530/+0x2540) and registered via 0x00DB58B0.
  Flags +0x615E/+0x615F/+0x616D feed unit +0x2ECC bits 0x10000/0x20000/
  0x40000.
- The q0002 rescue goblins (group 23) have equip type 0 (S22).

## Do

1. Find what the base e0100 model shows by default: which mesh groups are
   hidden/shown per kind/equipment (look for the uModel per-group display
   mask/parts-visibility writes in uEm0100 vf05/vf33 or its equipment units,
   and the material/part setup), and what each attached unit is (its model:
   e0100_03, e0100_20, om10440/om10465..., attach joint).
2. Carry the placement's equip type and leader flag into the goblin
   (ddhost::spawns already reads the record) and present the same body:
   visible groups plus attached equipment models at their joints.
3. Headless test: the rescue goblins' equipment state from the installed
   records; one final capture of a rescue goblin next to a placement with a
   higher equip type.

## Done when

Rescue-area goblins look like the original's early goblins (no armour), other
placements show their authored equipment, and the Result lists the sources.
