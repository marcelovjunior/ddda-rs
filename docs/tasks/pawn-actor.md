# pawn-actor: Saved and story pawns as active characters

- **Owner:** codex-1 (head assignment #2233)
- **Branch:** `task/pawn-actor`
- **Board area:** `fight/pawn-actor`
- **Status:** partial implementation; baseline record/equipment adapter ready for review

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

Implemented in `ddenemyrt::pawn_actor`: an owned saved-record snapshot per
host actor ID, active equipment-slot resolution, enhanced main/offhand
weapon values, armour defence and signed slash/strike/element resistance,
and the defence/element contributions of both accessory instance blocks.
Baseline `PlayerAttacker`/`PlayerDefender` inputs retain saved growth.
No level-1 kit is substituted. Data comes from the installed item tables,
enhancement resources and nine-slot defence selector in the executable.

Tested: three synthetic regressions cover slot replacement, enhancement
replacement, accessory write ordering, unavailable-item observations,
signed resistance/capping, learned skill levels and independent actor records. One installed
regression loads all story records 10..14, verifies their saved fields and
equipment conversion, and retains Rook's Mage level 6. All four pass in
release mode. This is not an original-game comparison or a playable pawn.

The item exclusion predicate `0x004647e0` is an explicit caller input;
the installed fixture enables its starting gear and does not validate the
general exclusion predicate. Body/motion construction, AI, active status
modifiers, augments, special equipment skills, inventory-bag attachment and
host integration remain unimplemented here. Accessory fields for other
consumers are retained but not applied to HP/stamina/attack. These APIs
provide baseline values, not fully modified combat actors.

Source survey:
The isolated `SavedPawnBase::skill_level` query translates `0x00780ba0`
with bit getters `0x0077c390`/`0x0077c400`. `0x00446a30` copies the saved
level-one/two bitsets into active info +0x730/+0x768; the skill state begins
at +0x72c. The third bitset (+0x7a0) is supplied explicitly as a runtime
equipment observation. Skill text metadata is read from `ExeTables`:
missing text means level zero; text flag 0x8000 distinguishes weapon skills
from core skills. A core skill accepts either learned bit. A weapon skill
uses level two before one; an enabled equipment override selects level
three only if at least one learned bit is present. The caller may ignore
equipment overrides, as the source's argument permits.

Disassembly of `0x00542620` selects AI parameter tier two for level >=3,
tier one for level two, otherwise tier zero (also when player-info is
absent). The saved-record query and tier selector are separate from AI
execution. Installed basic staff command 0xb4 has text index -1, so its
level is zero and it selects tier zero even when equipment bits are set.
The installed regression verifies this for all story records 10..14.
Other Mage commands still require their individual behavior translations.

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

Staff input lead: `cCmcMagicUserCombo::vf17` (`0x00557040`) initializes
three remaining presses (+0x280) and zero submitted shots (+0x284).
Its update (`0x005570d0`) has its own state machine, rather than the sword
combo template. In state 2 it submits through `0x00536940` (owner input
object +0x30), increments the shot count, decrements remaining presses,
and arms a 90-frame timeout for the next action acknowledgment. Subsequent submissions
check the actual `cPlActWpnWandShot` action and its +0x84 shot count;
time passing alone does not acknowledge a shot. `uCmc::vf200`
(`0x00b537d0`) copies that input word to actor +0x32f0 before the common
player pipeline. The alternate state 3 sets actor input mask 0x810000;
its selection predicate and exact command meaning still need tracing.
Final state 4 waits for actor +0x2650 == 0 and cancel slot +0x25a4 == 2
before returning completion. Aim updater `0x005575e0` writes the target
at +0x2fa0 with stage-coordinate conversion. A Mage adapter therefore
needs shot-count, action, cancel-slot and target observations; replacing
the weapon goal number in the sword-only driver is insufficient.

The staff combo's range request is command **0xb4**, proven by ECX at
`0x00557075` before `0x00542620`. Selector `0x0047ed00` maps commands
0xb4..0xf9 to the bank at `sPlayerManager +0x8e0`, so this combo reads
record zero. Resource initialization `0x0047e2b0` loads the nine ordered
`AIPlayerActionParameter` paths into +0x8d0 onward, identifying +0x8e0 as
`AI\\AIPlayerActionParameter\\AIPlActParamWand`. It also proves the existing
sword bank at +0x8d4, whose `plact.rs` comment still labels the resource
binding guessed. The tier selector remains the pawn skill-state query
`0x00780ba0`; do not assume every saved pawn uses tier zero.

Next: bind these active values to the existing combat representation, then
trace the Mage virtual-pad decisions and motion/resource selection. Record
recruitment/prologue setup and the tested baseline stat/equipment adapter
exist; body/inventory/host integration and Mage pawn inputs remain incomplete.
