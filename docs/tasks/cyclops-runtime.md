# cyclops-runtime: The Encampment cyclops fight (q0003)

- **Owner:** codex lane
- **Branch:** `task/cyclops-runtime`
- **Board area:** `fight/cyclops`
- **Status:** review (source checkpoints; full encounter remains open)

## Goal

codex-35 (road-to-encampment): q0003 at the Encampment is not just an
arrival. Event st210 ev00 is the *post-cyclops report*: its script waits
for SceNo 1800, the event ends at 2000, and bit 12 leads to 2100. Its
EnemyCheck targets kind 2 / group 20 / index 0, the Encampment's cyclops.
So the opening slice's last step needs the cyclops: its spawn (GPL
group 20 at stage 210 / the field cell), its AI and attacks through the
generic enemy runtime (ddenemyrt, like the goblins and the chimera), its
death reporting to the EnemyCheck, the guards and pawns fighting it, and
the clinging/weak-point rules the original applies to cyclopes.

Build the system, not the step: the cyclops family as a runtime enemy
usable anywhere it appears.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`. Test runs use
`--unattended`.

## Scope

`crates/ddenemyrt` (new cyclops module), `crates/ddai` if its think
tables need it, `crates/ddhost` enemy feed, the viewer's enemy drawing,
`docs/ENEMIES.md`, this brief's Results.

## Done when

In an `--opening` run at the Encampment the cyclops spawns where the
original puts it, fights the party with its original moves, and its
death drives q0003's EnemyCheck to SceNo 1800 and st210 ev00. Fill in
Results and report to `head`.

## Results

### Source checkpoint (codex-36, 2026-10-09)

Implemented `ddenemyrt::cyclops` for ordinary `uEm5000` (constructor
0x009780d0 writes kind 0x3f). Initializer 0x00978820 registers 115 action
descriptors at 0x01847d38 (stride 0x44, null terminator) and 44 resource slots
at 0x01849cf0, one null. The reader loads the 43 resource requests from the
installed executable and applies the source CRT flag fixups at
0x01357a00..0x01357cfa. Only the common/attack/damage motion lists bind as
body banks; player/enemy interaction lists and child models are preparation
requests. Thirteen hit-region-to-pool identities are read from0x014ff680.

Think initialization at 0x00978fxx uses default 0x01734b48 and, when the
placement enables its first table, the selected pointer at 0x01736ee8.
All six populated controller slots parse. The selected current/base remain
equal; default is overwritten with the fallback even in the enabled
branch. This distinction matters when resetting the manager. Invalid/null
roots return an error. No targets or action-completion observations are
invented.

The q0003 actor is in `scr\st100\etc\st100_63m52n_e20`, record 0,
`cSetInfoEnemy5000`, rather than a stage 210 placement. Group 20 requires
layout flag 30, has no scene window, and despawns on flag loss (mask 1).
The authored Proof-of-Arisen flag is enabled, weapon type 2, remaining HP
ratio 0.2, XP 500 and leg stagger/knockdown thresholds 1500; first-think
selection is disabled. Initializer 0x009790xx applies these special
encounter settings. They must be loaded from the placement, not made
family defaults or inferred from arrival at the Encampment.

Validation: one resource-fixup regression and one installed catalogue,
resource-resolution, controller-root and placement regression pass.
All 43 resource requests resolve in the install. Targeted clippy passes
with only the existing `combat.rs` type-complexity lint allowed; strict
clippy reports that existing lint. No original-game fight comparison yet.

Still unsupported: connected action execution, armor/weapon children, clinging and
weak-point reactions, party/guard combat, rendering and host death feed.
The full brief remains open. Next: connect body/bank loading and the
source Wait/Common/Attack callbacks, then the placement-specific health
initializer and host lifetime identity before exposing an encounter.

### Body and callback checkpoint; stand-down handoff

The catalogue now loads the body skeleton, three bound motion banks and
one body collision resource. Wait/Common/Attack callback primitives are
translated and tested. Wait update (0x0097ff30) sets body flags at
+0x2360/+0x2370 bit 0 and completes the action immediately; Common enter
(0x00980fb0, also inherited by Attack) requests its descriptor's motion.
**Correction to the initial source lead:** the request's second argument
is force-restart, not bank. Shared body vf36 (0x00886e70) preserves the
current motion when the ID matches and force is false. Wait uses false;
Common/Attack use true. uModel::vf30 (0x0105b480) decodes a packed ID as
bank `(id >> 8) & 15`, slot `id & 255`; the installed attack `0x164`
therefore resolves to bank 1, slot 0x64. A failing installed regression
exposed and corrected the earlier interpretation before this checkpoint.

Attack update (0x00981000) and Common update (0x00996f20) complete on
active motion +6 bit 2 or motion flags 0x80000000. Disassembly confirms
the two calls to 0x010570f0 query the same active motion object. Attack
ORs body flags 0x101; Common ORs 1. These primitives do not implement
base-enter state or a live unit driver. The loader plus two callback/fixup
unit tests and the installed integration test pass; targeted clippy
passes with only the pre-existing `combat.rs` type-complexity allowance.
Full `cargo test -p ddenemyrt -- --test-threads=1`: 67 passed, one
pre-existing pawn debug-trace test ignored. No viewer build was needed.

**Resume in `crates/ddenemyrt/src/cyclops.rs`:** implement base action
enter 0x0097fd30 before connecting the simple callbacks. It clears several
body action fields and reads per-action tables at 0x014fee64/0x014fee68.
Then implement the placement-specific health/leg initializer in
0x00978820 and the actual think/target driver. Generic callback wrappers
0x00987e20/0x00987e60 delegate IDs >=1000 to common enemy handlers;
local checks 0..4 are in 0x00987e90, with helpers 0x00987f10/0x00987fb0.

Death must preserve a family-specific ordering: 0x0097ca10 invokes common
uEnemy death **before** priority-2 action arbitration. A blocked death
animation request must not suppress death reporting. Motion/frame helper
0x0097e6a0 reads 14 interval rows at 0x015bfc88 and falls back to actor
+0x623c. These are source leads only, not translated death runtime.
No host/viewer hookup, live fight comparison, or q0003 completion is
claimed. Claims and build target are released at stand-down.
