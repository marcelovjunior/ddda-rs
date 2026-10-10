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

## Results — core checkpoint (codex-65, 2026-10-10)

Implemented and tested, **whole S47 remains open**:

- `cSetInfoEnemy0100::vf08` (0x008AEBB0) preserves authored equipment,
  leader, weapon-hand-start and ballista-license state. Kind-5 leaders
  clamp forbidden equipment types to 1 before the vf33 unit-mask dispatch
  (0x008CDD80). The host now carries these fields into each goblin lifetime.
- `uEm0100::vf124` (0x008CF4F0) starts a 512-bit body mask with group 0;
  the ordinary, helmet and leader selections read the installed lists
  through 0x017C277C/0x017C27B4 and weighted maps 0x01503208/0x01503230.
  Lists and model resource names are runtime data, not embedded masks.
- Source RNG is 0x00D00690, with native seeding 0x00D006C0 and static
  initializer 0x0137E5C0 (seed 0x1105). Three seeds and five subsequent
  draws each match private x86 instruction emulation. One host-owned
  appearance stream chooses each nonleader body once; leaders consume
  no appearance draw. **Not stream-exact with the original global
  0x018D017C:** unrelated subsystem draws are not integrated yet. Guests
  will receive resolved appearance through S44; no wire schema changed.
- Attached-unit resource paths are read from vf33 descriptors. Initial
  joint/offset rules come from 0x008EB550 (weapon), 0x004C6D60 (shield),
  0x008EBEC0 (extra equipment). Later weapon attachment changes and
  detached/dropped equipment are still open.

Actual headless validation on the installed game: five source unit tests
passed (13.39 s compile, 0.00 s tests); installed body-group/model/joint
validation passed (1.05 s compile, 3.16 s test); installed rescue placements
passed through the host with one, two and four Arisen (7.25 s compile,
5.91 s test). All completed with exit 0. An initial installed-test compile
failed for a temporary borrow; corrected the test's backing-buffer lifetime.
No original-game control or GPU capture was performed.

Next: connect resolved masks and separately attached units to the viewer,
preserving native joint scale, then run viewer tests/build and queue one
bounded S46-checkpoint capture. Original visual comparison remains open.

### Viewer checkpoint

The ordinary opening and `--play` goblin builders now use the host's
resolved mask, with each equipment unit on its native initial joint and
offset. Native `uCoord::vf21` (0x00DEBBA0) preserves the parent's joint
scale; the shared attachment adapter has an opt-in raw-matrix path, with
existing human `uCnsAttach` callers preserving their default normalization.
The extra unit's kind-5 material replacement (0x008EB920) reads its resource
name from descriptor 0x01838664. Other character parts have no override.

`DDVIEW_GOBLIN_EQUIPMENT_COMPARE` enables an explicitly labelled,
**render-only** comparison: one higher-equipment record from the installed
disabled group-24 roadside layout appears at its authored coordinates,
beside ordinary group-23 actors. It never joins `Fight`, opens layout gates,
or changes quests; it uses an isolated native appearance stream and is
retired on stage changes. Head approved this diagnostic in board #5841.

Actual viewer validation: 77 tests passed, 2 ignored (35.94 s compile,
9.86 s tests), followed by a successful release build (35.80 s). Then added
the installed renderer assembly test: it checks exact admitted mesh counts,
native attachment entries, replacement-material name coverage, and the
installed comparison record. The final focused two tests passed (17.94 s
compile, 2.66 s tests), followed by a successful release build (43.25 s).
All completed with exit 0. The first assembly-test compile required
unwrapping Bevy 0.19's `SystemState::get_mut` validation result; corrected.
No new warnings remain. No viewer window or original-game process touched.

Acceptance still open: after S31's NPC Guard regression is fixed and merged,
integrate the committed fix and run **full ddenemyrt/ddhost/final ddview
suites** (head #5837), then queue one bounded checkpoint capture with the
diagnostic. Guest appearance replication belongs to S44. Weapon switching
(0x008D73A0 toggles actor +0x6020 at frame 78; 0x008D7550 at frame 11),
dropped equipment, other goblin kinds, and original visual comparison remain
explicit follow-ups, not claims of this initial-placement checkpoint.
