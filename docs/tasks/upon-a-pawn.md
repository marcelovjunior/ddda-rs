# upon-a-pawn: q0002 "Upon a Pawn": the first pawn and the goblins

- **Owner:** codex lane
- **Branch:** `task/upon-a-pawn`
- **Board area:** `story/q0002`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Trace and run q0002 from its boot (GSF 381): its scripts, QCT, events and NPCs. That covers the first pawn, the goblins harassing a woman, and recruitment into the party. The goblins use the existing runtime. The maintainer reports goblins "looked weird" in play: compare their look (shading, skinning, materials) against an original capture at the same time of day, and fix any defect or record why it differs.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddstory/**`
- `crates/ddhost/**`
- `crates/ddpawn/**`
- `crates/ddview/src/opening.rs` (claim)
- `docs/QUESTS.md`
- `docs/PAWNS.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

q0002 plays from its boot to its clear in `--opening`, the first pawn joins the party, and the state matches the original's. Fill in Results and report to `head`.

## Results

(filled in by the agent)

### Research checkpoint (2026-10-09)

- Installed `quest\q0002_b00` checks stage-100 enemy group 23,
  indices 0, 1 and 2, and NPC group 4 index 0. Its cleanup disables
  stage-100 layout flags 3, 4 and 7, removes NPC group 4 and enemy
  groups 23/24, clears q0002 bit 51 and raises SceNo to 1450.
- The QCT has seven rows. NPC 304 death cleanup uses judgment 18,
  selector 0/1, and only disables layout flag 100:7 after leaving
  stage 100. Implemented generic NPC death predicates 18/19 from
  `0x0078ece0`, getters `0x00435710`/`0x00435740`; two tests pass,
  including the installed q0002 table. No fake enemy deaths are supplied.
- Rook recruitment traced to event preparation `0x0050c290`, row 23,
  calling `0x00440190` before st200 ev15. Details in PAWNS.md.
- Still open: real encounter integration, field traversal, first-pawn
  dialogue, pawn party/body setup and original goblin visual comparison.
  The brief is not complete and q0002 is not yet playable through clear.

### Implementation checkpoint after cable recovery

- Event-table row 23 now emits `StoryPawn { stock: 14, slot: 1 }` before
  `PlayEvent`, once per preparation (`0x0050c290`). Host recruitment reads
  stock through the lightweight runtime reader, installs the shared Rift
  party record and resets Rook's bag to the five equipped items plus one
  item 64 (`0x00440190`, `0x0045ece0`, `0x0045e680`). It respects the guest's
  reserved slot and repeat delivery cannot duplicate inventory. Equipment
  derived active stats and a moving/rendered pawn body remain open.
- q0002 rescue is optional: installed `scr\st100\fsm\q0002_event2` sets
  bit 10 and boots q0003. The main script clears q0002 without any reported
  enemy death; it waits until leaving stage 100 before raising SceNo 1450.
  This is tested with explicit encounter facts, not timed fake kills.
- Implemented `IsDispCmcMess` from `0x005bb320`: missing message GUI is
  busy; otherwise priority-1 availability (`0x00634090`) must equal 3 to
  report idle. `HostEvent::PawnMessageSlots` supplies that observation.
- Also restores original new-game layout flag 380:0, from `0x00445be1`,
  coordinated with the prologue brief; its source regression belongs there.
- Tested: five story regressions and one host recruitment regression pass.
  Rook's record round-trips through the existing Rift save, and two seats
  share it. Not compared with an original recruitment capture; no claim
  that the whole quest or goblin fight is playable yet.

### Live encounter adapter checkpoint

- OpeningWorld can opt into EncounterFacts and bind resolved placement
  targets to authoritative goblin combat owners. StoryWorld observes HP
  before the story phase; loading and persistent kills remain explicit
  placement observations. Actor requests bypass ScriptedHost timers.
- Added the sourced stage-200 challenge hook after event processing and
  before scenarios (0x00504330 -> 0x0051ccc0), respecting stop bit 0x80000
  and resetting its baseline on stage load. Missing actors cannot pass.
- All three host story_pawns tests pass, including 1,000 frames without
  synthetic actor progress, source new-game lot 380:0, stage/stop/reset
  behavior, and installed Rook recruitment. Viewer activation remains
  pending real placement/AI adapters and head's stage handoff API.
- Review follow-up: live activation clears camera fixture callbacks,
  suppresses already queued synthetic quest/GSF/stage reports, and keeps
  actual reports in a separate queue. JumpArea now awaits actual stage
  loading in live mode. Four host tests pass, including pre-activation
  queued callbacks, Camera 17, handoff, and preserved scripted fixtures.

### Stage handoff checkpoint

- The opening now continues ticking after q0002 boots and handles JumpArea
  through Streamer::enter. It reads the destination's authored start
  position/angle/member offsets, replaces collision, places every roster
  seat, clears old stage NPCs, and reports StageLoaded only afterward.
  Stream focus follows the viewed seat and installs rebuilt field terrain.
- Manual stage acknowledgments are independent from live actor facts:
  the current viewer still uses ScriptedHost actor/AI stand-ins. This is
  traversal plumbing, not a completed q0002 or original encounter match.
  Guest world streaming on remote stage changes is still missing; all
  host roster seats receive authoritative destination positions.
- Verified release viewer tests: 35 passed, one ignored. All five host
  story_pawns tests passed, including stage acknowledgment without live
  actor facts. Release viewer build succeeds. No original visual comparison
  or end-to-end gate traversal has been claimed.

- Follow-up: cutscene configuration now uses the current host stage (or
  guest Welcome stage), with a regression for stage-dependent lighting.
  The HUD only reports q0002 activity; implementation limits stay here.
- Enemy identity is now sourced end to end: 0x00cc1f60 reads the layout
  record id into SetInfo+4; 0x007613d0 copies it to actor+0x20f0;
  0x007643d0 -> 0x004223f0 matches the script index without subtraction.
  The reusable bind_spawned_goblin helper uses this id directly. Installed
  stage100 group23 produces precisely indices 0,1,2 (regression passed).
- Latest verification: targeted release cutscene lighting test and all
  six host story_pawns tests passed. Neither binding nor live actor mode
  is yet connected to the viewer's encounter spawning.

### Applied NPC controller research (source only)

- Guard interface vf05 `0x00578ee0` configures NPC intel through
  `0x00bbf580`; it does not itself wait for dialogue. Reflected fields
  (`0x00578ad0`) map type/center/radius/direction/range/height plus message
  kind, message number, quest number, guard flag and jump destination.
  Type 0 clears intel+0x490; other types install that configuration.
- Guard trigger `0x00bb0db0` first checks actor/global dialogue eligibility.
  For type 1 it transforms the configured center by the NPC matrix,
  accepts vertical separation equal to the height limit, requires horizontal
  distance strictly below the radius and (when nonzero) the facing range.
  An existing trigger latch, current talk and unsupported actor conditions
  must remain explicit observations; proximity alone is not sufficient.
- `TalkEnd (flag)` getter `0x007ab890` reads actor+0x2ea3 only for a valid
  actor owner. `uCharacterBase::vf111` (containing `0x00850121`) clears it
  each update; `uNpc::vf111` (containing `0x00bb32cb`) sets it from an actual
  eligible talk completion or the completed talk action. It is a frame
  observation, not a permanent has-talked flag.
- NPC applied graph finishing differs from the repeating enemy driver:
  mode 0/4 post-update `0x0059e160` -> `0x0059e3a0`/`0x0059f3e0` ->
  `0x005a1300` sees SysCondition bit 0, clears wrapper active+4, stops the
  graph (`0x00e095c0`) and clears SysCondition. The next `0x0059e320` frees
  the inactive applied wrapper+0x58 and resumes the normal controller.
- Caution: the parent ApplyFSM result (`0x005c8460`, instruction
  `0x005c8572`) tests normal cAICtrl+0x4c, not that Finish bit directly.
  The exact relationship to regular-controller restoration is still being
  traced; no new completion shortcut has been implemented.

### Applied NPC implementation checkpoint

- Added generic Npc::applied and explicit one-frame TalkEnd input. Applied
  graphs reject unsupported processes/interfaces and unimplemented nested
  cluster memory rather than silently completing. Guard configuration is
  read from the graph and emitted for the actor/talk adapter; GSF writes
  and Finish follow sourced callbacks. No quest name is tested by runtime.
- New host story_npcs adapter queues ApplyScript until an explicit stage /
  target / NPC binding exists, pauses the normal controller, runs applied
  graphs in stable NPC-id order, applies their GSF writes and exposes Guard
  and movement requests. Actor arrivals and eligible talk ends are explicit
  inputs. It restores the retained normal controller after the sourced
  inactive-wrapper cleanup delay. Parent ApplyFSM readiness remains separate.
- Verified: all eight ddnpcrt tests and seven host story_pawns tests pass
  in release. The installed q2 action waits 300 frames without talk, rejects
  an early stale talk pulse, then writes GSF381 and finishes once after a
  real observation and explicitly simulated test arrival. Host regression
  additionally waits for a real binding and verifies Finish does not set
  parent ApplyFSM readiness.
- Still pending: live NPC placement, guard eligibility/talk presentation,
  actual NPC movement, parent ApplyFSM pointer-lifecycle predicate, viewer
  actor integration and original comparison. Thus q0002 is still incomplete.

- Follow-up verification: release viewer check passes with the new NPC request
  variants. The installed host regression now retains a real daily graph,
  proves it remains suspended throughout the applied action, and verifies
  restoration without an early normal-graph step on the cleanup frame.

- Parent ApplyFSM pointer trace: NPC materialization `0x00bbe070` writes
  actor+0x2e64 = intel+0x110 at `0x00bbe545`. Intel initialization
  `0x00bbd930` creates the daily consideration FSM at intel+0x15c, which
  is that controller's +0x4c. Applied loading `0x00bb45e0` retains it and
  uses the separate +0x58 wrapper. Actor detach `0x00bbe670` clears the
  actor/controller link. The parent check `0x005c8460` counts a present
  uCharacterBase-derived actor with nonnull +0x4c as ready, independently of applied
  Finish. Existing Host::applied_script_finished naming is misleading;
  a shared API correction and actor-type observation adapter are proposed,
  not implemented. No action-Finish shortcut should feed that predicate.
- DTI registry cross-check: the other present-target branch is uNpcIntel,
  and tests intel+0x15c directly, the same retained normal FSM pointer.
  Therefore body-loaded and intel-only NPC observations require distinct
  target resolution but share this pointer predicate; graph completion is
  neither. Controller allocation occurs before the daily graph load, so
  missing graph data alone cannot establish an absent original controller.

### Guard dialogue eligibility and completion observations

- Source `0x00bb0db0` performs eligibility before spatial checks: stage
  flags, active dialogue, frontend state (`0x00bb4000`), transition state
  (`0x0043f640` / `0x0043f6a0`), actor unit readiness (`0x0049ace0`),
  actor disable/status bits and a valid interacting character. Original
  character action ids 0xba, 0x33, 0x25 and the death predicate reject it.
  Unknown status-bit meanings remain unknown; proximity must not imply
  eligibility. The guard flag adds restrictions but does not alone choose
  the launch branch; actor predicate `0x00bb4f90` chooses that branch.
- `0x0043dd80` reserves a candidate at game-system+0x50, provided no active
  dialogue exists. `0x0043ded0` promotes the same actor to active dialogue
  +0x54 (rejecting a different reserved candidate). `0x00bb4060` enters
  talk action 0x03000041 only after promotion succeeds. Reservation,
  activation and completion therefore need separate host observations.
- Guard-authored message routing `0x00bb47d0`: kind 2 looks up the NPC's
  loaded text resource by configured quest number; kind 5 uses key -2.
  Kind 4 checks resource key -1 for the message and falls back to key -2
  (`0x00baddf0`). A found resource is passed with the configured message
  number to `0x00455990`. The quest-number argument for kind 2 and -2 for
  kind 5 are verified in instructions `0x00bb47ef` / `0x00bb480a`.
  Missing resource does not manufacture dialogue completion.
- Talk exit helper `0x00bb41c0` checks actual talk-active state +0x5864,
  waits for the actor-state predicate, clears active bytes, reenables its
  controller unless +0x6fba blocks it, sets pending exit +0x586a and clears
  the active dialogue through `0x0043dfd0`. The talk action's state 0x1d
  calls it, as do action replacements `uNpc::vf132` / `vf134`. Thus exit
  is not necessarily successful text consumption. `uNpc::vf111` consumes
  pending +0x586a only with controller enabled and game flag 0x100000
  clear, producing the already documented one-frame TalkEnd observation.
- Proposed host contract (not implemented): explicit actor identity and
  per-PlayerId interacting-character eligibility/transform; observed
  frontend/transition/dialogue availability; candidate and active dialogue
  actor ids; typed authored Guard config and trigger latch; actual resource
  availability; actor talk-exit pending/controller-enabled observations.
  The host arbitrates the original shared dialogue slot deterministically
  for N players. Arbitration policy is a co-op extension, not original
  evidence. Unknown observations stay unavailable, never implicitly true.
  Existing observe_story_npc_talk_end remains an explicit fixture/adapter
  input; no spatial or UI-close shortcut was added.

- Head-approved follow-up renames the shared predicate to apply_fsm_ready;
  ScriptedHost deliberately retains its fixture delay. The host now stores
  an explicit optional normal-controller allocation observation by bound
  stage/target. It is independent of loaded graph and applied Finish;
  replacement, unbind and stage cleanup invalidate it. No allocation or
  target presence is inferred, and full live actor wiring is still pending.
- Verification: 15 targeted ddstory/ddhost tests pass (encounter, both
  opening suites, story_pawns). Explicit allocation regression covers
  unknown versus false, graph-independent true, rebind/unbind invalidation,
  and stage cleanup even when NPC data is absent. Installed applied graph
  still proves Finish cannot synthesize the allocation observation.
