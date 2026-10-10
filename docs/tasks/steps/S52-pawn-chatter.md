# S52: Pawn chatter (Rook talks: CMC messages and voices)

- **Branch:** `step/s52-pawn-chatter`
- **Files:** `crates/ddpawnai` / `crates/ddenemyrt` (when a pawn speaks),
  `crates/ddview` (uGUICmcMessage, voice playback), tests
- **Kind:** research + one feature

## Context

Pawns talk constantly in the original: on hiring, in fights ("Goblins!"),
on orders (S36's Go/Come/Help replies), when finding items or noticing
quests. Rook says nothing today. Known anchors: `uGUICmcMessage`
("rom\gui\GUIuGUICmcMessage"), `cCMCMessageResSet`, `mCMCMessageFetchMinimum`
/`Width`, voice banks `rom\voice\voice_cmc_{m,f}0{1,2}[_u|_d|_u2|_u3|_d2]`
(0x01484870..), `mLogPawnHelp`; S36 left order replies as typed requests.

## Do

1. Trace the chatter pipeline: event kinds (situations) -> message ids ->
   voice cue per pawn voice type, rate limits/priorities, who hears it in
   co-op, and the on-screen message (uGUICmcMessage layout).
2. Implement it for the opening's situations (hire, combat start, enemy
   kinds, S36 orders, low HP) with Rook's voice and the installed text.
3. Tests for selection and rate limiting; one capture with a line on screen.

## Done when

Rook speaks the original's lines in the opening's fight and on orders, with
text and voice from the install, sources documented.
