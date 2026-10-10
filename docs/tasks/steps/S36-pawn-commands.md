# S36: Pawn commands (Go! / Come! / Help!)

- **Branch:** `step/s36-pawn-commands`
- **Files:** `crates/ddsim` (the four order inputs), `crates/ddenemyrt`
  (pawn.rs / pawn_actor.rs: the order's effect), `crates/ddhost`, `crates/ddnet`
  if the input crosses the wire, `crates/ddview` (key bindings); tests;
  a section in `docs/PAWNS.md`
- **Kind:** research + one feature

## Context

The d-pad orders are a core part of the pawn system and of the tutorial
(Rook, the first pawn, joins in Cassardis). Nothing in the project models
them yet: hired pawns only follow the Arisen and fight on their own
(`crates/ddenemyrt/src/pawn.rs`).

## Known (head, 2026-10-10)

- The key/pad bindings are named `STG_ORDER_CMC_UP/DOWN/LEFT/RIGHT`
  (0x015591CC..0x01559208, registered in the key-config table built by
  0x0040A6C0; config.ini on PC). CMC = the pawns (the game's internal
  name, as in `Player Cmc Info`, `mCmcStock`).
- Spoken lines: `rom\voice\voice_cmc_{m,f}0{1,2}[_u|_d|_u2|_u3|_d2]`
  (0x01484870..), and `uGUICmcMessage` ("rom\gui\GUIuGUICmcMessage") shows
  pawn chatter text; `cCMCMessageResSet`, `mCMCMessageFetchMinimum/Width`.

## Head research (2026-10-10)

- Bindings: `STG_ORDER_CMC_UP/DOWN/LEFT/RIGHT` are entries 37..40
  (0-based) of the key table built by 0x0040A6C0 (after `STG_MGC_CH_MODE`,
  before `STG_TOGGLE_BOW_MODE`).
- The Arisen's side: `cPlActCMCOrder` (vtable 0x015E2570, DTI 0x019A16A0;
  vf05 0x00ACECF0, vf06 0x00ACF0F0): a shout action; lower motion from
  {0xF0, 0xF1, 0xF2}[index +4], upper {0xF4, 0xF5, 0xF6}, and other motion
  pairs (0x603/0x604, 0x615/0x616, 0x617/0x618, 0x654/0x655, 0x54/0x55)
  chosen by weapon type (uPlayer +0x3554/+0x3558 high bytes) and stance
  flags; it turns the Arisen (0x007975D0) and ends on its motion.
- The pawn side is already in docs/PAWN_AI.md "Commands": 8 none,
  9 Wait, 10 Go, 11 Help; `uPlayerBase::vf98` (0x00B79FC0) message cases
  9/10/11 write the mailbox uCmc +0x4ABC (= `param_1[0x12AF]`) only for
  pawn units (+0x206D & 6); 0x005927F0 moves it to pending +0x53C, applies
  after the delay +0x540, expires after +0x530 (~20 s). `ddpawnai::pawn::
  Pawn::set_command` already rebuilds the priority list (0x005AC240).
- Open: which key sends which message (find the sender of vf98 messages
  9/10/11 and what "Come!" sends: message 0xC/0x30 call vf120, or
  something else), and who receives it (the Arisen's own pawns only?).

## Do

1. Trace the input bit each binding sets on the player, where the order
   is stored (per Arisen, per pawn?), and how each pawn's AI reacts
   (Go: attack/advance; Wait: hold position; Help: regroup to the Arisen;
   Come: follow closely). Check the timing (how long an order lasts, what
   clears it) and anything the Arisen plays (a shout motion, a voice).
2. Model the four orders as ordinary per-seat sim input; a seat's orders
   reach only the pawns it owns (co-op: each Arisen commands its own
   party; follow the source on who obeys whom).
3. Pawn behaviour per order in the existing pawn actor, as far as the
   source shows; leave the pawn's spoken reply as a typed request if the
   chatter system is not there yet.
4. Viewer: bind the keys (arrow keys or the original PC defaults from the
   key-config table) and the pad d-pad.
5. Tests: Wait holds Rook in place while the Arisen walks off, Come
   brings him back, Go sends him at a goblin, Help regroups.

## Done when

Installed tests show each order changing Rook's behaviour as the source
says, the viewer's keys send them, and PAWNS.md documents the orders with
addresses.

**Correction (codex-64, #5496/#5511):** the native sender 0x00B768E0 knows three
orders, not four: UP (key 39) -> message 9 Go!, DOWN (40) -> 10 Come!,
LEFT/RIGHT (41/42) -> 11 Help! (installed `pawnorder_go/come/help` texts),
sent to all three pawn slots (sPlayerManager +0x9A0/+0x9A4/+0x9A8) without
owner filter. The 9 = Wait reading in PAWN_AI.md was wrong; there is no Wait.

## Results (codex-64, 2026-10-10 source checkpoint)

**Implemented:** Go 9 / Come 10 / Help 11 from sender 0x00B768E0,
per-seat held directions and rising-edge arbitration, pass-C mailbox
delivery, native delay/expiry/feedback boundaries (0x005927F0), priority
rebuilds, main-frame repeat cooldown (0x00B6D510, adjust ID 0x135), and
typed shout-index/reply candidates. Keyboard F1/F2/F3 and all d-pad
directions enter existing standalone/host/guest input paths. Protocol 8
adds one four-bit direction byte; reserved bits are rejected.

**Co-op decision, head #5446/#5548:** either Arisen can order the shared
main pawn; hired/support pawns follow their owner. Ascending host seat
order makes the highest valid simultaneous seat win a shared mailbox.
This is project policy, distinct from the native single-Arisen broadcast.

**Tested checkpoints:** five scalar clock/message regressions; an
installed Rook recipient/next-frame-priority regression; an installed
Go/Come/Help priority and battle-follow-distance regression; two wire
codec regressions. Viewer release builds exited 0: the `order_` filter
passed two tests (one order-specific, one existing skill test), compile
28.06 s; controller isolation passed one test, compile 29.38 s; both test
runs reported 0.00 s. Final shared-adapter core rerun exited 0: two
order-edge/cooldown unit tests (5.22 s compile, 0.00 s test), installed
Rook host test (5.33 s compile, 6.10 s test), and all seventeen ddnet
tests (6.30 s compile; suites 0.00 / 5.55 / 0.22 / 6.82 / 5.67 s).
An initial shared-target attempt exited 101 because the cached ddsim
artifact lacked the source's new input fields; package-only
`cargo clean -p ddsim` removed 15.7 MiB and resolved it. No full target
cleanup or new target was used. Both build leases were released after
observing actual process exits.

**Open:** full Help support-goal execution, ordinary noncombat formation
geometry, native GOAP stop/pre-emption, per-unit actor time-rate scaling,
weapon/stance shout animation, chatter cue selection, and an original
game comparison. The Rook test proves ownership/delivery and installed
priority rebuilding, not the unfinished end-to-end acceptance behaviours
listed above. Whole S36 remains open.

**Next action:** submit this checkpoint for independent review, integrate
reviewed committed main, then trace the native support/formation actors
(0x00589ED0 / 0x00597060) before adding their behaviours.
