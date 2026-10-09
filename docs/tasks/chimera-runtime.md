# chimera-runtime: The prologue's multipart combat gate

- **Owner:** Codex lane (codex-1 survey; implementation staffing via head)
- **Branch:** `task/chimera-runtime`
- **Board area:** `fight/chimera`
- **Status:** open; survey authorized by head #2072

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

Initial source/data survey only. No chimera runtime is implemented by this
brief. The first implementation action is to identify the two child units'
life/death masks and the final persistent placement-kill producer.
