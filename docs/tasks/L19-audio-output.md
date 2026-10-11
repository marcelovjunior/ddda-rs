# L19: audible audio (the XAudio2 2.7 mixer)

- **Owner:** claude-4
- **Branch:** `task/L19-audio-output` (worktree `E:\ddda-rs-wt\L19`)
- **Status:** review

## Why

L16 (`crates/ddruntime/src/audio.rs`, results in
[L16](L16-input-audio-services.md#results)) gives the game a complete
XAudio2 2.7 ABI: engine, mastering/submix/source voices, buffers, callbacks
and a clocked graph. It is silent: buffers are consumed in real time, but
no samples are mixed. The game itself decodes its Vorbis streams and submits
PCM; bank sounds are submitted as **MS ADPCM** (128 mono voices at 48 kHz,
70-byte blocks of 128 samples, `0x00DCE1F0`) or PCM (`0x00DCE2E0`). So the
rebuild needs what `xaudio2_7.dll` does between SubmitSourceBuffer and the
speakers, nothing more: the game's own sound code stays the lifted original.

## Task

1. **A mixer crate, `crates/ddaudio`**, independent of `ddnative` and
   `ddruntime` (so it builds and tests in seconds), that renders the XAudio2
   graph in quanta (XAudio2 processes 10 ms passes, 480 frames at 48 kHz):
   - source formats the game uses: PCM 8/16-bit, IEEE float, MS ADPCM
     (`WAVEFORMATEX` + `ADPCMWAVEFORMAT` coefficient tables, block
     alignment, the play/loop ranges of `XAUDIO2_BUFFER` in samples);
     xWMA only if the census in step 3 shows the game uses it;
   - sample-rate conversion by source rate × frequency ratio to the
     destination's rate, continuous across buffers and loops;
   - voice volume, channel volumes, output matrices per send (the game
     supplies 1-to-6 matrices), submix stages in processing order, the
     mastering voice's channel count and rate;
   - voice filters (XAudio2's state-variable filter, `XAUDIO2_FILTER_PARAMETERS`
     with the documented frequency/one-over-Q mapping) and the AudioReverb
     effect: implement reverb last, and only after the rest is audible;
   - the timing L16 already models (buffer start/end, loop end, stream end
     callbacks at the sample they occur) must stay exactly as it is: the
     mixer consumes, L16's graph reports.
2. **Wire it into L16's graph** (`audio.rs`): where the silent graph
   advances a voice, the mixer now reads the guest buffer bytes and
   produces samples into L16's bounded output channel. Keep the change in
   `audio.rs` small; all DSP lives in `ddaudio`.
3. **Census first** (into Results): every source format, channel count and
   rate the game creates voices with, every send/matrix/filter/effect use
   (Ghidra over HTTP, addresses as evidence), so nothing is implemented the
   game never uses and nothing it uses is missed.
4. **A host sink in `ddplay`**: the output channel to the default audio
   device (WASAPI shared mode through `cpal`, or the `windows` crate),
   with the device's mix format converted at the edge. Headless runs and
   tests keep the channel without a device.
5. **Checks against the real thing**, like `d3d9ref` for graphics: an
   oracle that renders the same graph on the host's XAudio2 (2.9 ships with
   Windows; capture the mastering output with a pass-through XAPO on the
   mastering voice, or route to a submix with a capturing XAPO) and compares
   samples:
   - ADPCM and PCM decode with ratio 1 and unit volume: exact;
   - matrices and volumes: exact up to float rounding;
   - resampling and filters: XAudio2's resampler is proprietary, so compare
     within a stated tolerance (signal-to-error ratio) and record it.
   Record where 2.9 and 2.7 may differ and say what was compared.

## Rules

- Read first: `AGENTS.md`, `docs/RUNTIME.md` (implementation status and
  the graphics backend design, which this mirrors), L16's brief and
  results, `crates/ddruntime/src/audio.rs`.
- Never invent behaviour: every rule comes from the SDK headers
  (`xaudio2.h`, `xaudio2fx.h`, `mmreg.h` for ADPCM), Microsoft's docs or the
  oracle; cite the source in Results.
- The game's sound code is lifted, never replaced. No special cases by
  address.
- The game install is read-only; tests may read its files with
  `DDDA_EXE`/the install path; never commit game data.
- Inventory note from L16: the buffer-end callback `0x00DCCD00` was missing
  from the inventory; check whether the address-taken discovery rule (main
  066d970) now lifts it and report.
- Builds and memory (the machine runs five agents):
  - Always `CARGO_INCREMENTAL=0`, and put your target dir on C:
    (`CARGO_TARGET_DIR=C:/ddda-targets/L19`).
  - `ddaudio` builds use `CARGO_BUILD_JOBS=4`.
  - Anything that builds `ddruntime` (it pulls the huge `ddnative` crate)
    uses `CARGO_BUILD_JOBS=2`, and only when at least 8 GB of RAM is free
    (`(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory`).
  - Do the `ddaudio` work first, so most iterations never touch `ddnative`.
- Keep your prompt cache warm: never sit inside one tool call for more than
  a few minutes; run long jobs in the background with output to a file and
  poll it.
- Do not spawn subagents.
- Commits on your branch only, as
  `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  with no AI co-author trailer. Never push. Never commit game data, anything
  from `E:\ddda-re`, or decompiler output.
- Fill in Results, update the `docs/RUNTIME.md` row, and commit. End your
  run with a short summary for the head.

## Results

### Summary

- `crates/ddaudio` (no dependency on `ddnative`/`ddruntime`): decoders,
  SRC, the per-voice chain, submix stages, ramps, default matrices, guest
  effect hook and the reverb. Every DSP rule is compared against the real
  **XAudio2 2.7** (`XAudio2_7.dll`, the DirectX June 2010 redistributable's
  COM server, installed on this machine) and against 2.9: all bit-exact or
  within float rounding (table below).
- `audio.rs` (L16) feeds the mixer the frames its clock consumes and sends
  the mastering voice's mix down the existing output channel; timing and
  callbacks are unchanged. The game's own XAPOs run through IXAPO.
- `ddplay` plays the channel on the default device (`cpal`, WASAPI shared
  mode) with the mix format converted at the edge (`sink.rs`).

### Census (Ghidra over HTTP, capstone on the analysis exe)

Voices the game creates (`CreateSourceVoice` = engine slot 8, `+0x20`):

| Where | Format | Count, flags, sends, effects |
|---|---|---|
| `0x00DCE1F0` | MS ADPCM, mono, 48 kHz, block 70, 128 samples/block, the 7 standard coefficient pairs (`ADPCMWAVEFORMAT` on the stack) | 128 voices; flags 0, max ratio 2.0, no callback, default send (mastering), no effects |
| `0x00DCE2E0` (6 groups) | 16-bit PCM 48 kHz, 1, 2 and 6 channels | per group 6 voices: each channel count with flags 0 and with flags `0x10` (2.7's `XAUDIO2_VOICE_MUSIC`); max ratio 2.0; callback = the group's object at `+0x51544` (L16's vtable `0x0142AC9C`); default sends |
| `0x00DC7BA0` | as `0x00DCE2E0` (channels from `+0xA0`, flags 0/`0x10` by `+0x3C`) | re-creation of one streaming voice |
| `0x00DCE530` (from `0x00DD1C1E`) | PCM 48 kHz, channels `+0xA0` and bits `+0x6AC` of the caller's object (run-time) | one voice, flags 0, default send |

No float, xWMA or other source format is created; 8/24/32-bit PCM and
float are decoded anyway because the runtime accepts them and `0x00DCE530`
takes its depth at run time.

Submix voices (`CreateSubmixVoice` = slot 9; all 48 kHz, flags 0):

| Where | Voice | Channels, stage | Effect chain | Sends |
|---|---|---|---|---|
| `0x00DD58D0` | A `+0x4DD54` | 6, stage 4 | game XAPO object `+0x4DD78` (6 out) | default (mastering) |
| `0x00DD58D0` | B `+0x4DD50` | 6, stage 3 | game XAPO object `+0x4DDE4` (6 out) | `SetOutputVoices` -> A |
| `0x00DD58D0` | 4 x `+0x4DD00` | 6, stage 2 | game XAPO (vtable `0x0142AFE8`) at `+0x4E074 + 0x290 i` | -> B |
| `0x00DCE600` | reverb `+0x4DD58 + 4 i` | 1, stage 2 | AudioReverb 2.7 (`0x00DCE664`), 1 in / 1 out | -> B with a 1-to-6 `SetOutputMatrix` (`+0x4DC68 + 0x18 i`) |
| `0x00DCE600` | 4 x `+0x4DD10 + 0x10 i` | 1, stage 1 | game XAPO (`+0x4ED20 + 0xA40 i - 0x26C`), 1 out | -> reverb submix |
| `0x00DDB240` | 3 x `+0x51490` | 1, stage 0 | game XAPO class of ctor `0x00DD4B20` (vtable `0x0142B138`), 1 out | default |

Mastering: `CreateMasteringVoice(0, 0, 0, device 0, no effects)` in
`0x00DCE0E0`, so the device's channels and rate (the runtime's null
endpoint: stereo 48 kHz).

Routing and controls: sources send to a stage-2 submix or B plus an
optional reverb send (`0x00DC6680`, `0x00DC7A80`; `0x00DC7A80` can route
through a `0x00DDB240` submix). Volume is set only through
`SetOutputMatrix` (slot 16): `0x00DC7FAF`, `0x00DC8036` (to mastering),
`0x00DC8E59`, `0x00DCE6FE`, `0x00DD14E2` (reverb 1-to-6), `0x00DD157D`,
`0x00DD15C2`, `0x00DD161E`, `0x00DD1AFB`, `0x00DD1B37`, `0x00DD1B60`.
`SetFrequencyRatio` at `0x00DC6D8F` (pitch, max 2). `SetEffectParameters`
on the reverb submixes at `0x00DCEA17` and `0x00DD5B61` (52 bytes from
`0x00DC5C70`, the SDK's inline `ReverbConvertI3DL2ToNative`, so I3DL2
parameter sets).

Never called in the sound code `0x00DC0000..0x00DE0000` (every
register-indirect call by vtable offset classified): `SetVolume`,
`SetChannelVolumes`, `SetFilterParameters`, `SetOutputFilterParameters`,
`EnableEffect`/`DisableEffect`, `SetSourceSampleRate`; no voice uses
`XAUDIO2_VOICE_USEFILTER`, no send `XAUDIO2_SEND_USEFILTER`. So **voice and
send filters are not implemented** (the brief's SVF item), deliberately.
Volume and channel volumes are implemented (cheap, and part of the chain).

The game's own XAPOs (four classes, all with Process `0x00DCF4F0`, which
calls the class's DoProcess at vtable slot 17): vtables `0x0142AF48`
(abstract), `0x0142AFA0` (DoProcess `0x00DCCD30`: acts only when the
mastering voice has 2 channels), `0x0142AFE8` (`0x00DD5030`), `0x0142B138`
(`0x00DD4E70`). They are statically linked `CXAPOParametersBase` classes.
Their QueryInterface (`0x00DCF480`, base `0x00DCF350`) compare with the
**2.7 IIDs** at `0x0142AF28` (IXAPO `A90BC001-E897-E897-55E4-9E4700000000`)
and `0x0142AF38` (IXAPOParameters, `...01`), not the Windows SDK's 2.8+
ones; L16 used the 2.8+ IXAPOParameters IID (fixed in `audio.rs`).

Inventory (L16's question): main `066d970`'s address-taken rule does **not**
lift `0x00DCCD00`. It is reachable only through vtable `0x0142AC9C`, which
has no RTTI locator (the dword before it is `0x41A00000`), and the rule
scans code immediates only. The same gap leaves `0x00DCF3F0`
(LockForProcess), `0x00DCF3C0` (Calc*Frames), `0x00DCCD30` and `0x00DD5030`
unlifted (board #9121, #9166). Until they are, the game's XAPOs cannot be
locked: `audio.rs` reports each once and passes their audio through.

### What ddaudio does, and the source of each rule

| Rule | Source | Check (tests/oracle.rs, 2.7 and 2.9) |
|---|---|---|
| Per-voice order: decode, SRC, volume, effect chain, channel volumes, one matrix mix per send; submix stages in order, then mastering | "XAudio2 Voices", "XAudio2 Audio Graph" | `matrices_volumes_and_stages_match_the_host`: game-shaped graph, max error 1.2e-7 |
| PCM 8/16/24/32 and float scales (x / 2^(n-1)) | oracle | 16-bit `pcm16_unit_ratio_is_exact`: bit-exact |
| MS ADPCM block decode (header, coefficient sets, adaptation table) | `mmreg.h` layouts; Microsoft "Multimedia Standards Update" (1994) | `adpcm_unit_ratio_is_exact` (game format): bit-exact |
| ADPCM step size wraps at 32 bits on overflow | oracle (only `i32::wrapping_mul` matches) | `adpcm_step_overflow_matches_the_host`: bit-exact |
| SRC: linear interpolation, output one input frame behind; no SRC (plain copy, no delay) when source rate x ratio equals the destination rate | oracle; "XAudio2 Sample Rate Conversions" (SRC only when necessary) | `resampling_matches_the_host`, ratios 0.5..2: bit-exact on both versions |
| Default send matrices (1/2/6 channels, stereo mastering) | 2.7's `GetOutputMatrix` | `default_matrices_match_the_host`: values equal, render error 3e-8 |
| Volume/channel-volume/matrix changes ramp over 48 frames; 2.7 adds (new-old)/48 per frame from frame 1 and lands on the new value at frame 48 | oracle | `level_changes_ramp_like_the_host`: 2.2e-8 on 2.7 |
| Reverb: the platform's AudioReverb XAPO itself (2.7 by COM; else 2.9) | the effect is proprietary; hosting it is exact by construction | `reverb_matches_the_host` (impulse and noise; SDK defaults and an I3DL2-converted set): bit-exact; `reverb_dry_path_is_exact` |
| Guest XAPOs: QueryInterface(IXAPO 2.7), LockForProcess once with WAVEFORMATEXTENSIBLE float, masks 0x4 / 0x3 / 0x60F, MaxFrameCount 480, in-place Process with the input's buffer flag | `xapo.h`; the format and frame count XAudio2 passes, measured with the capture XAPO | `xaudio_runs_guest_xapos_and_skips_unlifted_ones` (runtime) |

Where 2.9 differs from 2.7 (measured, asserted by the oracle): the default
6-to-2 matrix (2.9: FL/FR 1, centre and back -3 dB = 0.70794576, no LFE;
2.7: a normalised downmix, front 0.29454544, centre 0.20818181, LFE
0.09090909 = 1/11, same-side back 0.25181818, other-side back 0.15454546),
the ramp arithmetic (2.9 interpolates; float bits differ), and the
AudioReverb's output (2.9's taps are a third of 2.7's level for the same
parameters). Decode, SRC, explicit matrices and volumes are identical.

Not compared: what either version does in the pass where a voice switches
between SRC and plain copy (ratio changing to or from exactly 1), submix
SRC (every game voice is 48 kHz), and channel layouts the game does not use
(the default matrix falls back to a diagonal there, labelled a guess).

### Integration (`audio.rs`, `ddplay`)

- Where the clock consumes `n` frames of a buffer, `feed` decodes exactly
  those frames from guest memory into the mixer before any buffer-end
  callback can hand the memory back to the game. Buffer timing, callbacks
  and their order are unchanged (L16's tests pass unmodified apart from the
  IID constant).
- After the voice loop, `render` snapshots the engine's voices (owned, so
  no runtime borrow is held while guest code runs), mixes the pass and
  sends the mastering block (`channels x rate/100` floats) down the
  existing bounded channel. The mixer runs every pass whether or not a
  host listens, as XAudio2 does (the game's XAPOs keep state).
- Small ABI fixes found on the way, all from the docs/oracle:
  `SetOutputVoices` resets the send matrices to the default
  ("SetOutputVoices" remarks); `GetOutputMatrix` without a prior
  `SetOutputMatrix` returns the default matrix instead of failing; the
  2.7 IXAPO/IXAPOParameters IIDs.
- Guest XAPOs: on first use per (voice, effect) `QueryInterface(IXAPO)` and
  `LockForProcess`; each pass `Process` (slot 10) in place on a copy of the
  buffer on the guest audio thread's stack, with the input's VALID/SILENT
  flag and `IsEnabled`; the output flag is honoured. A destroyed voice's
  XAPOs get `UnlockForProcess` and `Release`. A call that reaches unlifted
  code (an `Unbound` panic, caught) restores the CPU registers and stack,
  marks that effect pass-through and prints one line; any other panic is
  re-raised.
- `ddplay`: `sink.rs` owns the `cpal` stream on its own thread. The edge
  conversion: the two channels to the device's first two (front L/R),
  further channels silent, a mono device gets 0.5 (L + R) (2.7's own 2-to-1
  default); `ddaudio`'s resampler when the device rate is not 48 kHz;
  clipping to [-1, 1] (a mastering voice clips, "XAudio2 Voices"). Queue
  bound 100 ms (oldest dropped), silence on underrun. Without a device the
  blocks are drained. `main.rs` only gains one call after `Session::new`
  (L25 owns the rest of `main.rs`, board #9155/#9161).

### Verification

- `cargo test -p ddaudio`: 5 unit tests (formats, ADPCM header/nibbles,
  scales, SRC copy/interpolation/run-dry).
- `cargo test -p ddaudio --test oracle -- --ignored --test-threads 1`
  (needs an audio device; about 15 s): 9 tests, all pass, numbers in the
  table above. The capture XAPO silences what it records, so nothing is
  heard.
- `cargo test -p ddruntime --test input_audio`: 16 passed, 1 ignored. New:
  `xaudio_mixes_submitted_pcm_into_the_output` (exact x/32768 on both
  channels through the default mono-to-stereo send),
  `xaudio_decodes_the_games_adpcm_bank_format` (0x00DCE1F0's format and
  call, output equals ddaudio's decode), `xaudio_runs_guest_xapos_and_skips_unlifted_ones`
  (lock format/frames, in-place halving, unbound LockForProcess passes
  through, references balance).
- Pending (queued when the head paused builds): the `ddplay` build with
  `sink.rs` and its two unit tests, and the ignored
  `original_engine_initialization` run on the player's exe (lifted
  0x00DCE0E0 with the mixer in the loop).
- Implemented and compared against the original XAudio2 (2.7 and 2.9) for
  everything in the table; **not** compared against a recording of the
  game itself: the rebuilt game does not reach gameplay sound yet, and the
  game's XAPOs cannot run until their entries are lifted.

### Left open

- Lift `0x00DCCD00`, `0x00DCF3F0`, `0x00DCF3C0`, `0x00DCCD30`,
  `0x00DD5030` (non-RTTI vtable entries; a discovery rule is suggested in
  board #9121). Then the game's own effects run with no change here.
- The SRC/plain-copy switch pass (ratio moving to or from exactly 1) is
  not compared.
- Hosts without AudioReverb (non-Windows, or Windows without either DLL)
  play the reverb dry.
- Guest memory is read byte by byte when decoding (simple, not fast); fine
  for the game's voice counts in tests, worth a bulk read if profiling
  shows it.
