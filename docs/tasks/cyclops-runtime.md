# cyclops-runtime: The Encampment cyclops fight (q0003)

- **Owner:** codex lane
- **Branch:** `task/cyclops-runtime`
- **Board area:** `fight/cyclops`
- **Status:** in progress (catalogue/think initialization checkpoint)

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
0x009780d0 writes kind0x3f). Initializer0x00978820 registers115 action
descriptors at0x01847d38 (stride0x44, null terminator) and44 resource slots
at0x01849cf0, one null. The reader loads the43 resource requests from the
installed executable and applies the source CRT flag fixups at
0x01357a00..0x01357cfa. Only the common/attack/damage motion lists bind as
body banks; player/enemy interaction lists and child models are preparation
requests. Thirteen hit-region-to-pool identities are read from0x014ff680.

Think initialization at0x00978fxx uses default0x01734b48 and, when the
placement enables its first table, the selected pointer at0x01736ee8.
All six populated controller slots parse. The selected current/base remain
equal; default is overwritten with the fallback even in the enabled
branch. This distinction matters when resetting the manager. Invalid/null
roots return an error. No targets or action-completion observations are
invented.

The q0003 actor is in `scr\st100\etc\st100_63m52n_e20`, record0,
`cSetInfoEnemy5000`, rather than a stage210 placement. Group20 requires
layout flag30, has no scene window, and despawns on flag loss (mask1).
The authored Proof-of-Arisen flag is enabled, weapon type2, remaining HP
ratio0.2, XP500 and leg stagger/knockdown thresholds1500; first-think
selection is disabled. Initializer0x009790xx applies these special
encounter settings. They must be loaded from the placement, not made
family defaults or inferred from arrival at the Encampment.

Validation: one resource-fixup regression and one installed catalogue,
resource-resolution, controller-root and placement regression pass.
All43 resource requests resolve in the install. Targeted clippy passes
with only the existing `combat.rs` type-complexity lint allowed; strict
clippy reports that existing lint. No original-game fight comparison yet.

Still unsupported: action execution, armor/weapon children, clinging and
weak-point reactions, party/guard combat, rendering and host death feed.
The full brief remains open. Next: connect body/bank loading and the
source Wait/Common/Attack callbacks, then the placement-specific health
initializer and host lifetime identity before exposing an encounter.

Action source leads: Wait update0x0097ff30 requests bank0, sets body
flags2360/2370 bit0 and completes the action immediately; Common
enter0x00980fb0 requests bank1. Attack update0x00981000 tests both motion
completion queries and motion flags80000000 before setting action done,
and sets body flags101. Its query arguments still need disassembly before
translation. Base enter0x0097fd30 also applies per-action data tables;
these side effects cannot be skipped when building a faithful driver.
