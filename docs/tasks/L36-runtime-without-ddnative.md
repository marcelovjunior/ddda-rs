# L36: ddruntime builds and tests without the lifted program

- **Owner:** claude-4
- **Branch:** `task/L36-runtime-split` (worktree `E:\ddda-rs-wt\L36`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

Every `ddruntime` build pulls `ddnative` and its 16 `ddlift` shards. In
a fresh worktree that is many minutes of compilation, and it is the
machine's bottleneck (several agents queue for two heavy slots). Most of
`ddruntime`'s code and tests do not need the lifted program: the Windows
services, threads, files, user32, the D3D9 layer, input, audio, network,
Steam and movies. They run against fixtures and test bindings. Only the
session's entry into the game needs it: `Session::start`, the initializer
census, the examples that run lifted code, and the tests that call
original functions.

## Task

1. **Make `ddnative` an optional dependency** of `ddruntime`, behind a
   default-on feature (`game`). Without it, `ddruntime` builds with
   `ddnative-base` only. List every use of `ddnative::` (and the
   generated resolver) and gate the session and dispatch glue that needs
   it. Prefer one small `cfg` boundary (for example a module that owns the
   resolver hookup) over `cfg`s scattered through the services.
2. **Gate the tests and examples** that need lifted code behind the
   feature: `required-features` in Cargo.toml for examples, and `cfg` at
   file or test level. All other tests must run with
   `cargo test -p ddruntime --no-default-features`.
3. **Measure:** a cold and a warm build of
   `cargo test -p ddruntime --no-default-features` against the current
   default, in a fresh target dir, in the heavy slot once heavy work
   resumes. Record the numbers.
4. **Update AGENT-RULES.md:** runtime work outside the game entry uses
   `--no-default-features`, needs no heavy slot, and uses
   `CARGO_BUILD_JOBS=4`.
5. **Merge-train rule.** This touches files many agents edit (Cargo.toml,
   lib.rs, session.rs, test headers). Keep the diff minimal and
   mechanical, and say in the hand-off what other branches must do on
   merge.

## Results

### The boundary

`ddnative` had exactly one use in `ddruntime`:
`Registry::with_lifted_program` (`src/dispatch.rs`), which registers
`ddnative::generated::lifted::all()`. `Session::new` and the `boot`
example call it. Nothing else names `ddnative` or the generated program.
So the boundary is a single `#[cfg(feature = "game")]` on that loop:

- `Cargo.toml`: `ddnative` is optional; feature `game = ["dep:ddnative"]`,
  on by default.
- Without `game`, `with_lifted_program` returns an empty registry. A
  session still loads the exe, maps it and binds every service, so tests
  that call services through the real exe's import tokens keep working.
  A call to an original address stops as "call to untranslated code",
  the existing unbound report.

Gated behind the feature:

| What | Why |
|---|---|
| examples `boot`, `call`, `initializers`, `winmain` (`required-features = ["game"]`) | run lifted code (CRT entry, initializers, `Session::start`) |
| `tests/runtime.rs` `the_crt_start_up_bridge_runs_the_heap_initialisation` | `Session::start` |
| `tests/input_audio.rs` `original_engine_initialization` (already ignored) | calls `0x00DCE0E0` |
| `tests/movies.rs` `installed_movie_delay_and_guest_stream`, `installed_movie_original_stream` (already ignored), their helper `installed` and its only user of `get_media` | call original delay-load and stream code |

The `imports` example and every other test need no lifted code.
`ddplay` depends on `ddruntime` with default features, so it is
unchanged.

### Measurements

In the heavy slot (2026-10-12): fresh target dirs, `CARGO_INCREMENTAL=0`,
`CARGO_BUILD_JOBS=3`, the same machine with other agents' jobs running.
Warm means one runtime source file (`services.rs`) touched, then a
rebuild.

| Build (`cargo test -p ddruntime --no-run`) | Cold | Warm | Target dir |
|---|---:|---:|---:|
| default (`game`: ddnative and the 16 ddlift shards) | 499 s | 23 s | 4.4 GB |
| `--no-default-features` | 16 s | 7 s | 249 MB |

That is 31x faster cold, 3.3x faster warm, and 18x less disk. Then
`cargo test -p ddruntime --no-default-features` passed every test: 6 test
binaries, 59 tests, 0 failed.

### For other branches on merge

- `crates/ddruntime/Cargo.toml`: keep `optional = true` on `ddnative`, the
  `[features]` block and the four `[[example]]` blocks; add your
  dependencies next to them (e.g. L19's `ddaudio`). An example that runs
  lifted code needs its own `[[example]] ... required-features = ["game"]`.
- A new test that runs lifted code (calls an original address or
  `Session::start`) carries `#[cfg(feature = "game")]`. Tests that only
  call services, including through a real-exe session, need nothing.
- Runtime work outside the game entry then builds with
  `cargo test -p ddruntime --no-default-features`, without the heavy slot
  (AGENT-RULES.md, "Runtime work without the game").
