# opening-movie: Movie playback (st100ev00.wmv)

- **Owner:** head
- **Branch:** `task/opening-movie`
- **Board area:** `story/movie`
- **Status:** partial

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Play the game's WMV movies from the install, starting with `movie\event\st100ev00.wmv` (the beach and the dragon) at q0000. Decode them at run time (Windows Media Foundation through the `windows` crate is the likely route; no transcoded copies on disk), with audio, a skip key if the original has one (trace it), and the subtitle source if there is one.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddview/src/movie.rs`
- `crates/ddstory/**` (request only)
- `docs/EVENTS.md` (movie section)
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

The movie plays fullscreen at its native size and rate, with sound, in `--opening` at the traced point, and returns control to the quest script. Fill in Results and report to `head`.

## Results

head, 2026-10-09:
- **`mtmovie`** decodes the install's WMVs with Media Foundation's
  Source Reader at run time: video as top-down BGRA (`MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`
  to RGB32, stride-aware), audio as `f32` PCM. `st100ev00.wmv` is
  1280x720, 29.97 fps, 109.6 s, 6-channel 48 kHz; installed test
  `the_opening_movie_decodes_video_and_audio`.
- **`ddview` `movie.rs`** plays it full screen:
  - video is decoded ahead on a worker;
  - the soundtrack is decoded whole and folded to stereo from MF's
    FL FR FC LFE BL BR order;
  - the clock starts with the sound;
  - `MovieFinished` is set at the end.
- **Trying it:** `ddview <nativePC> --movie st100ev00` (also `st230ev00`,
  `st240ev05`, `st610ev21`, `title_A`, `title_B`). Checked: the frame at
  20 s shows the epigraph.

Open:
- the original's skip input (Enter and Esc are a viewer choice);
- subtitles, if the movie has any outside its picture;
- the hookup to q0000's event st100 ev00 in `--opening` (after the
  prologue, stage 800).

### S08: original movie skip input (codex-1, 2026-10-09)

**Answer:** the event movie accepts a newly pressed **Start** button from
any of four pads, or keyboard **Escape**. There is **no seen-before gate**
in the event playback/skip path; first playback is skippable once the
event enters its playing state. Enter is not the keyboard skip binding.
This is executable tracing plus a read-only binding snapshot, not a live
first-playback comparison.

- `sEventManager` play handler `0x00435fb0` calls `0x004367c0` every update
  before testing timeline completion; success switches to state 4 (skip).
  Movie preparation `0x00435d90` finds `uMovie` named `Movie`, starts it
  through `0x00f7a720`, and enters that same state 3. The movie is not a
  separate input-policy path.
- Pad test in `0x004367c0`: for slots 0..3, at
  `sQlocInput + 0x1774 + slot * 0x2e8`, test `pad[+0x14] & pad[+0x220]`.
  Reflection `0x0040bf00` identifies `+0x14` as `mBtn.trg` (press edge).
  Constructor `0x0040a3d0` sets `+0x220` to **0x00000008**. This is the
  engine's internal mask, not an XInput mask. The binding array starts at
  `+0x218`; index 2 is `ID_START`, named by `0x0040a6c0` (string
  `0x01558ffc`). The constructor's later configurable action-table copy
  does not overwrite index 2. `sQlocInput + 0x232d != 0` suppresses the pad
  path; that is an input suppression check, not movie history.
- If no pad triggered, the disassembly calls `0x0040d200` with EAX=1,
  ECX=`sQlocInput`, and stack argument 0. These are binding **ID_CANCEL**
  (index 1) and trigger mode 0. The helper reads its key vector at
  `sQlocInput + 0x12fc + 1 * 0x14`, tests keyboard bits at
  `sKeyboard + 0x160`, or the corresponding mouse-trigger mask.
- Default-key builder `0x0040ab20`, instructions `0x0040ab33..0x0040ab75`,
  initializes that index with scan code **0x01** through `0x0040b460`
  (`MapVirtualKeyA(scan, 1)`), yielding **VK_ESCAPE = 0x1b**. It sets the
  binding's editable flag to 0; `0x00408600` only loads configuration for
  editable bindings. Thus Enter is not an alternate default here.
- After a successful input, `0x004367c0` optionally sets sGameSys
  `+0xac6f0` bit `0x01000000` when `0x04000000` is already set. This occurs
  after acceptance and never rejects the skip; it is not a seen gate.

Validation: decompile and disassembly agree on the input offsets, masks,
register arguments and state transition. A read-only oracle snapshot of
the paused running game found all four skip masks equal to `0x8`, the
index-1 key vector containing only `0x1b`, and input suppression byte 0.
No game state was changed; no capture or game assets were added. No Rust
changed, so crate tests/builds are not applicable to this research step.

Next: S16 should use Escape and each connected pad's Start press edge,
allow the first playback to skip, and hook movie completion/skip into the
existing event lifecycle. Fade/cleanup `0x00436070`/`0x004361a0` remain
separate from the input findings; do not claim their presentation timing
has been matched by this step.
