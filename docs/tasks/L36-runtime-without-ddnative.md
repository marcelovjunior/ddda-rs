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

(open)
