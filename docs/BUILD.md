# Building the game exe fast (L75)

How the play builds (`play`, `play-fast`, `play-o0` in `Cargo.toml`) spend
their time on the maintainer's machine, and the settings that cut it.

The machine: Ryzen 7 5700X3D (8 cores, 16 threads), 32 GB RAM, Windows,
MSVC toolchain, Rust 1.99 stable (`RUSTC_BOOTSTRAP=1` for `-Z` flags).
Several agents build at once, so every wall time below says what else ran.
CPU time and the core count ("x2.2") are the more stable figures.

## Use

    scripts/build_play.sh [--profile play-fast] [--gate] [--timings] TAG

The script:
- takes a heavy slot (`HEAVY_BIG=1`);
- fixes the environment (`RUSTC_BOOTSTRAP=1`, `RUSTFLAGS=-Zthreads=8`, no
  `CARGO_INCREMENTAL`), so every caller hits the same fingerprints;
- retries a build whose rustc was killed by the memory guard, but stops on
  a real compile error;
- copies `ddplay.exe` and `ddplay.pdb` to `<target>/runs/TAG`;
- prints the time and the crates it compiled.

The default target directory is the profile's shared warm one
(`C:/ddda-targets/playfast` for play-fast).

`--gate` runs cargo with `-j 16` and `scripts/rustc_gate.rs` as
`RUSTC_WRAPPER` (see "Job tokens" below). The wrapper is not part of
cargo's fingerprints: switching it on or off rebuilds nothing.

## Where the time goes

A play-fast build of ddplay compiles about 150 crates. Three things matter:

- **32 lifted shard crates** (`crates/ddlift/lift_NN`, ~10 MB of source
  each). Under play-fast, 15 of them are at opt-level 1 and 17 at
  opt-level 0.
- **ddnative**, which depends on every shard: ~8-12 s.
- **ddruntime, ddplay and the link.**

Times measured on ddlift-05, rustc run directly, on a loaded machine:

| what | wall | CPU |
|---|---|---|
| opt-level 0, `-Zthreads=1` | 34.7 s | 38.8 s |
| opt-level 0, `-Zthreads=8` | 17.0 s | 47.3 s (x2.8) |
| opt-level 1, `-Zthreads=8` | 150-216 s | 323-443 s (x2.0) |
| opt-level 1, `-Zthreads=8`, split into 57 modules | 72-94 s | 487-540 s (x5.7-6.7) |

Peak memory: ~1.9 GB at opt-level 0, ~1.9 GB at opt-level 1, and ~3.0 GB
at opt-level 1 split.

### Codegen units need modules

`codegen-units = 64` had almost no effect on the shards, because rustc
never splits one source module across codegen units. `lift.py gen`
writes each shard as four modules (`gen(chunk=400)`: 400 functions per
`lifted_NNN.rs`).

`-Zllvm-time-trace` on ddlift-05 at opt-level 1 shows:
- 14 codegen units, only 4 of them with any work;
- LLVM optimisation times of 99, 58, 32 and 19 s, which run nearly
  serially (2 cores busy);
- the largest single function takes 15 s;
- the passes that dominate: InstCombine 47 s, loop passes 41 s, LICM 19 s,
  of 208 s optimiser time in total.

The same source split mechanically into 57 modules of ~170 KB (whole
functions, in order; tool `C:/ddda-targets/L75/tools/split.py`) builds
in 72 s instead of 216 s, back to back. It costs 10% more CPU and 1 GB
more peak RAM. At opt-level 0 the split is also a little faster (20.7 vs
26.5 s).

Decision (head): `gen` chunks by bytes (claude-7, lift.py). Accepted
side effect: at opt-level 1, calls between lifted functions in different
modules of a shard are no longer inlined. FPS is to be checked by the head.

### Job tokens cap the threads

rustc's parallel front end (`-Zthreads`) and its LLVM workers take
cargo's jobserver tokens. A build with `-j N` therefore runs at most N
threads in total, whatever `-Zthreads` says.

ddlift-13 (opt-level 0, `-Zthreads=8`) rebuilt through cargo:

| `-j` | wall |
|---|---|
| 1 | 37-44 s |
| 4 | 20 s |
| 16 | 17 s |

Under a fake jobserver with no spare token, rustc run directly behaves the
same (x0.9 cores). The unstable `--jobs-frontend` does not avoid this.

heavy.py sets `CARGO_BUILD_JOBS` from free RAM (2-15). So a cold build
leaves cores idle while the opt-level 1 shards finish, and a warm build
that recompiles one shard gets as few as 2 threads.

`scripts/rustc_gate.rs` decouples the two limits. Cargo runs with
`-j 16`, and the wrapper limits the shard compilers instead:
- At most `DDDA_GATE_SHARDS` shard compilers run at once (build_play.sh
  derives the count from free RAM).
- None starts while less than `DDDA_GATE_FREE_MB` is free.
- Opt-level 1 shards go first.
- A waiting shard lends its job token to the pool and takes one back
  before it starts.

Other crates pass straight through. A 5-shard test admitted the shards
in order and built correctly.

### Linking

ddplay.exe (386 MB) relinked from the captured link command:

| linker | `/OPT:REF,ICF` (rustc's default) | `/OPT:REF,NOICF` | `/OPT:NOREF,NOICF` |
|---|---|---|---|
| link.exe 14.44 | 5.0 s | 3.4 s | 3.1 s (exe 506 MB) |
| rust-lld (lld-link) | 2.8 s | 2.5 s | 2.5 s (exe 405 MB) |

- link.exe `/INCREMENTAL` relinks in 0.9 s, but needs `/OPT:NOREF`, which
  grows the exe to 506 MB.
- lld's PDB still holds the public symbols that `scripts/sample_profile.py`
  needs: all 59,822 lifted functions, checked with dbghelp.
- Setting the linker (`target.x86_64-pc-windows-msvc.linker`) changes
  every crate's fingerprint, so it is to be switched together with a
  change that rebuilds all shards anyway (head's decision: with the
  byte-chunked shards).

### Rebuild cases (warm target, play-fast)

| change | crates | time |
|---|---|---|
| runtime only (one new fn in ddruntime) | ddruntime 2 s, ddplay 8 s (link 5 s) | 9.8-11.3 s |
| one opt-level 0 shard (ddlift-13), jobs=2 | shard 26 s, ddnative 8 s, ddruntime 1 s, ddplay 6 s | 34 s |

### Did not help (shard 05)

All measured on a loaded machine; differences under ~20% are noise.
- `-Cllvm-args=-fast-isel` at opt-level 1: slower (173 vs 150 s), bigger rlib.
- codegen-units 16 or 256 without the module split: no gain (16 was slower).
- `-Zmir-opt-level=0` at opt-level 0, and `-Zinline-mir=no` or
  `-Zmir-opt-level=1` at opt-level 1: no clear gain. These would also
  change the generated code.
- `-Zthreads=16` versus 8: no gain (8 cores).

### Caching across target directories

Cargo's `-C metadata` hash of a workspace crate does not depend on the
checkout's path: two worktrees produce the same file names. sccache is
not installed. Its hits would need the same lift output, the same
ddnative-base and the same flags. When lift.py or ddnative-base changes,
every shard changes anyway, and a warm target already skips unchanged
shards. So a shared cache only pays when several agents cold-build the
same commit. Not pursued.

## Open

- A clean cold play-fast build before/after (split + gate + lld), run
  overnight when the head releases the machine. Partial data from the
  stopped run: 4 gated shard compilers, opt-level 1 shards 113-275 s each
  with 4 sharing 16 tokens, minimum free RAM 8.5 GB.
- A cold build is CPU-bound: 15 opt-level 1 shards at ~320-490 CPU-s
  each. Fewer functions at opt-level 1 (L66, function-level hot/cold
  layout) is the lever for cold builds; split and gate shorten the tail
  and fill the CPU.
