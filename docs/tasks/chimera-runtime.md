# chimera-runtime: The prologue's multipart combat gate

- **Owner:** Codex lane (codex-1 survey; implementation staffing via head)
- **Branch:** `task/chimera-runtime`
- **Board area:** `fight/chimera`
- **Status:** in progress; codex-28 implementation assigned by head #2090 / codex-1 #2098

## Goal

Implement the installed em5200 encounter at stage 380, group 10/index 0,
so the prologue's persistent-kill condition follows an actual combat death.
Preserve body/child-unit ownership and support all Arisen seats. A timeout,
missing actor, unloaded cell or scripted zero-HP placeholder must never
produce the quest kill fact.

## Evidence and dependencies

Read `docs/OPENING_SLICE.md`, the prologue brief and relevant `docs/AI.md`
sections. The opening-prologue survey found 18 stage-380 groups and 59
placements; later groups have different scene gates. Read the group and
placement records at runtime instead of copying the survey into code.
`set_03` checks persistent enemy death for group 10/index 0. Coordinate
the host observation adapter with upon-a-pawn/opening-prologue.

Initial executable leads (2017-12-13 Steam):

- DTI separates `uEm5200`, `uEm5200_00` and `uEm5200_01`, each with action
  and death classes. Resolve which child represents which part from code.
- Body think-table array: `0x016cd234`. Reuse the shared table walker.
- Body death selector `0x009a4dc0` examines masks at actor `+0x2e34/+0x2e38`
  before common enemy death, then selects local actions `0x3a/0x52..0x58`.
  Mask semantics remain unresolved; do not infer them from class names.
- `cEm5200ActDie` entry/update `0x009aec20/0x009aef20` select internal
  states and motions, reference child units at `+0x72e0/+0x72e4`, and
  eventually call cleanup `0x00aad0c0`. Trace the persistent-kill writer
  separately from motion completion and actor removal.

Bulk stats, motion/event tracks, hit volumes, spawn records and tables must
come from the player's install. Small embedded constants may be translated
with exact source addresses and regression tests. Keep private source dumps
and recordings outside the repository.

## Scope

Prefer isolated `crates/ddenemyrt/src/chimera.rs` and corresponding tests,
family-specific modules under `crates/ddai/src/thinktbl/`, and
`docs/CHIMERA.md`. Reuse existing body, damage, status, spawn and table APIs.
Claim exact paths before editing. Coordinate shared `ddenemyrt` fight/lib,
`ddhost` spawn/encounter adapters and shared `ddai` files with their owners.
Viewer hookup needs head's agreement; head owns streaming. Do not edit
MILESTONES/ROADMAP; report evidence and next action for head to fold in.

Root board #2175 additionally coordinates exact files
`crates/ddcombat/src/damage.rs` and `crates/ddenemyrt/src/combat.rs` for the
backward-compatible valued-hit API and mirrored linked-hit helper.

## Work and acceptance

1. Survey the actual prologue placement, body/child resources, life regions,
   action registration and selected think tables. Record unresolved behavior.
2. Translate damage routing, part states, action/motion transitions and final
   death selection into headless code. Preserve authored root movement and
   event timing; do not tune substitutes by eye.
3. Connect the placement to host-owned combat actors and the explicit
   encounter facts registry. Part disable/death and whole-encounter kill
   must remain distinguishable. Unload/reload must preserve existing kill
   state without inventing new kills.
4. Test a deterministic two-Arisen fight: both owners can damage the correct
   regions, partial part outcomes do not clear the encounter, and final
   combat death reports the original placement's persistent kill exactly
   once. Run the installed story order against these facts and verify that
   it stays blocked before the real completion and advances afterward.
5. With presentation hookup coordinated, run a bounded unattended prologue
   check. Compare motion/part/death observations against original evidence;
   report separately what is implemented, tested and compared. Unverified
   fidelity does not become a passing original-comparison claim.

## Results

Source checkpoint: isolated body/child death and parent-reaction selectors
are implemented with source addresses. Twelve targeted regressions pass.
`docs/CHIMERA.md` records ownership, common priority masks, child-01 guards,
the persistent writer chain, action registration, and damage-routing leads.
An installed catalogue reader resolves 317 action records and 147 resource
requests; the installed regression locates combat resources and parses the
three region files (5/4/2 sets). Runtime hit-region maps preserve pool IDs.
Source-traced CRT flag fixups retain 41/28/27 bound resources; on-disk
zero flag words are not mistaken for effective runtime flags.
The ordinary hit resolver now optionally retains valued damage for a separate
linked-collision helper; five synthetic regressions check no second mitigation,
main-only HP, mapping/attribute/no-death guards, status/break behavior and
per-attacker metadata. No link walking or host fight integration yet.
Validation: `cargo test -p ddcombat -p ddenemyrt` passes 72 tests with one
existing ignored diagnostic (33 combat and 39 enemy-runtime tests).
The normal body action 0x3a is a falling action before death action 0x57;
the 180-unit death cleanup timer is not the persistent-kill producer.

No host combat actor, placement kill, story progression or original-game
comparison is claimed by this checkpoint. Next: the child-00 collision-link
relay of already-valued hits to the body and exact body/child damage binding.
