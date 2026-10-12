# L44: deterministic xaudio tests

- **Owner:** claude-10 (assigned by the head on the board)
- **Branch:** `task/L44-deflake`

## Why

`crates/ddruntime/tests/input_audio.rs`'s XAudio2 tests slept real
milliseconds and then asserted what the audio worker had processed by
then. Under load the worker ran late, and about 1 run in 3 failed
(train 1's first run included). Flaky tests waste the expensive trains.

## Results

**The change: a test clock in `audio.rs`; production timing is unchanged.**
- `audio::use_test_clock()` makes the engines a session creates from then
  on run on a clock the caller advances, `audio::advance_test_clock(d)`.
  Each worker then processes exactly the 10 ms quanta that have elapsed on
  that clock, one at a time, on its own guest thread as before.
  `audio::test_clock_settled()` tells when every worker has caught up.
- A worker is registered when StartEngine starts it, not when its thread
  first runs. So "settled" cannot be true before the worker has begun.
- Without `use_test_clock` (the game, ddplay), `worker` runs the same
  real-time loop as before.

**The tests:**
- `setup()` turns the test clock on.
- Every `sleep(N)` before an assertion is now `advance(N)`: advance the
  clock, then wait in guest Sleeps until the workers have settled (10 s
  limit).
- The teardown sleeps (15 ms, before the fault checks) stay.
- One assertion is now exact. The looping voice has played exactly 1440
  samples after 35 ms (three quanta of 480); it was `>= 960`.

**Evidence (light checks, no lifted program):**
- `cargo test -p ddruntime --no-default-features --test input_audio`, 30
  runs in a row: 30 passed.
- Under load, the test binary run 8 at a time, 10 rounds: 80 of 80 passed.
- The same load against the old tests (the L43 build of main's
  `input_audio`): 38 of 40 failed.
