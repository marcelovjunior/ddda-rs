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

## Results: core checkpoint, S52 still in progress

Implemented and tested headless CMC readers/selectors in `ddpawnai::chatter`
and the authored story `Request::PawnMessage`. This checkpoint does **not**
complete S52: host composition, installed GUI/audio consumption, opening
combat-list/low-HP producers and the bounded capture remain outstanding.
The separate `ddhost/pawn_chatter.rs` adapter is source preparation and has
not been compiled or tested at this checkpoint.

Native sources (DDDA.exe 2017-12-13 Steam):

- `0x00CB4430`/`0x00CB45F0`: CMC version 110813, 16-byte configuration rows,
  signed priority, expiry and immediate-path flag. `0x00413C80` supplies
  fallback priority 1, expiry 60 and the ordinary/immediate dispatch.
- `0x005CFC20`: order phrase lists by saved `mUseSceneTalk[0]` and native
  goal classifications. Read lists and jump tables from the installed exe;
  no phrase IDs copied. `0x00599780` consumes the changed-command latch:
  refreshing the current order or its expiry does not produce a reply.
- `0x00599C50`, `0x008BC5F0`/`0x008BC630`: enemy notice records and first
  matching knowledge-gated phrase row. Ordinary notices compare the variant
  only for unit kind `0x8E`; defeated notices compare it for every kind.
  Study bits are LSB first, below `0x2840`. Lists terminate only at `-1`;
  other signed IDs are retained and their bank meaning remains unresolved.
- `0x005C6130`/`0x005C62A0`: authored `mMesId:u32` at +4 and
  `mCmcId:s32` at +8. A negative slot selects the first living pawn before
  checking CMC readiness, without falling through to a later ready pawn.
  Party pawn slots are separate from human seats, including reserved slots.
- `0x005A01B0`: two integer draws shuffle three slots before eligibility
  selection. `0x005AADC0` clears the actor's ordinary request list every
  eligible phase, including during cooldown; selection requires strictly
  negative time after subtraction. `0x0057EFF0`/`0x005AAF2C` select cooldown
  1800/360/210/150/60 from TALK thresholds 200/400/600/800.
- `0x00455C30`: stable descending priority, global message-ID duplicate
  refresh, grouped replacement by pawn slot in place. Reaching 32 requests
  discards the first node, retaining 31. `0x004557B0` expires requests before
  fetching. `0x00454F80` initializes the fetch minimum/width to 20/50;
  `0x00422FA0` converts the native RNG word by `2^-32` with x87 arithmetic.
- `0x00449930`/`0x00457A50`: text/stream bank resolution, saved voice
  fallback and gender/voice bank suffix. Bulk bank mappings remain in the
  player's executable. Text uses the decimal message label, not an index.
  Motion `voice_cmc_*` banks are separate from these ordinary STRQ streams.
- `0x00632D20`/`0x00632FF0`: actual installed GUI is
  `id/cmc_mes/cmc_mes0`, three cloned entries, five-second hold. Native
  decimal-label binding and font measurement are `0x00633260`/`0x006340F0`;
  layout, font sizes and textures will be read from the install.

Actual validation on 2026-10-10: full
`cargo test -p ddpawnai -p ddstory` exited **0**, 67 tests passed across
11 nonempty suites and two successful empty doc-test suites. Ten new
chatter regressions and the new typed-story regression were present and
passed. The first attempt exited 101 because the reader wrongly rejected
native signed enemy IDs other than `-1`; the native loop justified removing
that rejection, and the full rerun passed. Private log:
`E:/ddda-re/notes/steps/S52/core-second.private.log`. Source SHA-256:
chatter `C248B4E0E7983374412430E03DC54220841ED990C5E8C46847305F325DE5B12D`,
orders `86CB7694D08C457EEE22219874EFA776FC037B57232B62994DAEA6E1D208F2E6`.

No original-game comparison or GUI/audio capture yet. Ghidra stayed down;
research used the private analysis exe with Capstone and installed-resource
readers. No computer use, game control, save writes, installation writes or
private assets were committed. Next action: compose the admitted-pawn host
adapter with the typed authored request, then installed native GUI/voice
consumption; run full downstream tests and a release viewer build before
requesting the single nearest-checkpoint capture from the GPU holder.
