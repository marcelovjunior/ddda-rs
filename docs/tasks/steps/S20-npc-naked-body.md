# S20: A Cassardis NPC drawn as a bare base body

- **Branch:** `step/s20-npc-body`
- **Files:** most likely `crates/ddview/src/opening.rs` (NPC
  materialization) and `crates/ddlook` (NPC looks)
- **Kind:** diagnose and fix one visible defect

## The defect

In the opening, after ev10 (demo shot `4-control`, stage 200 SceNo 1200,
the class-selection room), a bald, naked male stands next to the Arisen:
an NPC drawn with only the base body `m000`, no hair, no clothes. It
appears with or without head's weapon-swap change, so it predates it; the
NPC materialization merges (upon-a-pawn, S13) are the likely source.
Reproduce: `ddview <nativePC> --opening --skip-intro --demo --unattended
--shots <dir>` (140 s timeout), see `4-control.png`.

## Do

Identify which NPC it is (id, record, layout), why its look has no parts
(missing look record, wrong NPC id to look mapping, a look built before
its data loaded, a stand-in body), and fix the shared NPC look path. If
the original doesn't show this NPC here at all, fix its admission instead.

## Done when

The demo's `4-control.png` shows no naked NPC (or the NPC correctly
dressed), ddview tests pass, and the Result names the NPC and the cause.

## Result (codex-51)

The bare actor is **Arisen seat 1**, not an NPC. It has no NPC id, layout
record, or NPC edit path. The ordinary command defaults to two players
(`Args.players = 2` in `main.rs`), but `opening_world` supplied only
`vec![arisen]`. `opening::setup` spawned both seats; seat 1 received
`None`, which `play::setup` passed to `character::spawn` with `play::BODY`
(`model\\pl\\m\\m_base\\m000\\m000`). Thus its bald, bare body was the
explicit missing-appearance fallback. Neither NPC admission nor
`ddlook::npc_look` selected this body.

Implementation **3b4601b** fills missing occupied-seat appearances with
the existing `ddlook::save::template_look`, which reads the first built-in
pawn of the requested sex from installed `etc\\editPattern\\romPawnInfo`
and its equipment. This is the existing **viewer choice**, not a claim
about original starting equipment. Supplied editor/save appearances are
preserved. Local startup now enumerates the actual Arisen roster: a
single-player session no longer creates a phantom second body by copying
seat 0's pose. LAN keeps its explicit capacity reservation because its
snapshot renderer expects those entities to exist before peers join.
The small related `main.rs` change passes this network policy to setup.
No NPC is hidden, no NPC gate changes, and no outfit is invented or
copied into the repository.

Validation:

- `cargo test --release -p ddview --target-dir
  E:/ddda-rs-wt/m5-cassardis/target`: **58 passed, 2 existing ignored**,
  12.14 s. The roster regression covers one/two Arisen, a separate pawn,
  a remaining nonzero seat, and LAN reservations. The installed appearance
  regression requires second-seat hair and outfit parts while preserving
  a supplied female first-seat appearance.
- `cargo build --release -p ddview --target-dir
  E:/ddda-rs-wt/m5-cassardis/target`: **passed**, 48.40 s. Existing
  `opening_world` visibility and audio `forget_actor` warnings remain.
- `git diff --check`: passed.
- The exact ordinary demo command from the brief, with the default two
  players, produced `demo/4-control.png` at stage 200 SceNo 1200.
  Inspected visually: **both Arisen have hair and complete clothing**;
  the bare body is gone. The startup log records seats `[0, 1]` and 31
  NPCs drawn. Both Arisen retained their looks when the starting weapon
  kit replaced their view bodies.
- The 140 s watchdog stopped the owned viewer after 140.05 s (harness
  exit 124). All five captures through `5-talk-quina.png` completed.
  The subsequent demo was navigating `Q0002Event` at SceNo 1300;
  this step does not claim the later q0002 endpoint passed.
- Private logs/executable: `E:/ddda-re/notes/steps/s20/`.

The S20 visual gate is passed. Original-render equivalence and actual
LAN joining were not compared by this step; the regression preserves the
existing LAN reservation policy. Next action: review/merge this startup
fix, then test distinct edited/save appearances in a two-player opening.
