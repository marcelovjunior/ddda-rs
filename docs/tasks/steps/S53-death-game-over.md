# S53: When the Arisen falls: dying, revival, game over in the opening

- **Branch:** `step/s53-death-game-over`
- **Files:** `crates/ddenemyrt` / `crates/ddhost` (death state, pawn revive),
  `crates/ddview` (game-over GUI and retry), tests
- **Kind:** research + one feature

## Context

The party can now die in the opening (goblins hit hard, S49). The original
has a dying state (pawns can revive a downed Arisen? check: in DD the Arisen
dies at 0 HP while pawns can be revived), a game-over screen, and "retry /
load last save" choices. Today an Arisen at 0 HP just stays down. Co-op adds
a design question (one Arisen down while another plays): follow COOP_DESIGN
or record a decision.

## Do

1. Trace the original: Arisen 0 HP -> death action/motion, pawn downed state
   and revival by allies, the game-over GUI (class/layout) and its choices
   (load last save, retry from checkpoint...) and what each restores.
2. Implement it for the opening: death motions, pawn revival, the game-over
   screen, and "retry" via the host save/checkpoint (S46/S48) the way the
   original reloads.
3. Co-op: decide (COOP_DESIGN) what happens when one of two Arisen falls.
4. Tests: an Arisen at 0 HP reaches game over; a downed pawn is revived by
   another member; retry restores the last save.

## Done when

Dying in the opening behaves like the original (death, revival, game-over
GUI, retry), with sources and the co-op decision documented.

## Research checkpoint (in progress)

All addresses below refer to DDDA.exe 2017-12-13 Steam. Private exports and
disassembly stay outside this repository.

- Zero HP calls `uPlayerBase::vf72` at `0x00B7C470`, which sets the dead and
  pending-death actor flags. Damage dispatch `0x00B735A0` subsequently calls
  the final selector at `0x00B747D0`. A normal grounded Arisen or pawn selects
  action `0x1E`, `cPlActDmgCrumbleDead`. Linked actors, action locks and an
  already active fatal phase guard that selection. Stone/dead-down/special
  stage cases are separate branches.
- Crumble-death entry `0x00AD56F0` starts damage-bank motion `0x109`.
  Completion `0x00AD5AF9..0x00AD5B64` waits for the actual layer end or the
  actor's signed integer completion field to reach one. It requests action
  `0x26` for a non-pawn, `0x80` for an ordinary pawn, or `0x82` for pawn
  variant two. The common action dictionary is read from the player's exe;
  registration `0x00B608B5` supplies its address and record stride.
- `cPlActDead` entry `0x00AD7AB0` resets its clock. Its update `0x00AD7BF0`
  spends Arisen actor time only after entry, then requests game over once at
  or beyond installed adjust parameter `0x136`. It does not begin that
  delay directly at zero HP.
- Pawn near-death `0x00AFCE40` / `0x00AFD110` has motion phases before its
  expiration phase. Installed adjust parameter one supplies the limit;
  native scene/session/action flags pause it and `0x00B87D20` can reset it.
  Irreversible death or blocked revival still requests pawn loss.
- Rescuer action `cPlReviveCMC`, `0x00AD8950` / `0x00AD8A90`, queues a
  type `0x18` contact through `0x00AD8F10`. Recipient handling
  `0x00B6D54D..0x00B6D599` accepts that contact for a pawn only, with its
  native link/death/revival guards, and requests return action `0x81`.
  Return entry `0x00AFD8D0` restores HP from installed adjust parameter
  `0xFB` and the contact's recovery ratio. Ordinary curatives do not revive
  an actor. The separate non-pawn rescuer branch handles Wakestones; that
  original path remains unimplemented.
- Game-over GUI loading `0x0064E1D0` reads
  `id/DDN/gameover/gameover0_001`, localized game-over messages, and other
  installed resources. `id/gameover/gameover_01` and `_02` are schedulers,
  not GUI layouts. `_02` is selected for the Wakestone branch. Ordinary
  option dispatch `0x0064F270` distinguishes latest save, checkpoint,
  title and difficulty choices. Exact scheduler/atlas presentation and the
  save-choice consumer remain under investigation.
- The native airborne request `0x16` is abstract: damage dispatch
  `0x00B736D4` converts it into throw action `0x18`, variant two. Airborne
  fatal motion/physics is an explicit open item, not a grounded fallback.

Head approved the separate multiplayer rule in board message 7481, recorded
in COOP_DESIGN: dead Arisen spectate living seats, survivors continue,
disconnected profiles do not veto shared game over, and the host retries
all active players through an atomic shared save replacement. This is
project policy; the original supports one Arisen.

## Results (not ready for review)

Source helpers and ten named regressions are prepared in `ddhost::death`.
The first full host/network run on `db730fa` compiled in 45.71 seconds and
exited 101: nine new regressions passed, including whole-save preservation
after malformed late chunks; the installed-data regression failed because
it incorrectly required a positive game-over delay. Targeted resource
inspection confirmed a valid zero delay in this installation, so that test
now exercises the actual installed threshold inclusively. The corrected
full run on source `178c1a0a5fb5c0114508a8d3c2553f2860c2757b` completed
with process exit zero: 149 tests passed, none failed or were ignored,
across 44 result summaries (including two empty doc-test summaries). All
ten new death and late-chunk regressions passed against the installed
resources. The command was `cargo test -p ddhost -p ddnet --target-dir
E:/ddda-rs-wt/item-use-review-target -- --test-threads=1`; its private log is
`E:/ddda-re/notes/steps/S53/full-host-net-corrected.log`. This verifies the
helper and atomic-loader checkpoint only. No runtime death, revival, GUI,
default release build or comparison against the original is claimed yet.
An authorized narrow save-loader change parses
Rift, NPC row controls and player places before applying any candidate
state, so malformed late chunks cannot partially replace a retry. Its
regression passed in both full runs. Runtime retry still needs a host
checkpoint adapter, complete attempt-state replacement and presentation
restoration; the loader regression alone does not establish those features.

Next action: integrate the proven grounded action/pose chain and real pawn
revive interactions, then run all touched/downstream suites and the default
release build. Native GUI/save-choice tracing and original comparison are
still required. No game install/process writes, pushes or main edits.

Source-only continuation: the sim now has the ordinary grounded collapse,
Arisen dead, pawn down-start/down and pawn return motion phases. Durations
come from the installed damage bank, loaded for all three starting kits;
the fatal slot clears queued melee/skill/bow events and prevents zero-HP
fighters emitting new attacks. Four additional regressions cover actual
layer exhaustion, living-seat movement, pawn-only return admission, queued
attack cancellation and the installed bank. These changes have **not been
compiled or tested**; the new state also needs the approved network body
tag. Pawn return completion arbitration, external actor completion, fatal
root correction and airborne/stone variants remain open.

Native tracing further identified the option mapping by `0x0064F8D8`
and the installed message resources: zero is latest save, one Normal Mode,
two Easy Mode, three checkpoint and four title. The initial label of
`0x0064FB10` as a checkpoint selector was incorrect: it saves a difficulty
change (Easy sets save flag bit zero; Normal clears bit two). GUI exit
`0x0064F95A` reports `0x10` for latest/Normal/Easy, `0x11` for checkpoint
and `0x12` for title. The controller requests GameSys code `0x25` at
`0x0050ED76` for latest and code `0x26` at `0x0050ED98` for checkpoint.
Their load-side consumers and complete attempt restoration still need
tracing. Pawn near-death requests CMC
situation six, list one, at both `0x00AFCFBF` and `0x00AFD17F`; that is an
urgent request-list index, not a speech priority or ordinary cooldown list.

Grounded host continuation is also source-only: it observes roster Arisen
HP after the hit pass, cancels personal input before movement/orders/doors,
reports spectators by active persistent profile and starts the installed
delay after actual Dead entry. Profile reconnect preserves the collapse or
Dead phase/frame; a new profile reusing that seat is living. The port's
ordinary-grounded admission conservatively uses its mapped control domain
and scenery floor contact rather than claiming complete native actor-flag
coverage. Unmapped air, held and petrification paths remain visible errors
and never silently become grounded after landing. It does not change the
shared authored-camera lock or `Host::load` semantics.

Three installed host regressions and two network whole/delta/application/
malformed-payload regressions are drafted, bringing the additional source
tests to nine beyond the ten tested at the helper checkpoint. Network body
tag 19 carries all six grounded death phases. The protocol-11 transition
is pending reviewed chatter protocol 10 in main. None of this continuation
has been compiled or tested; full touched/downstream suites and the default
release build remain required. Ordinary save replacement still does not
reset a live attempt; dedicated reconstruction and viewer restoration are
open.
