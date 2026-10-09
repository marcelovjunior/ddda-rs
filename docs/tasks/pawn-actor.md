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

Equipment consumer follow-up: `0x00b87450` sums exactly nine equipped slots,
selected by the installed nine-word table at `0x015ea2a8` (slots 2 through
10 in this build), rather than all twelve saved slots. Arisen and pawn bags
are selected separately by actor flags and pawn index. Physical/magick
defence use `0x0045a1b0/0x0045a270`; slash/strike percentages start at 100
and subtract `0x0045a330/0x0045a3d0`. Element and status resistances start
at zero and accumulate their item getters. `0x00b87940` then adds the two
accessory bonus blocks at active-info +0x640/+0x6a0. Element/status sums
are capped above at 100; negative resistance values are not generically
clamped away. This is distinct from the twelve-slot equipment setup pass.

Weapon attack getter `0x004599a0` and defence getter `0x0045a1b0` decode
enhancement from the equipped item's flags when their level argument is
negative: bits 0x400, 0x200, 0x40, 0x20, 0x10 and 0x08 select levels
6 down to 1 in that priority order. Both call `0x0045b620`, which selects
the installed `rItemLevelParam` row by item kind and level-up type. Reuse
`dditem::lvparam` for those resources; raw `Item::attack()` alone does not
represent an enhanced saved weapon. The decompiler omits that helper's
floating-point result operations. Disassembly at `0x0045b6b5..0x0045b747`
resolves them: XMM1 retains the original base, while each matching entry
replaces XMM0 with `base * rate` (`0x0045b720`) or `base + rate`
(`0x0045b72b`, direct flag). The last matching entry across levels 1 through
the requested level wins; these changes do not compound. The getter
`0x00cbf910` writes only XMM0, preserving XMM1. No match, missing row or
unsupported item kind keeps the base. A level above 6 returns the original
base at `0x0045b744`, and invalid/unavailable items return zero earlier.
No enhancement formula was ported by this survey. The existing
`dditem::forge::level_stat` and `level_table` already implement this rule,
with `tests_forge::level_stats` covering replacement rather than
compounding. `dditem::rules::instance_level` already decodes the equipped
flags. Reuse these APIs rather than adding another enhancement calculator.

Existing combat types already separate saved base attack/defence from
weapon attack and equipment defence (`PlayerAttacker`, `PlayerDefender`).
`ArisenData::load` currently fills them from a level-table row and one
weapon, leaving armour contributions at defaults. A saved-record constructor
must supply the preserved base values and the sourced equipment aggregation;
reusing the existing kit constructor would lose both saved growth and gear.

Next: bind these active values to the existing combat representation, then
trace the Mage virtual-pad decisions and motion/resource selection. Record
recruitment/prologue setup already exist; body/stat/inventory conversion and
Mage pawn inputs remain incomplete and untested by this source-only survey.
