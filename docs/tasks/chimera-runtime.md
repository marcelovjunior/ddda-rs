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
Shared parameter loading now accepts explicit common/region resource paths,
including both child suffixes. One installed regression verifies legacy common
stats equivalence, all three chimera kind-zero pools against parsed resources,
and missing-path errors. The existing em#### identity validation stays intact.
`ChimeraData::load` now constructs all three installed bodies and combat stats;
an installed regression checks motion-bank order and independent owner pools.
Scale and collision rates/flags are explicit caller inputs. Actor initialization,
action stepping and host placement binding remain outstanding.
Latest validation: `cargo test -p ddenemy -p ddenemyrt` passes 51 tests
(10 parameter-reader and 41 enemy-runtime), with one existing ignored diagnostic.
Follow-up: installed body-specific magick bonuses and their guarded
post-mitigation modifier are implemented; 13 chimera integration tests pass.
The modifier keys on the resolved pool ID, not the raw collision region.
An additive configured combat pass now supports explicit owner mappings,
ordered immediate linked-hit relay and a post-mitigation callback, preserving
the legacy wrapper. Six further synthetic regressions cover nonidentity
mapping, three simultaneous owners, ordering, equivalence and atomic errors.
Full `cargo test -p ddcombat -p ddenemyrt` passes 81 tests (33+48), one ignored.
The callback's current `has_hp` observations are explicitly distinct from the
original actor +0x5bd0 death marker. Source timing is confirmed: common vf72
calls vf119 (0x00ab37d0), setting that marker before the persistent-kill call.
Synchronous accepted-death policy observation is now exposed; host binding remains unimplemented;
unmapped ordinary configured hits return a preflight error.
Review follow-up restricts the modifier callback to configured enemies;
20 chimera integration tests pass, including a mutating callback that must
leave an unconfigured enemy's ordinary damage unchanged.
The additive `EnemyHitPolicy` observer runs after each local and mirrored hit,
before the next one. Twenty-one chimera integration tests pass, including
accepted versus priority-blocked child death with identical zero HP and the
resulting difference in the later body modifier. No live host wiring yet.
`ThinkInitialization` reads body/child controller roots with the shared walker,
preserving selected current versus fallback default/base and manager lock/scale
initialization. Its installed test and 15 ddai library tests pass. The Wait
action prefix and unit-specific think/action stepping remain next.

Codex-1 independently reviewed the retained initialization checkpoint
against `0x009a0880`; all 22 chimera integration regressions passed. After
bringing current main into this task branch, strict ddenemyrt library
clippy passed with the existing combat callback type-complexity allowance.
The release viewer build remains queued behind the active lane builds.

No host combat actor, placement kill, story progression or original-game
comparison is claimed by this checkpoint. Next: implement the body/child
Wait action prefix with their separate motion and think owners, then bind
the existing damage policy and linked-hit relay to host-owned actors.
