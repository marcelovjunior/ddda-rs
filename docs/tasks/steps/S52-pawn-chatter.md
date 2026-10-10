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

### Follow-up source preparation (not compiled or tested)

The next source batch adds host admission bindings, `FrameReport.pawn_messages`,
reliable `ToClient::PawnMessage` with persistent-profile hearing and monotonic
message IDs, and a thin installed-font/STRQ consumer. It preserves actual
hirer, network identity and hire time; rebinding retires old requests. The
host's successful-load tail clears transient speech after fallible parsing;
failed load must preserve the queue. A new installed regression checks the
pass-C order mailbox only produces speech on the following move frame and
checks failed-load atomicity. All follow-up tests remain **unexecuted**.

New native evidence: `0x00AFCFBF` and `0x00AFD17F` request situation 6 on
actor list **1**, not configuration priority 1. `0x00AFCE65` initializes
the former caller's EDI to 1. `0x00599BB0` resolves situation records from
`0x0152D0F0`; `0x0059D0E0`/`0x0059D180` select four-candidate message groups,
skipping individual `-1` holes. These are runtime executable readers.
`0x005AAF7F..0x005AB09E` forwards every eligible list1 request in insertion
order, without ordinary TALK cooldown or random selection. The host exposes
`SituationEvent { pawn_slot, situation, list_index }`; S53 supplies the
actual guarded near-death entry event. No guessed low-HP threshold is used.

Combat timing remains open. Global `0x018FA4F4` is `sRecognition`, proved by
DTI registration `0x0132C820` and constructor `0x00487890`. Combat-list writer
`0x004403A0` reads its recognized-byte records at `0x00440554`, then pawn
sensor2 candidates at `0x00440628`. Recognition `0x00487B00`/`0x00488260`
uses seen/lost clocks; visibility `0x00488440` tests the player camera's
frustum (`0x007D3F50`) and a collision ray. Existing placeholder pawn
`BATTLE_RANGE` and current attack targets are not substituted for this list.
The native sensor/recognition adapter and original comparison are outstanding.

Presentation currently covers plain single-line glyphs with native ordinary
message/name font objects 5/9 and settled row positions. Native multiline,
markup, portraits, filters, slide/fade sequences, GUI acceptance/history,
voice pitch/spatial/lip synchronization and immediate NPC messages remain
explicitly unsupported. The installed GUI/text/voice regression is also
unexecuted. Source preparation alone is not a Ready result.
Viewer reconnection/session replacement also remains open: the current
viewer creates one session at startup. A future replacement must reset
presentation deduplication to that new session, while same-host load/retry
must preserve monotonic IDs. `Client::new` starts a fresh client counter.

Opening hooks for head to apply, because `opening.rs` remains head-owned:

```rust
// prepare(), after Host::new and before host/session construction completes:
host.pawn_chatter = ddhost::pawn_chatter::Runtime::with_data(std::sync::Arc::new(
    ddhost::pawn_chatter::Data::load(&vfs)?));
// step(), after the successful host.tick_network result:
crate::pawn_chatter::queue(&mut stage_view.commands, &report.pawn_messages);
// guest_step(), in the existing drain of client.events, before PlayEvent:
if let ddnet::proto::ToClient::PawnMessage(message) = &event {
    crate::pawn_chatter::queue(&mut view.commands, std::slice::from_ref(message));
}
```

Next actual gate: integrate reviewed committed main with the S53 transactional
loader and head's capsule-swept push-out fix, then obtain the normal full
touched/downstream Cargo window and viewer release window. No follow-up
Cargo run, GPU work, original process control or capture has occurred.
