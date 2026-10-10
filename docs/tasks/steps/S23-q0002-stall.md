# S23: The opening stalls at SceNo 1400 after ev15 (TOP PRIORITY)

- **Branch:** `step/s23-q0002-stall`
- **Files:** most likely `crates/ddnpcrt` (NPC variables/processes),
  `crates/ddhost/src/story_npcs.rs`, tests
- **Kind:** diagnose and fix the tutorial's current blocker

## The blocker

The ordinary opening route (`ddview <nativePC> --perf-route`, 600 s
allowed) plays ev15 (row 23) and then stays at SceNo 1400 at about
(-825, -66, 236), the q0002 step, and never reaches the Cassardis gate or
the field (codex-1, S13 Result follow-up; log
`E:/ddda-re/notes/steps/s13/route-600s.err.txt`). Head's own run shows the
same: the demo's `Q0002Event` step targets (-879, -8, 250) and nothing
advances.

Leads from the logs (observations, not proven causes):
- NPC 37: unresolved Attack flag after ev15.
- NPC 34: process "Set Invincible" not translated
  (`AI\FSM\Npc\st200\quest\q0002_n34`).
- NPC 3 / NPC 8: NPC variable `cAIActionFS...` not translated
  (`q0002_n3`, `q0052_n008`).
- NPCs awaiting headless movement (Goto) to their targets.

## Do

1. Find what the original needs to leave SceNo 1400 (q0002's script: the
   condition that moves it on, ddquest/ddstory), and which of the NPCs
   above it waits for.
2. Translate the missing NPC variable(s)/process(es) on that path from the
   exe (no shortcut that sets the flag or SceNo directly).
3. Re-run the route and report how far it now gets.

## Done when

The route passes SceNo 1400 by the authored mechanism, tests cover the
translated callbacks, and the Result names what was missing and where the
route stops next.

## Result (codex-54, 2026-10-10 UTC)

- Implementation: `bfedf90` plus lifecycle correction `f509db7` on
  `step/s23-q0002-stall`. Head approved the
  related `ddhost/src/npcs.rs`, `ddnpcrt/src/lib.rs` and viewer `opening.rs`
  edits in board message 3726. They connect authored dialogue resources,
  actor exit and the ordinary interaction/demo path; no menu/subtitle edits.
- The missing dependency is **NPC 1**, stage200 group27 record0. Installed
  `ai\fsm\npc\st200\quest\q0002_n001_ai` places this actor, enters its
  force-talk Guard (kind1, quest2, conversation1), waits `TalkEnd (flag)`,
  then executes its authored GSF381 write. Installed
  `scr\st200\fsm\bootev_st200ev15` waits GSF381 and boots quest2. The
  existing viewer stored Guard requests but never dispatched their
  authored conversation, and its demo only talked to Quina. NPC3/8/34/37
  errors and unfinished Goto requests are separate, still unresolved.
- Guard text uses the player's installed `id\npc_wind\npc_quest_talk`
  resources and executable preload whitelist at `0x0163cdf0`, via
  `0x00bace50`; resource selection follows `0x00bb47d0`. Missing quest text
  cannot fall back to base text. The critical NPC1 quest2 conversation is
  selected by its authored number, not a copied line or a new flag write.
- Spatial Guard checks translate the non-angular kind1/kind2 portion of
  `0x00bb0db0`: local center transform, strict horizontal radius, inclusive
  kind1 height and the intel trigger latch. Unknown angular/nonspatial
  guards, override resource registries and jump talks remain explicit gaps.
- Dialogue ownership uses the existing source-derived slot reservation,
  promotion and matching-owner release (`0x0043dd80`, `0x0043ded0`,
  `0x0043dfd0`). The host retains the NPC interaction lock and story's
  active-dialogue stop observation after text End until the actor exit phase.
  Lifetime replacement/unload retires only the matching owner; seats cannot
  steal an active conversation. A departed seat cancels its Guard ownership
  without producing TalkEnd; another seat can retry the manual Guard. The
  original manual branch leaves its trigger latch clear, while the
  motion-ready automatic branch sets it. Retiring a completed actor cannot
  abort an unrelated conversation that its former seat started later.
- Source refinement: `uNpc` constructor `0x00bab3a0` initializes motion
  mode3, so the adapter does **not** default it to0 or claim its motion
  finished. Normal `cNpcActTalk::vf06` (`0x00b9a430`, state0x1b) releases
  the active dialogue after the stock release checks `0x0044a140` and
  `0x00449f40`. Its plain, non-jump text path owns no appearance stock
  transactions; the empty-list case of `0x00788bb0` succeeds. This release
  precedes motion-gated cleanup `0x00bb41c0` (state0x1d). The independent
  `uNpc::vf111` fallback (`0x00bb3290`) observes current talk action
  `0x03000041` plus the genuinely empty shared dialogue slot and emits
  TalkEnd even while motion/controller readiness is unknown. Text End alone
  does not pulse TalkEnd. Tests also retain the separate motion-gated pending
  exit branch and preexisting disabled-controller state.
- **Viewer adapter assumption:** the successfully constructed idle body
  with the current canonical lifetime, outside cutscene/menu/talk, is
  eligible for this supported opening interaction. Full original cUnit
  status, character flags, talk animation and automatic Guard activation are
  not simulated. The demo explicitly interacts with NPC1 after ev15, like
  the ordinary manual interaction path. This is not an original-game
  comparison or a claim that all Guard actors are faithfully implemented.
- Final core checks: `cargo test --release -p ddhost -p ddnpcrt` passed
  **56 host + 28 NPC tests**, no ignored tests, using the shared core target.
  The installed `story_guard` regression runs the real ev15 boot script and
  actual NPC1 placement with two seats; rejects unavailable body, radius
  equality, second-seat takeover and stale lifetime; keeps GSF381 clear on
  text End; then the actor frame produces GSF381 and quest2 through the
  installed scripts. No TalkEnd, Goto completion or enemy death fixture is
  supplied. Final focused checks after `f509db7` passed the installed host
  regression (4.43s) plus all 10 Guard tests (1.44s), including first-seat
  departure, second-seat retry, null/occupied shared-slot rejection and the
  active-dialogue script stop.
- Viewer checks: after narrow cleaning the changed crates in the shared M5
  target, **59 tests passed, 2 existing ignored**, 7.92s; release build passed
  in 37.21s, with two existing warnings. A private copy of that binary is
  `E:/ddda-re/notes/steps/s23/ddview.exe` (`bfedf90`). Final `f509db7`
  release build also passed in 34.95s; its private copy is
  `ddview-final-f509db7.exe` in that directory. The final lifecycle changes
  received focused core tests and a release build, not a second GUI replay.
- The ordinary 1920x1080, two-player `--perf-route --unattended` run has a
  600-second watchdog and writes `route-600s.err.txt` / `route-600s.out.txt`
  under that same private directory. It **completed with exit0**, PID31568,
  logged interval **263.24s** (02:33:38.264 to 02:38:01.509 UTC): ev15 ended;
  NPC1 conversation1 displayed its seven authored lines; text ended at
  02:37:24.846; the next actor/story frames produced GSF381 and quest2 and
  advanced the demo to Gate. No flag, scene, actor arrival or event completion
  was forced. The original game was not accessed or modified.
- Actual gate/field evidence: stage100 entered at
  `(41337.184, 31001.443, 143614.31)`, cell `(64,54)`; ordinary movement
  crossed into `(64,53)` at `(39998.6, 31228.379, 143007.31)` and settled for
  30s. Final position `(39998.914, 31229.26, 143021.88)`, **SceNo1410**,
  GSF372/381 true, q0001 words `[0x18000000,0,3]`, q0002 booted. The field
  admitted NPC304 (group4 record0) and spawned six goblins. This also makes
  the S13 field-admission route reachable without its isolated fixture.
- The next **observed** missing callback is NPC304's
  `ai\fsm\npc\st100\quest\q0002_n304`: `Set Invincible`. The benchmark
  deliberately stops after the field settle, so rescue combat, its camera
  handshake and quest clear are not accepted. S22 independently identified
  NPC304 as Reynard and traced the next callback to `0x007aa040` (vf180 or
  intel+0x5c0, mask0x10000000); that is a next brief, not an S23 change.
- SceNo clarification: the authored q0002 boot keeps **1400**. Its quest
  cleanup reaches **1450** only after the party leaves stage100, as covered
  by the existing installed quest regression. Field admission is therefore
  expected to occur while SceNo is still1400; the field route then actually
  advanced to1410 through its installed scripts.

Next: translate the NPC304 Invincible callback and remaining encounter
prerequisites, then extend the authored q0002 route through its rescue/exit.
Replace the explicit body-eligibility assumption with original actor status
and talk-motion observations before claiming general Guard fidelity.

### NPC304 continuation (head message 3883)

- Current implementation `1deb4cc`, after task-only integration of main
  `cb300d8`. `Set Invincible` translates `0x007aa040`: Bool toggles
  mask0x10000000 in the validated body's vf180 word, or intel+0x5c0 when
  no linked body exists. The existing canonical ActorFlag path now accepts
  this process and preserves all unrelated bits; no immunity/damage
  consumer is claimed for an NPC combat body that does not yet exist.
- The next immediate callback was `SetGaugeDisp`. Registration
  `0x005342c0` binds **state** callback `0x00534bc0`, with no entry callback.
  It writes intel+0x547; linked-body helpers `0x00bb8c10` / `0x00bb8ca0`
  update the same byte and create/remove `uGUINpcGauge`. The headless port
  emits a typed Gauge request on state polling and stores its desired
  boolean per canonical intel lifetime. Constructor `0x00bbbfa0` clears
  that byte; unload clears the matching port owner. Actual NPC gauge
  allocation/drawing and HP/death facts remain separate, unimplemented
  consumers. Merely accepting this callback does not draw a health bar.
- Installed Reynard regression uses actual stage100 NPC304 group4 record0
  with authored `q0002_n304`. The graph remains active in its trembling
  state, sets Invincible/NoDamage/FixPos, and requests its gauge. Explicit
  **test fixture** observations of q0002 bits25/20 then execute authored
  protection clear and gauge-off transitions. Unrelated bits survive; the
  nonloop motion remains pending without an actor completion. No production
  quest bit, HP, enemy death, motion end or Goto arrival is manufactured.
- Full required checks on this integrated source: **57 host + 28 NPC tests
  passed**, none ignored; both installed Guard/Reynard tests pass. Viewer
  **61 passed, 2 existing ignored**, 12.65s (29.73s compile); release build
  passed in **41.90s** with the same two existing warnings. Private binary
  `E:/ddda-re/notes/steps/s23/ddview-npc304-1deb4cc.exe`.
- Final two-player ordinary replay used the same 600-second watchdog,
  private logs `npc304-route-600s.err.txt` / `.out.txt`, PID32560. It reached
  stage100, admitted NPC304, and emitted **no Invincible/SetGaugeDisp
  quarantine**. It then stopped at a new **application failure**, not a
  completed benchmark: `NPC materialization: NPC MultiNpc materialization
  not implemented` at02:53:23.748 UTC. Immediately before this, streaming
  unexpectedly added origin-near cells49/50 to the arrival cells63..65.
  That streaming/admission cause is unresolved; no MultiNpc placement was
  silently skipped to finish the benchmark. Logged interval233.16s.
- The OS process reported exit0 despite the failure. `main.rs` calls
  `app.run()` and discards its returned AppExit (line659); the log therefore
  matters to acceptance. The prior complete route remains specifically
  evidence for the earlier `bfedf90` binary. The integrated `1deb4cc`
  replay passed the Guard/gate/NPC304 callback boundary but did not reach
  field-cell walk/settle or rescue completion. No original game access.
- Next source leads, not implemented here: `Set No Talk` callback
  `0x007aa670` toggles vf180/intel mask0x00800000; `0x00bb4060` rejects that
  bit during talk promotion. NPC motion requests currently retain only an
  interface class name, losing authored motion3226 parameters; the actual
  interface `0x00576e10` and parameter copy `0x00576ef0` are the starting
  points for a body-motion adapter and genuine motion-end observations.

Next: head approved [S32](S32-npc-authored-motions.md) in board3999 for
Reynard's authored motion pipeline. Separately investigate the unexpected
origin-cell streaming and actual MultiNpc admission, and propagate AppExit
to the process exit code so failed unattended routes cannot appear green.
