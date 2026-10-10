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

### Next actor/Guard integration scope proposal

- Reviewed main's current startup: `stage_npcs` discovers daily FSM files
  and explicitly ignores placement gates; `spawn_npc` completes ordinary
  movement immediately. Neither establishes a resolved live placement.
  The movie/editor hookup now lives in main's intro module (42e05b9);
  no viewer file was edited for this proposal.
- Smallest truthful lifecycle hook: allocate a headless NPC-intel normal
  consideration controller on explicit placement initialization, even if
  its daily graph is absent/disabled, matching `0x00bbd930` before
  `0x00bbdaa0`. Its lifetime is separate from graph availability and
  applied action lifetime. Forward readiness only for that bound resolved
  actor/intel; unload and replacement retire it. Existing `spawn_npc`
  success alone can prove a loaded Core, not target presence.
- Proposed files: ddnpcrt/src/lib.rs, new ddnpcrt/src/guard.rs and
  tests/guard.rs, plus owned host story_npcs/host/opening/tests. Add a
  generic authored-resource/conversation entry point to the existing
  per-seat talk runtime, with explicit loaded resource and eligibility /
  activation observations. Candidate reservation and active talk stay
  separate. TalkEvent::End alone must not pulse TalkEnd: the actor cleanup
  and controller-enabled predicates remain necessary.
- `0x00bace50` constructs quest text paths
  id/npc_wind/npc_quest_talk/qQQQQ_nNNN, but preloads only quest ids listed
  for that NPC by executable table `0x0163cdf0` (u16 lists terminated by
  0xffff; ids below 128). This is bulk runtime data, not a constant list
  to copy. A supplied loaded-resource registry is a valid narrow boundary;
  exact installed preload construction still needs a runtime table reader.
- API audit found replacement binding invalidated observations but left
  the old actor's applied action runnable. The owned fix retires old
  applied/pending state before replacing identity; it does not apply the
  old graph to the replacement. All eight host story_pawns regressions
  pass after the fix, including replacement retirement.
- Runtime preload feasibility verified read-only: existing ddexe checked
  PE access can read the whitelist directly. Installed and unpacked list
  bytes match for NPCs 1, 2 and 304; q2 is listed for 1 and 304, not 2.
  The current installed controller regression's NPC 2 is deliberately a
  daily-graph suspension fixture, not a claim that its actor owns q2 talk.
  Scope extension for ddexe dependency is proposed on board #2328.

- Replacement regression now changes identity while the old applied graph is
  actively waiting for talk. A later old-actor TalkEnd cannot emit movement,
  Finish or GSF381 during 600 further frames. All nine host story_pawns tests
  pass; the separate normal-controller restoration regression remains intact.

### Guard resource checkpoint (codex-31)

- Added typed per-NPC loaded-resource selection from 0x00bb47d0 and
  0x00baddf0: quest keys, base key -2, override key -1 only when the
  authored conversation exists. Missing quest resources do not fall back.
- Quest preload lists now read 0x0163cdf0 through ddexe from the player's
  installed executable (0x00bace50), plus generic authored talk-set loading
  and a per-seat authored conversation start preserving the NPC lock.
- All ten ddnpcrt tests and touched-crate clippy pass, including installed
  NPCs 1/304 q2 resource loading, NPC2 preload exclusion, fallback selection
  and two-seat isolation. No actor eligibility, activation, TalkEnd cleanup
  or full q2 route is inferred from loading or closing text.
- Remaining: real controller lifecycle, actor/Guard adapter and frozen NPC
  variable providers. FreeFlag getter 0x007abda0 uses independent 128-bit
  storage at +0xbc; Hit getter 0x007ab860 reads actor vf180 word bit7;
  NpcGoodsHaving 0x007ac320 tests actor pointers +0x6f44/+0x6f48 after
  NPC-owner validation. These variables are traced but not yet translated.

### Frozen NPC variable checkpoint

- Added FreeFlag for both action and daily consideration graphs: four private
  words initialized by 0x007a74f0, index-limited getter 0x007abda0 and
  setter 0x007abdd0. It is independent of the FreeCondition word.
- Hit (flag) and NpcGoodsHaving use explicit actor observations with the
  sourced predicates above. Intel-only owners return false; actor adapters
  must provide their current flags/goods, not infer them from movement.
- Twelve ddnpcrt tests and touched-crate release clippy pass, including
  128-bit boundaries, owner/flag/goods cases, installed schedules and q2.
  The baseline viewer remains the earlier executable and still freezes
  seven NPCs; rebuilt viewer comparison is pending. Other unsupported
  variables may still stop schedules; no full NPC behavior claim.

### Tutorial route integration baseline (2026-10-09, codex-31)

- Ran the 87843ee viewer plus codex-29's opt-in diagnostics at 1920x1080,
  unattended. Intro run loaded st100ev00 (109.6 s movie), skipped after
  2.4 s by unattended policy, auto-confirmed edit and reached stage200 /
  SceNo1100. It saved a screenshot and exited; not a full movie acceptance.
- Separate existing demo completed in about 224 wall seconds: Fighter class
  select, ev10, Quina talk, collision-planned walk, ev15, then stage200 /
  SceNo1400, GSF372/381 true, q1 words [18000000,0,3], q2 booted.
  Seven step screenshots retained privately. Both processes exited.
- This is not the requested end-to-end tutorial. Main skips intro when
  demo is enabled; prepare fast-forwards beach dragon loss before rendering;
  the vocation menu remains the labelled two-choice stub; opening setup
  passes literal None fight data and no pawn/goblins. GSF381 still comes
  from the scripted actor stand-in, not the new Guard actor adapter. Demo
  ends before the actual Cassardis exit and the goblin rescue encounter.
- Baseline also reproduced seven NPC schedule freezes from the missing
  variables now translated in18f1466. New viewer validation is pending.
  Concurrent builds exclude performance acceptance; no original-game
  comparison claimed. Logs/captures: E:/ddda-re/notes/opening-route-codex31.
- Next action: resolve actual NPC layout targets and own intel/controller
  lifetimes, then connect guarded dialogue and sourced actor cleanup. Keep
  fight data and spawned enemies authoritative in the host, not a second
  independent viewer Fight.

### NPC placement / controller lifetime checkpoint

- Added generic _n layout candidate reading through existing GPL/LOT readers.
  cLayoutSetNpc vf12 (vtable01593f30) resolves to75faa0 shared gate.
  Record identity uses SetInfo+4 -> intel5c8 (7624d0), while mNpcId
  goes to intel3e4 (7a32f0); they are never interchangeable.
- Installed checks establish stage200 group1 record0 is NPC39, not NPC1.
  Stage100 group4 record0 is NPC304. Earlier q2 graph tests using target
  group1/NPC2 were fixtures; actual first-pawn script target was subsequently resolved below. No production binding hardcodes those fixture identities.
- Npc::intel owns consideration/action fields without requiring a loaded
  daily graph (bbd930 before bbdaa0). Explicit load_story_npc_intel creates
  that lifetime, retains allocation after graph-load failure, invalidates
  replacement/unload, and reports actual presence/readiness to live worlds.
  HP, body presence and NPC life are deliberately not inferred.
- Candidate loading is not materialization: 762000 name-family selection,
  intel-pool capacity and archive readiness remain adapter prerequisites.
  The API is not yet invoked from viewer startup; GSF381 remains fixture.
- Unsupported host graphs now halt their graph execution and retain the
  controller owner, instead of deleting it. This is port error recovery,
  not an original completion. A synthetic error regression covers it.
- Passed: placement/lifecycle3, story_pawns9, host NPC2, ddnpcrt12 tests.
  Targeted host/NPC clippy has no new warnings with the pre-existing
  install.rs chunks_exact lint allowed; broad all-target checks also find
  the pre-existing replay.rs modulo lint. Viewer release build passed; private checkpoint binary copied.

### Host NPC single-step follow-up

- Host now steps each NPC once and handles its error in place. The old
  aggregate-then-retry path lost requests from preceding owners and advanced
  them twice when a later owner failed. Failed graphs still halt while their
  allocated owner, last position and error diagnostic remain.
- Two-owner regression proves the healthy entry request survives a later
  unsupported process and does not repeat; the failing owner stays parked
  with one diagnostic. Test fails against the previous implementation (lost
  Place request) and passes with the fix.
- Passed placement/lifecycle4 and story_pawns9 tests; targeted host clippy
  passes with only the already documented install.rs lint allowed.
- Next: resolve real q2 target/materialization prerequisites before wiring
  Guard talk to the route. Installed NPC1 candidates include stage200 group27
  record0 (authored q0002_n01); this is not yet a verified script binding.

### First-pawn placement binding and materialization trace

- Installed scr\st200\fsm\bootev_st200ev15, state `apply fsm`, applies
  q0002_n001_AI to kind5/group27/index0. The matching stage200 NPC layout
  record is NPC1, with authored initial action q0002_n01. Added that identity
  to the installed-data regression; group1/NPC39 remains an independent
  record-identity check, not a q2 actor binding.
- Correction to materialization prerequisites: 0x004a76a0 is the 200-entry
  intel allocation/eviction pool, not a persistent-life predicate. It searches
  empty entries, then states3/4, then lower-priority entries. NPC vf15
  0x00762000 selects priority101 or103. Life observations remain independent
  and unresolved; no persistent-life gate is claimed from this call.
- 0x007627e0 selects the longest case-insensitive name prefix from the runtime
  table at0x0150f6f0. The stage100 NPC archive branch waits/queues when the
  per-NPC archive reference exists; stage200 bypasses that branch.
- 0x007a32f0 copies authored FSMPath into intel+0x3f0 via0x00bbf400;
  0x00bbdaa0 starts that authored script before attempting the independent
  daily controller graph. The current host placement API still only retains
  authored_action metadata: script lifetime integration is the next action.

### Initial authored-script lifetime checkpoint

- load_story_npc_intel now queues nonempty placement FSMPath through the
  existing applied-script owner. Source0x00bbdaa0 calls0x0059e260 before
  normal daily graph loading; that function allocates cAIFSMCtrl with the
  cAIActionFSMDemo wrapper. The independent daily owner remains suspended
  and available for restoration by the existing Finish path.
- Already queued explicit ApplyFSM supersedes the initial placement default.
  Regression covers both initial script startup and explicit replacement,
  proves the suspended unsupported daily graph is not stepped, and verifies
  unload does not resurrect either owner. Placement5/story_pawns9 pass;
  targeted host clippy passes with the documented pre-existing lint allowed.
- This API is still not called automatically by the viewer. Next action is
  materialization adapter integration, followed by real Guard interaction;
  no route-completion or original-game comparison claim.

### NPC name-family reader

- NameFamilies reads the null-terminated table at0x0150f6f0 from the installed
  executable. Selector translated from0x007627e0 chooses the longest
  case-insensitive nonempty prefix, retaining the first equal-length match.
  0x00762000 identifies MultiNpc by record address0x0150f700.
- Synthetic prefix/tie/no-match regression and installed normal/multi/unknown
  family checks pass, along with all five placement tests. Targeted clippy
  passes with the documented pre-existing lint allowed. No bulk table copied.
- This closes name selection as a materialization prerequisite; intel-pool
  lifecycle and the stage100 archive readiness adapter remain separate.

### Materialized-set reconciliation

- Added host reconciliation for the adapter's complete materialized placement
  set per stage. The adapter must resolve actual loaded cells, group gates,
  name family, intel-pool admission and archive readiness before calling;
  passing raw candidates does not meet that contract. No admission or
  streaming-radius policy is invented by this API.
- Unchanged owners retain execution state; removed/changed placements retire
  before new owners load. Duplicate identities, invalid stage/id and external
  owner conflicts are validated before retirement. Ordering is stable.
- Added stable story_npc_intels iteration independent of normal/applied graph
  maps, so presentation can keep actors while their normal graph is suspended.
- Regression covers reordered snapshots, retained position, removal, empty
  stage set, duplicate rejection and external-owner conflict without partial
  retirement. Placement6/story_pawns9 pass; targeted clippy passes with the
  previously documented lint allowed. Release build pending shared target.

### Guard arbitration and actor completion observations

- DialogueSlot translates candidate reservation0x0043dd80 and promotion
  0x0043ded0 while tracking the interacting PlayerId alongside actor lifetime.
  Reservation success can leave another candidate unchanged; promotion rejects
  that mismatch. Same-actor seat ownership is an explicit co-op extension.
  Matching-owner clear follows0x0043dfd0; retirement invalidates only the
  removed lifetime. Original helper audio/global side effects are not ported.
- TalkEndObservation translates both independent branches of uNpc::vf111
  0x00bb3290. Pending actor+0x586a must equal1 and requires enabled controller
  and game flag0x100000 clear; only that branch consumes pending. Separately,
  actual talk action0x03000041 with an empty active-dialogue slot also emits
  the frame's observation. Earlier completion prose omitted this second
  branch. Unknown observations cannot satisfy either branch.
- These contracts accept actual actor observations; UI End never calls them
  implicitly. They do not implement spatial/global eligibility, actor talk
  action transitions, or the exit helper's controller reenable side effects.
- Two new regressions cover competing seats/actors, candidate success versus
  activation, lifetime retirement, blocked pending exit and both completion
  branches. All four Guard release tests pass; ddnpcrt clippy --no-deps
  -Dwarnings passes. Latest viewer release build is running.

### Applied graph error isolation

- Unsupported authored/applied graphs now halt in place and report one
  diagnostic without aborting the whole host frame or publishing Finish.
  The normal controller remains suspended; the materialized actor stays
  owned. This is explicit port error recovery, not original completion.
- halt_graphs also clears an uncommitted Finish bit from a partially processed
  frame. Regression covers pending completion suppression and two applied
  owners where the healthy request survives the later owner's failure.
- Passed ddnpcrt lib7, placement7 and story_pawns9; targeted clippy passed
  with the documented pre-existing host lint allowed.
- Viewer release through951b5f0 passed and was copied privately. Current
  adapter work follows head2565 with codex29's separate route harness hooks.

### Cached loaded layouts and canonical NPC positions

- LoadedLayouts caches successfully parsed NPC records for supplied loaded
  cells; group gates remain reevaluated from current story state without
  repeated archive reads. Tests verify a true group flag cannot admit a cell
  the adapter did not supply, and LOT32 admits the actual group27 NPC1 record.
- Materialized NPC position now lives in the host independently of normal or
  applied graphs. Placement initializes it, committed Place requests update
  it, and explicit mover arrivals update it without presentation interpolation.
  It is retired with the owner and participates in stable replay hashing.
- Position/ownership/cache checks and existing story-pawn regressions pass
  (placement7/story_pawns9); targeted host clippy passes with the documented
  pre-existing lint allowed. Viewer adapter WIP is separate and compiled;
  GUI smoke waits for codex29's uncontended route timing run.

### Guard motion prerequisite (codex-31)

- Translated 0x00bb4f90 into TalkMotionObservation: mode zero permits
  the motion transition; otherwise actual active motion and actor flags
  bit2 are required. Unknown observations do not authorize the transition.
  The same predicate gates real talk exit in 0x00bb41c0. This is one
  prerequisite, not complete Guard eligibility or automatic UI cleanup.
- All five Guard release tests and ddnpcrt clippy --no-deps pass.
  The predicate remains ready for actor-state adapter wiring.

### Cassardis NPC viewer materialization checkpoint (codex-31)

- Opening now caches actually parsed whole-stage Cassardis NPC layouts,
  evaluates their authored gates each host tick, recognizes installed name
  families and reconciles materialized intel owners. Model enumeration follows
  those owners even when their normal graph is suspended. It no longer creates
  every daily-FSM filename as an NPC with an invented one-HP lifetime.
- Place requests use canonical host positions; Guard configuration is retained
  and Goto waits for a real headless mover. Visual interpolation does not report
  arrival. Stage unload retires Cassardis owners. Field NPC admission still
  awaits actual layout/archive readiness; MultiNpc and pool priority eviction
  remain explicit unsupported cases. Current Cassardis admission covers the
  fresh-slot path within the original 200-slot cap, not a shared eviction pool.
- Release build and viewer clippy passed (existing warnings only). Bounded
  1080p unattended demo smoke drew 31 initial NPCs, completed weapon-table
  interaction and started ev10. Screenshot at 25 seconds shows SceNo1100,
  cutscene row22 frame597, GSF372=true and GSF381=false. No timing acceptance
  was taken while another build ran. Private screenshot/logs remain outside
  the repo; no original comparison or full-route acceptance is claimed.
- The run exposed q0001 NPC1/NPC2 applied actions requiring missing
  cAIActionFSMDemo.SceNo; they quarantine without losing their actor. Nine
  daily Goto requests correctly remain pending. Guard actor eligibility,
  actual talk cleanup, headless locomotion and field NPC admission are still
  outstanding. Next: translate the source-backed SceNo variable, then rerun
  the bounded route to expose the next real prerequisite.

### Shared SceNo in NPC graphs (codex-31)

- cThinkFSM reflection 0x007a82d0 directly binds getter0x005baf00 for SceNo
  (sGameSys+0x30). Both normal and applied NPC owners now receive the host's
  current scene each frame. Missing observations remain errors; no default
  scene or actor completion is manufactured.
- Eight NPC library tests, two installed applied-action tests and nine host
  story_pawns tests pass. The q1 NPC1/NPC2 installed graphs now read SceNo
  and reach their next unsupported actor process, Set Disable Flag, without
  emitting Finish. This is not full q1 action support.
- Touched-crate clippy passes with the pre-existing install.rs
  chunks_exact_to_as_chunks warning allowed; that unowned code was unchanged.
- Viewer follow-up65b3b73 removes a rendering-membership filter from scenario
  NPC observations. All admitted canonical intels now contribute positions,
  even without a rendered body. Heading still uses authored placement yaw;
  canonical direction updates from Wait/Place remain unresolved.

### Authored disable flags on materialized NPC intels (codex-31)

- Process registration0x007a74f0 binds Set Disable Flag to0x007aa840.
  The callback maps twelve authored bits and ORs either a valid NPC's vf180
  word or, without that body, intel+0x5c0. The mapping is a translated
  instruction switch, not copied bulk data. Constructor0x00bbbfa0 initializes
  intel+0x5c0 to zero; intel tick0x00bbcb80 clears a separate bit0x400.
- The NPC runtime emits typed DisableFlags requests. The host records them
  on its materialized intel lifetime, includes them in replay hash, preserves
  them across unchanged placement snapshots and clears them on retirement.
  Actor-body flag copying and gameplay consumers are still unimplemented;
  this checkpoint does not infer active/searchable status from these flags.
- Nine NPC library tests, two installed applied-action tests and eight host
  placement tests pass. Installed q1 NPC1/NPC2 now emit disable requests and
  wait at scene1100 without false Finish or unsupported-process quarantine.
  Host regression covers same-owner retention and replacement reset. Clippy
  passes with the same existing install.rs lint allowance.
- Next: source the corresponding clear/set boolean flag callbacks used after
  the q1 scene transition, then actor eligibility and real Guard completion.

### Boolean actor flags and linked-body storage (codex-31)

- Added typed boolean callbacks for SuperArmor0x007aa1f0,
  NoSurprise0x007aa3a0, LiftDisable0x007aa430,
  NoLiftReaction0x007aa4c0, NoEscape0x007aa5e0,
  LiftEnable0x007ab460 and FixPos0x007aa790. They set or clear one
  sourced bit without losing unrelated flags. These do not complete motion
  interfaces or infer actor eligibility.
- Resolved the previous body-copy uncertainty: uNpc::vf1800x00bb4730
  returns linked intel+0x5c0 when body+0x5854 exists, otherwise body+0x2738.
  All three NPC-derived vtable entries traced resolve to that getter. Thus
  linked bodies and intels share canonical flags directly; no copy bridge is
  required. Gameplay consumers of those bits still need their own translation.
- FixPos(false) on a real body additionally changes body+0x1df0 from zero
  to13. The typed request preserves its identity for that future body adapter;
  the intel-only callback has no such side effect. This body-ground-mode
  operation remains unimplemented and is not silently inferred from a mesh.
- Ten NPC library tests, two installed applied-action tests and eight host
  placement tests pass. Installed q1 NPC1/NPC2 survive SceNo1100 to1200,
  emit initial disable writes and later flag clears, and do not falsely
  Finish. The host regression checks selected-bit clear/set, unrelated-bit
  retention and retirement. Touched-crate clippy passes with the previously
  documented install.rs lint allowance. No new viewer run yet.

### Guard candidate cleanup and unit readiness (codex-31)

- Added matching-owner candidate removal from0x0043de30, called by
  Guard dispatch0x00bb0470 when the actor loses eligibility. It cannot clear
  another seat's candidate or an active dialogue, and never emits TalkEnd.
- Guard's0x0049ace0 unit readiness consumes cUnit+4: low status bits must
  equal2 and bits0x4800 must both be present. This differs from generic
  cUnit::vf02 at0x00403830, which accepts low bits1 or2. The new helper
  preserves unknown status as unavailable; it does not infer readiness from
  NPC intel flags or a rendered mesh.
- Further constructor trace: uNpc ctor (instruction0x00bab3b1) sets motion
  mode+0x57e4 to3 and intel link+0x5854 to null pending setup. Do not default
  its motion mode to0. Clarified that bb4f90 false chooses Guard's manual
  candidate path when permitted; true chooses automatic launch. False is
  not universal ineligibility, although actual talk exit does require true.
- Seven Guard release tests and ddnpcrt clippy pass. Body initialization,
  eligibility observations and active action cleanup still need integration.

### Checked materialized actor lifetime observations (codex-31)

- Added deterministic host lifetime tokens for each new materialized intel.
  Repeated identical placement snapshots preserve the token; retirement and
  same-id/same-target reallocation change it. Tokens and allocation counter
  participate in replay hashing. This is a co-op identity mechanism, not an
  invented original-game field or an address copied from the game process.
- Checked arrival and TalkEnd observation APIs reject retired owners. The
  regression keeps a real replacement applied owner active, proves stale
  observations cannot mutate it, then proves the new token is accepted.
  The existing synchronous fixture methods remain for explicit current-owner
  calls. Nine host placement tests and targeted clippy pass.
- Scope limit: an actor lifetime is not an individual movement command.
  An asynchronous mover must additionally validate interface/command identity
  when an action changes within the same actor. Viewer/Guard wiring will use
  these tokens instead of equating an NPC id with a permanent actor.


### Wounded NPC callbacks and bounded route follow-up (2026-10-09)

- Added typed NoMotionTalk (0x007aa280, bit 0x01000000) and NoDamage
  (0x007aa0d0, bit 0x08000000), registered by cThinkFSM at 0x007a74f0.
  Both set/clear the exact NPC intel flag word; linked bodies return that
  same word through 0x00bb4730. Damage/motion consumers remain separate work.
- Installed NPC44 q0052_hurt_03 now executes these callbacks and stays active
  for 90 ticks without unsupported-process failure or synthetic Finish.
  Validation: ddnpcrt 10 unit + 3 installed applied tests; strict crate clippy.
- Combined doors/main47d00ac private binary, --opening --demo --unattended
  --perf --size 1920x1080, bounded 180-second capture: stage200, SceNo1300,
  GSF372=1, GSF381=0. WalkOut declares completion while the player remains
  indoors at (7854.979,1731.179,-6936.855); Q0002Event targets
  (-879.392,-8.178,250.404) and incremental planning does not advance.
  Private evidence: notes/opening-route-codex31/npc-route-doors.png and
  npc-route-doors.stderr.txt. This is a route blocker, not proof of walkout,
  quest2 completion, field entry, or full opening gameplay. No timing gate.
- Hand-reviewed doors38cf0b8 duplicate-method removal: retained
  sync_npc_intels/npc_requests are identical to the existing NPC checkpoint.
  Next: lifetime-bound presentation cleanup, then real actor/movement/Guard
  prerequisites; route planner doorway handling needs its owning lane.


### Viewer lifetime ownership follow-up (2026-10-09)

- Viewer request caches now follow all canonical NpcLifetime owners,
  independently of successful model creation. Retiring or replacing an
  unrendered actor clears its Guard/Goto/Place caches as well.
- Render bindings use the same allocation token, so replacing the same NPC
  id at the same authored target retires the old body instead of reusing it.
- Validation: release ddview check passes (three existing warnings).
  Host token rejection already has the active replacement regression in
  0edc13e. No live movement/Guard completion is introduced by this adapter.


### Check QuestFlag observation (2026-10-09)

- Translated cThinkFSM registration 0x007a74f0 callbacks 0x007aadd0
  (enter) and 0x007aae20 (poll), confirmed with read-only disassembly.
  Enter clears private FreeFlag; poll refreshes from quest bit through
  0x004356c0, ActType0 matches set and ActType1 matches clear. Invalid
  quest>=128, bit>=96 or destination>=128 does nothing; other ActType does
  not change the destination. This writes private FreeFlag, not quest state.
- NPC step_observed reads the current host World by reference for controller
  and applied action graphs; all three host/runtime stepping paths feed it.
  Standalone step remains available and errors if a graph needs absent world
  data. No false quest state is manufactured.
- Tests: 11 ddnpcrt unit, 4 installed applied, 9 NPC placement, 9 story-pawn
  pass. Clippy ddnpcrt/ddhost passes with existing chunks_exact_to_as_chunks
  and type_complexity allowances. Installed NPC3 q0002_n3 passes the quest
  process and reaches the explicit Check Player Distance observation gap,
  remaining unfinished; this is not full actor/route acceptance.
- Task-only main merge 66c425b preserved one NPC helper pair plus doors.
  Next: source player-distance actor observation and actual Guard readiness.


### Actor-distance and movement source prerequisites (2026-10-09)

- Combined release binary at checkpoint30dd8ea builds in 37.70s, two existing
  warnings; private copy notes/opening-route-codex31/npc-30dd8ea.exe.
- Check Player Distance registration (reflection around 0x007a94c8) binds
  getter0x007abe20. It requires valid owner +0x2a8 and body pointer plus
  player-manager +0x99c, then returns full 3D distance between positions
  +0xe0/+0xe4/+0xe8. A missing owner/body/player returns zero in the original.
  The port must distinguish an observed absent body from an unimplemented
  body observation. Co-op target selection is not defined by this single-player
  getter; no nearest-player or seat0 policy is silently inferred here.
- cAIActionInterfaceNpcSetGoto ctor0x00573400/vtable0x01577358:
  start0x00573630 initializes trace and returns pending4; update0x005736e0
  uses trace0x0057b9a0 and explicit actor states. Trace result4 can complete,
  but running with stop-motion enabled waits for actor motion completion;
  result3 continues movement. No fixed travel timer or 150cm/s interpolation
  is justified by these routines.
- Direct trace helper0x0057c760 compares full3D distance squared with authored
  trace arrival radius squared (+0x14), then performs collision/height/state
  checks for other cases. Stage offsets and route branches are explicit in
  0x0057b9a0; this is not a generic distance-only arrival implementation.
- Route harness inspection: opening.rs WalkOut transitions after SceNo1300
  plus six seconds, regardless of whether the actor left the interior. This
  explains the captured premature stage of the driver, without establishing
  the remaining doorway collision/planning cause. Reported to coordinator;
  benchmark driver remains outside this NPC edit scope.


### Batch handoff / still not accepted (2026-10-09)

Codex31 closes this batch at coordinator request; the brief remains open.
Checkpoint chain: 81cdd19 lifetime presentation, 30dd8ea quest observation,
bff6e07 source prerequisites (e5293d9 wounded flags precedes them).
All owned viewer processes have exited; core and M5 targets released.

Queued requirements:

- Guard: actual body eligibility/readiness and motion observations; distinct
  candidate reservation vs active owner; per-seat arbitration, authored
  resource selection, real actor cleanup feeding TalkEnd. UI end alone must
  never complete an NPC action.
- Goto: preserve authored trace/arrival/stop behavior, host-owned movement,
  route/collision/height checks and animation completion. Asynchronous arrivals
  need allocation lifetime plus current command identity; lifetime alone is
  insufficient when the same live actor receives a replacement command.
- Distance: distinguish unavailable actor observation from observed missing
  body; define co-op target policy explicitly before wiring getter7abe20.
- Materialization: stage100 needs loaded-cell/archive readiness and pool
  admission; actor position remains canonical, orientation is still authored.
- Route: fix WalkOut's six-second advance using actual exit evidence and
  investigate doorway interaction/navigation; current capture proves only
  scene1300 indoors, GSF381false. No quest2 clear or field acceptance claimed.

Coordinator queued road-to-encampment/actual-exit harness as the next route
priority. No further NPC edits or builds are active in this worktree.
