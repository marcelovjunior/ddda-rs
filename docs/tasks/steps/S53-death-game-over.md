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

## Results: core checkpoint ready for review; production S53 incomplete

Branch `step/s53-death-game-over` has a tested core/network checkpoint on
source `486219519af13d48136e11c34f5e608aae5a8a56`. Reviewed main
`6afbc3c` (S52 chatter/protocol 10) and `5192882` (S56 EDIT) were
integrated before the final gates. This is not the whole brief's Done gate:
production retry presentation, native pawn revival contact and game-over
GUI/comparison remain open.

The checkpoint implements the ordinary grounded collapse/Dead and pawn
down/return motion phases using actual installed damage-bank layers for
all three starting kits. Zero-HP/fatal replacement clears queued actions
and outgoing melee/skill/bow activations; existing projectiles remain
independent until an attempt replacement. The host observes actual roster
Arisen HP, blocks personal input/orders/doors, retains death action/clock by
persistent profile across reconnect, and counts the native delay only after
actual Dead entry. Active survivors continue; disconnected profiles do not
veto game over. Unsupported air, held and stone paths remain explicit errors
and never turn into a grounded branch after landing. Personal death does
not affect the shared authored-camera input lock.

Optional per-profile VITL retains current/max/white HP and
current/base/level stamina. Reflection `0x00785790` and original save XML
source these fields; level stamina is a gain added to the body-weight base.
Chunks validate before mutation and apply after fighter construction, both
during load and reconnect. Stored-base/installed-table disagreement is
reported and retained. Legacy saves without VITL retain installed/progression
reconstruction. Public saves refuse seated dead actors and omit dead parked
VITL; internal durable export allows real all-dead retry and deterministic
dead-host hashing. White HP/item-recovery ceilings are retained; generating
the native damage wound pool and pawn save vitals remain open.

Dedicated `Host::prepare_retry` constructs a fresh independent host through
a factory without Clone bounds on story data/world. Every fallible check
precedes the exclusively borrowed `PreparedAttempt::commit_retry`, an
infallible ownership swap. Live roster seats/holes/profiles/control persist;
checkpoint profiles restore durable state and absent active/parked profiles
retain live state. An absent active profile requires an observed installed
checkpoint party place. Dead absent newcomers revive at installed/progression
maxima. Checkpoint shared Rift slots remain authoritative; conflicting
absent-owner hires use ordinary departure bookkeeping and a typed notice
without invented RC/items, preserving the restored Rift room and hirers.
Fight/projectiles/enemies/pawn brains, Sim time/actions, private input/order/
interaction queues and attempt locks are reconstructed. Reviewed S52
`fork_for_retry` retains installed data/RNG/monotonic speech IDs while
retiring its attempt queues. EDIT restores by profile, retains absent and
legacy records, rejects malformed records atomically and cannot follow a
reused seat.

Protocol 11 carries death body tag 19, authoritative per-profile DeathReport,
and attempt epochs on inputs, snapshots and reliable story/cutscene/pawn
messages. Clients infer no game-over/spectator policy from HP or motion.
A compact reliable RetryNotice precedes replacement whole snapshots; stale
epochs/acks/events and duplicate/old Welcome cannot roll back the attempt.
Server preparation stages exact existing Link clones under an exclusive
borrow, retaining connections, reliable IDs/sequence/ack state and transport.
All notices/looks/stage validation and allocation happen before commit;
commit clears commands/baselines/history without rebuilding Server/socket.
Only head-delegated mechanical viewer patterns changed: PawnMessage struct
matches in net.rs/opening.rs and PlayEvent accepting the epoch field.

All final gates ran against the same clean source above:

- Full core command:
  `cargo test -p ddsim -p ddenemyrt -p ddhost -p ddnet --no-fail-fast --target-dir E:/ddda-rs-wt/item-use-review-target -- --test-threads=1`.
  Session 77480 exited **0**, with **460 passed, 0 failed, 2 existing ignored,
  0 filtered**, across 79 summaries (including four empty doc suites).
  All **47 S53 regression names passed exactly once**, with no SKIP/errors.
  Suite durations summed to 666.60 seconds. The existing ignored tests are
  `installed_reynard_rescue_contact_trace` and `debug_trace`.
- Full downstream command:
  `cargo test --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target -- --test-threads=1`.
  Session 6166 exited **0**: **123 passed, 0 failed, 2 existing ignored,
  0 filtered**, in 86.74 seconds. The ignored tests require private human
  joint-remap/camera reference poses.
- Default build:
  `cargo build --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`.
  Session 56491 exited **0**. Jobs were capped at two and normal global
  core/M5 leases were released only after actual exits. Existing viewer
  private-interface/dead-code warnings remain; no warnings were suppressed.

Private evidence stays outside the repo:
`E:/ddda-re/notes/steps/S53/full-core-4862195-gate.private.json` records the
source, exact 47-name manifest and core log hash
`2942FEBFE1652917A087969C87BCCDABC64835AD23D6E3B1611B048BAE23734F`.
`viewer-release-4862195-gate.private.json` records viewer/build actual exits,
log hashes, clean source and matching built/private executable.
The viewer log hash is
`EAD459BADDFE222336EC4611D74AF2A54703E39A30D8BD2030028E801F1390C5`;
the build log hash is
`551166EDC0639CD34CD4C82DAEB295F0C7F5B3545D3031D3BCECDCAA3031B00C`.
Private pin `ddview-s53-core-4862195.exe` is **95,747,072 bytes**, SHA-256
`8839B5E3FB1B17B499C0BA5B06C0A5AFABEB13F995B3EFF9C1DD746AA9E886D6`.
No GUI/capture/original-game run is claimed for this pin.

Failures were preserved and corrected before full reruns: an initial
shared-cache API mismatch (only owned source timestamps refreshed, content
unchanged, no clean/new target); a fixture accessing private Runtime state;
a chatter assertion that omitted the prior real profile-leave lifecycle;
a real reconnect bug applying VITL before final stamina construction; and
an existing legacy-health assertion expanded to both VITL and explicit
no-VITL saves in both seat orders. Exact six-field assertions remain intact.
The earlier helper run (178c1a0/session 67590, 149 passed) remains evidence
for its original ten names, superseded by the final full gate above.

Head owns the next production viewer milestone (board 8307). Its resources
must be staged before commit: installed terrain/camera/start positions and
restored looks, fresh OpeningWorld, followed by infallible ownership moves
retiring cut/talk/NPC adapters/guest bodies/streamer lifetimes. Calling
fallible `enter_stage` after swapping the host would mutate live state and
clear chatter a second time. PreparedAttempt exposes candidate() and
notices(); its candidate provides saved story stage/start, exact roster/
party/Sim places, EDIT/progression/equipment. RetryNotice carries epoch,
frame, observed Spot, restored looks and owner-filtered dismissals.
Spot.cell comes from actual staged terrain/grid data. Resource preflight
outside the exclusive Server borrow must check roster/epoch freshness.

Remaining native limits: living revive-probe activation/Check-owner geometry,
real pawn contact admission/HP restoration/get-up arbitration, airborne/
stone/special fatal branches and external signed actor completion;
latest/checkpoint load consumers after native aGameMain teardown; authored
game-over scheduler/atlas geometry and Wakestone branch. Confirmed option
mapping is latest, Normal, Easy, checkpoint, title; 0x0064FB10 changes
difficulty, and controller codes 0x25/0x26 request latest/checkpoint.
Native teardown 0x0050EF80 -> 0x0050B5E0 removes aGameMain at 0x0050B67E.
Pawn near-death CMC is situation six/list one, not priority; conscious and
special huge-death situations must preserve their native action contexts.

Next action: head integrates this reviewed core checkpoint and stages the
production retry/game-over presentation; native pawn-contact tracing and
comparison remain separate unfinished S53 work. No main edits, pushes,
installation modifications, computer-use or original-process control were
performed by this agent.
