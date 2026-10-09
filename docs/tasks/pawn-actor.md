# pawn-actor: Saved and story pawns as active characters

- **Owner:** Codex lane; staffing via head
- **Branch:** `task/pawn-actor`
- **Board area:** `fight/pawn-actor`
- **Status:** open; brief and source trace authorized by head #2131

## Goal

Construct active pawn actors from saved or installed story `Cmc` records,
including equipment-derived combat stats, inventory and vocation-appropriate
motion/input. Rook (story stock 14, Mage level 6) and the prologue companions
must retain their actual records when entering the fight. Do not substitute
the current level-1 Fighter kit.

## Context and source leads

Read `docs/COOP_DESIGN.md`, `docs/PAWNS.md`, relevant `docs/COMBAT.md`, and
the upon-a-pawn/opening-prologue briefs. Existing
`ddstory::prologue::story_stock` reads the required records at runtime;
`ddhost::prepare_prologue` maps hero records per Arisen seat and installs
shared companion records. `recruit_story_pawn` installs Rook's record and
bag before ev15. These do not yet construct active bodies and stats.

`Fight::add_pawn` in `ddenemyrt/src/pawn.rs` currently uses a level-1 Fighter
kit and sword AI parameters. That limitation is explicit; extending it must
not silently reinterpret a Mage as a sword user. Preserve the existing
headless virtual-pad pipeline (`uCmc::vf200`, 0x00b537d0) and actor ownership.

Executable leads (2017-12-13 Steam; trace exact contracts before coding):

- Rook recruitment: event setup 0x0050c290 row 0x17 -> 0x00440190.
  Record copy paths include 0x0043e5b0 and 0x00447390.
- New-game setup 0x00445ab0: stock 10..13, active stat setup 0x00446860,
  inventory setup 0x004460d0. The override at player-info +0x344 is
  discipline (getter 0x0077c340), not experience.
- Rook items: 0x00440190 calls 0x0045ece0 and 0x0045e680. Verify equipped
  item flags, quantities, bag ownership and item-derived effects.
- Trace Mage priority goals/action parameters, staff motions and spell
  requests from installed AI resources. The existing sword-only bank is
  insufficient evidence for Mage decisions.

## Scope and coordination

Prefer isolated conversion code and tests under `ddenemyrt`/`ddpawnai`
and a host construction adapter. Claim exact paths; coordinate `fight.rs`,
`pawn.rs` and shared exports with codex-1, and `ddhost`/`opening.rs` with
upon-a-pawn and head. Rendering is presentation only. No simulation crate
may acquire Bevy, OS input or audio dependencies. Use existing shared build
targets by agreement; never build into the main worktree target.

Bulk records, gear, stats, motion tracks and action tables are read from the
player's install. Source-addressed small embedded constants and short
overrides may be translated with regression tests. Private dumps and game
files stay outside the repository.

## Done when

1. One reusable conversion accepts a saved/story record and installed data,
   preserving level, vocation, equipment, skills and inclinations. Unsupported
   vocation/skill behavior is explicit; no fabricated stats or Fighter fallback.
2. Rook and each required prologue companion construct the correct body,
   life/stamina pools, inventory and supported input driver. Mage attacks and
   support behavior use sourced parameters and actual learned/equipped skills.
3. Host-owned IDs and one shared pawn party work with N Arisen records.
   Recruitment does not duplicate an actor or reset its bag; stage transfer
   retains the same identity and state. Ownership never relies on a global
   player or conflates Arisen IDs with enemy/shell IDs.
4. Deterministic tests cover installed Rook and prologue records, independent
   owners, no duplicate recruitment, and supported Mage input/combat behavior.
   Source-derived formulas have regression tests with boundary cases.
5. A bounded viewer check shows Rook after recruitment and the prologue
   companions after startup using these records. Report what was implemented,
   tested and compared with original evidence separately. Missing spells,
   presentation or AI behaviors remain documented blockers.

## Results

Source survey only; no active actor implementation in this branch.
0x00447390 runs edit conversion 0x00780fa0, parameter conversion 0x00446a30,
equipment effects 0x00446860 and skill refresh 0x00780350, then copies pawn
type, knowledge and inclination records into the active pawn-info record.

0x00446a30 copies saved vocation +5 to active +0x10 and saved level +4 to
active +0x700. The saved HP/stamina/base attack and defence fields at
+0x188..+0x1ac are copied to active +0x29c..+0x2c0; experience +0x1b0 goes
to +0x2c4. Saved discipline +0x60 goes to +0x344. These are existing saved
base values, not a request to reconstruct a level-1 kit. It also expands
signed skill slots, copies learned-skill bitsets and rebuilds status arrays;
invalid status kinds outside 0..39 are cleared. Equipment effects remain
a separate pass: 0x00446860 scans 12 valid equipped slots, resolves each item
through 0x00459340 and calls 0x00442fe0/0x00442d50 after resetting its output
fields. Consumers and derived formulas still require tracing.

Next: bind these active values to the existing combat representation, then
trace the Mage virtual-pad decisions and motion/resource selection. Record
recruitment/prologue setup already exist; body/stat/inventory conversion and
Mage pawn inputs remain incomplete and untested by this source-only survey.
