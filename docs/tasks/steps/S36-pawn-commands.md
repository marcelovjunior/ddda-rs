# S36: Pawn commands (Go! / Wait! / Help! / Come!)

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
