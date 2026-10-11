# L19: audible audio (the XAudio2 2.7 mixer)

- **Owner:** claude-4
- **Branch:** `task/L19-audio-output` (worktree `E:\ddda-rs-wt\L19`)
- **Status:** open

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

(open)
