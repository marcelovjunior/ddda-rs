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
full suites are pending. No runtime death, revival, GUI or retry completion
is claimed yet. An authorized narrow save-loader change parses
Rift, NPC row controls and player places before applying any candidate
state, so malformed late chunks cannot partially replace a retry. Its
regression passed in the first run; complete touched/downstream success is
still required in the coordinated full core window.

Next action: integrate the proven grounded action/pose chain and real pawn
revive interactions, then run all touched/downstream suites and the default
release build. Native GUI/save-choice tracing and original comparison are
still required. No game install/process writes, pushes or main edits.
