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

Viewer patch proposal for head's currently owned `opening.rs`, against
`18a4ca5` lines 1214 onward (not applied): replace the `stage == STAGE`
shortcut in `resume_checkpoint` with the following after a successful
dedicated host attempt replacement:

```rust
let (stage, start) = (s.world().stage_no, s.world().start_pos);
enter_stage(stage_view, st, s, stage, start, true)?;
```

This rebinds same-stage collision, camera and NPC lifetimes too. It is only
the stage-reentry hook: old fight/projectiles/AI, enemy admission/view maps,
event/talk/menu actors and network interpolation still need fresh attempt
lifetimes. The current `Host::load` before it does not provide that reset,
and a fallible stage load after a host swap is not atomic presentation.
Native reload tracing supports rebuilding the world: `0x0050EF80` waits
for fade/resources, checks the latest/checkpoint request masks at
`0x0050F0E4` / `0x0050F0F3`, then changes to controller state `0x24` at
`0x0050F2B1`. That state dispatches to `0x0050B5E0`, which retires its
temporary GUI objects and asks the area manager to remove `aGameMain`
(`0x0198CDF0`) at `0x0050B67E`. The save read/reconstruct consumer after
that lifecycle boundary remains open.

Dedicated retry continuation is **source-only, uncompiled and untested**.
Under head's explicit approvals 7929/7952, `Host::prepare_retry` constructs
an independent host through a fresh-world factory with no `Clone` bounds
on the story data/world. Every chunk/resource/placement check happens on
that candidate. `PreparedAttempt` borrows the live host exclusively while
presentation prepares; dropping it preserves the live attempt, and
`commit_retry` performs an infallible host swap. The network server lives
outside this host and must retain its connections. A checked monotonic
attempt epoch is prepared for the pending protocol-11 reset notice.

This is project reconstruction policy, not a claim that the original had
multiplayer retry. The live roster retains exact seats, holes, profiles and
controls. Present checkpoint profiles restore checkpoint durable state;
absent seated/parked profiles retain their live durable chunks. An absent
seated profile requires a caller-observed checkpoint party place rather
than its former stage position or an invented offset. Checkpoint shared
Rift slots remain authoritative. An absent seated profile's hire survives
only in a free unreserved slot; collision uses ordinary departure log and
history bookkeeping and emits a typed profile notice, without invented RC
or items. Its bookkeeping temporarily detaches/restores the candidate Rift
room so it cannot decrement counters or selections for another restored
pawn. Native inventory ownership reclaim remains outside `Rift::release`'s
existing contract and is not claimed by this retry work.

All bodies and fractional sim time are reconstructed, private host input
edges/order/interaction/request/vocation latches clear, and combat is
rebuilt from the installed data after load. Old enemies, projectiles,
fighter actions and pawn brains cannot be carried by the factory result.
Seven additional retry regressions are source-present: failed factory/late
chunk/stage preparation and dropped candidate preservation; exact roster
holes and all-seat action reset; absent seated durable state/party place;
absent parked durability/place; free hire versus colliding ordinary
dismissal; deliberately poisoned factory hidden queues; and installed
dead-seat/projectile/enemy/pawn-brain replacement plus resumed movement.
There are now 26 named S53 regressions in source, ten actually passed at
the helper checkpoint and sixteen continuation regressions **not run**.

Pending dependencies/limits: reviewed S52 chatter `fork_for_retry` must
preserve installed data/RNG/monotonic message IDs, and reviewed S56 EDIT
must receive retry durability assertions. Protocol-11 epoch/input/snapshot
baseline reset, the production installed party-placement/fresh-world
factory, atomic viewer resource preparation/replacement and native save
choice consumers remain open. Existing save policy reconstructs combat HP
from installed kit/stats; native saved HP/stamina durability is not proven.
Pawn revival contact selection, airborne fatal branches and authored
game-over presentation also remain open. No new Cargo/default-release or
original comparison result is claimed for this continuation.

Head approved a further source-only VITL extension (8028/8037/8055/8082).
Native `cSAVE_DATA_PARAM` reflection at `0x00785790` and save XML retain
Arisen current/max/white HP plus current/base/level stamina. Level stamina
is a gain added to the body-weight base, **not** maximum stamina; the port
retains all six fields and uses their sum for the runtime stamina maximum.
The optional per-profile chunk is parsed with all other fallible work
before host mutation, then applied after fighter reconstruction. An absent
chunk preserves legacy reconstruction. Stored bases differing from the
available installed weight-class table are logged and retained rather than
silently changed. White HP is now retained in the actor and item recovery
uses the existing sourced white-ceiling helper. Damage wound-pool reduction
remains untranslated; this is not a claim that new damage generates the
original wound pool. Pawn vitals remain a follow-up.

Public checkpoint/autosave refuses a seated dead profile with a typed error.
The internal durable export remains available for all-dead retry and never
turns failed public saves into empty hashes. A dead newcomer absent from
the checkpoint is revived at installed/progression maxima for the fresh
attempt; a living newcomer keeps its live vitals. Public saves omit VITL
for disconnected dead profiles, allowing later admission after checkpoint
restoration to rebuild living vitals. Internal export preserves those dead
values for live merging. Checkpoint VITL with nonfinite/invalid fields or
nonpositive HP is rejected atomically by retry.

Six more installed regression names cover exact six-field/per-profile and
parked round trips, legacy absence, malformed atomicity/zero-HP checkpoint
rejection, absent dead versus living retry, stored-base diagnostic retention
and omitted dead parked VITL with refilled readmission. The existing parked
hash test was updated to require saved-health changes while keeping status
transient. The all-dead installed retry test now also exercises public-save
refusal and deterministic dead-host hashing. All of this remains
**uncompiled and untested**, with 32 S53 regression names in source (ten
helper names actually passed, 22 continuation names not run), plus the
updated existing profile regression to execute in the full touched suites.

Protocol-11 continuation is source-only. Inputs, snapshots and reliable
story/cutscene/pawn messages carry an attempt epoch. The client waits for
the reliable compact RetryNotice before admitting a replacement epoch,
retires prediction and delta baselines, requires fresh same-stage collision,
and projects terminal/spectator/game-over policy solely from the host's
authoritative per-profile DeathReport. Body HP and motion never infer that
policy. Invalid reports and baseline epochs are rejected. Server preparation
clones the exact existing Link state under an exclusive mutable borrow,
preserving connection IDs, reliable IDs, sequence/ack state and transport;
all notice validation/allocation happens before the host swap. Commit clears
old commands, acknowledgements, history and active event state without
rebuilding Server/socket. Preserved old reliable gameplay is explicitly
epoch-filtered, and a duplicate/old Welcome cannot undo an admitted retry.

Thirteen additional network regression names cover notice owner dismissals
and looks, whole/delta epoch rollback, malformed command ranges, DeathReport
round trips/invalid policy, same-stage client reset, stale snapshots,
profile-based policy, preserved connections, failed/dropped retry and stale
input/ack rejection, plus encoded-packet old gameplay and Welcome/notice
reordering. The source total is 45 S53 regression names: ten helper names
actually passed and 35 continuation names **uncompiled and unrun**. Reviewed
S52 main `6afbc3c` is integrated and its actual `fork_for_retry` preserves
installed chatter data/RNG/monotonic IDs while retiring attempt queues.
S56 reviewed-main integration and EDIT retry assertions follow next.

The mechanical viewer `net.rs` pawn-message pattern is updated under the
parent's normal scoped claim. Required head-owned `opening.rs` patch (not
applied here): in `consume_client_events`, change
`PlayEvent { row, frame }` to `PlayEvent { row, frame, .. }`. Production
retry resource preparation, fresh-world factory and presentation retirement
remain open, so this is not runtime/UI readiness. No additional Cargo,
release or original-game comparison result is claimed.
