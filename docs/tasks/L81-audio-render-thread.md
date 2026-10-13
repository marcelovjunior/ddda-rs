# L81: audio work attributed to the render thread

Assigned by head to codex-2, branch `task/L81-audio-render-thread`, based on
`task/opt-int`. Scope: `crates/ddruntime/src/audio.rs`, `crates/ddaudio`,
focused tests/benchmark and L81 documentation. Runtime-only, no game runs,
no shard builds. User assignment is the brief (no pre-existing L81 file).

## Results

### Attribution

The two supplied private gameplay profiles do **not** establish an audio
path on render guest thread `0x1034`. In `profile_gameplay_render2.txt`,
lines 520 onward identify the audio worker as guest `0x103c` (5.6 CPU seconds
in 40 seconds); its inclusive frames include `audio::worker`, `process`,
and `Mixer::render`. The render thread instead spends 34.1 CPU seconds in
40 seconds and lists graphics/dispatch frames, with no audio worker/mixer
frame. Source agrees: `start_worker` creates a separate guest thread;
`worker` calls `process`, which consumes buffers and renders each 10 ms
quantum. `submit` queues guest pointers/metadata; it does not mix or copy
sample bytes. Audio feed executes under the guest lock, so its cost can
still delay another guest thread waiting for that lock.

The 6.5% self / 10% inclusive `Vec<u8>::extend_from_slice` symbol is a
generic allocation-library function. Its first profile's ddaudio suffix
is not caller evidence. The only production u8 slice extension in ddaudio
is a 42-byte reverb parameter conversion for the 2.9 fallback, skipped
when parameters are unchanged; resampler extension is f32. Shared/folded
symbol attribution is the likely explanation, not proved from these
aggregate reports. `ddgpu::constants` (22.2% inclusive) has repeated
four-byte extensions and is a candidate caller; reported to head and
claude-2, who owns the graphics path. A caller-resolved profile is still
needed to identify the exact render-thread copy site.

### Implemented

The actual audio source feed already used one `Mem::read_bytes` bulk read,
not byte-by-byte slice extensions. It allocated byte and decoded-float
vectors on every nonempty consumption segment, cloned the ADPCM coefficient
vector, and allocated an ADPCM block buffer. Now each voice caches its
immutable decoder format (sample-rate updates remain synchronized), and
Audio owns reusable byte/float/ADPCM scratch. Each feed still reads guest
memory at consumption time, before any buffer-end callback can recycle it.
No scratch borrow spans guest callbacks. PCM removes two temporary
allocations per segment; ADPCM removes four, after capacity warmup.
Mixer input storage may still grow; the render pass and graph snapshots
retain their existing allocations. This is not a claim that all mixing
is allocation-free. No sample arithmetic or callback timing changed.

### Validation

- First `cargo test -p ddruntime --no-default-features`: 134 passed,
  manual benchmark ignored. Final reruns (parallel and serial) hit the two
  existing timing assertions `present_runs_without_the_guest_lock_in_parallel_mode`
  and `interlocked_does_not_wait_for_the_guest_lock` under shared-machine load;
  the audio differential still passes. These tests do not exercise audio.
  No checks or thresholds were weakened. Head should rerun when quiescent.
- `cargo test -p ddaudio`: five passed; nine host tests ignored by default.
- Explicit serial host oracle run: all nine passed against host XAudio2.
- New synthetic recorded-like stream regression compares pre-L81 feed and
  reused feed mixed output bit-for-bit: mono/stereo/six-channel PCM and
  game-layout mono ADPCM, uneven chunks, backward seeks, underrun, volume
  ramp, guest-memory reuse, and shared scratch across formats. No recorded
  game bytes are embedded. Stable scratch pointers checked after warmup.
- Final serial `input_audio` integration rerun: all 20 passed, including
  guest XAPOs, buffer callbacks and unlocked mixing.
- Optimized differential and manual benchmark also pass.

Benchmark command (CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=4,
CARGO_TARGET_DIR=C:/ddda-targets/L81):

```
cargo test -p ddruntime --no-default-features --release --lib benchmark_source_feed -- --ignored --nocapture
```

The benchmark retains the old allocation path, times 480-frame feeds through
real GuestMemory, alternates old/new order, discards 100 warmup calls and
takes the median of 5,000 calls per format. Mixer drain is outside timing.
Both paths pay the same timer overhead. Median is used because concurrent
agents cause scheduling outliers in elapsed-time averages.

Three repeat runs after the optimized build (median call time, microseconds):

| Input | Before, runs 1/2/3 | After, runs 1/2/3 | Speedup range |
|---|---|---|---|
| Mono PCM16 | 2.3 / 2.5 / 2.4 | 1.9 / 2.1 / 2.1 | 1.14-1.21x |
| Stereo PCM16 | 2.9 / 3.4 / 3.5 | 2.7 / 3.2 / 3.2 | 1.06-1.09x |
| Six-channel PCM16 | 7.7 / 8.2 / 8.2 | 7.1 / 7.7 / 7.8 | 1.05-1.08x |
| Mono ADPCM | 4.2 / 4.3 / 4.3 | 3.6 / 3.7 / 3.7 | 1.16-1.17x |

Shared-machine microbenchmarks, not isolated frame or whole-mixer timings.
Earlier elapsed averages had scheduler outliers, including one apparent
six-channel slowdown; these medians do not imply an FPS improvement.

### Limits and next action

No original-game or ddplay run, no FPS claim, no shard build, no GPU test.
Head should integrate with the graphics changes and collect a fresh
caller-resolved render-thread profile/FPS comparison. The reported byte-copy
hotspot remains a graphics/profile-attribution investigation, not evidence
that the mixer should move threads.
