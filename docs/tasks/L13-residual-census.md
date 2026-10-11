# L13: the residual: close what can be closed, account for the rest

Owner: Codex (same session). Branch `task/L13-residual` from the head's
`main` (eab8faa or later), worktree `E:\ddda-rs-wt\L13`.

## Why

After L12: 43,298 of 56,593 inventoried functions verified (76.5%, 90.8%
of code bytes); platform 12,707 (replaced, not translated). What remains of
the game's own code:

- about 497 lifted functions without a usable fixture (outside-image 287,
  runaway 88, NaN/Inf 78, other 44);
- 86 lift rejections (x87 input 26, rsqrtps/rcpps/rcpss 33, segment
  overrides 9, indirect 7, platform-dependent cpuid/rdtsc/pushfd 4,
  sfence 3, ...);
- functions the inventory calls "untouched" (490), "translated" or
  "documented" that are in neither list: find out why each one is not in the
  lifter's selection at all (thunks, data mislabelled as code, platform
  overlap, scan filters...).

The goal is every function translated 1:1 and verified, so the end state
of this task is: everything closable is closed, and every function that is
not verified has a recorded, evidenced reason.

## Task

1. Census: one table (private JSON plus a summary in docs/INVENTORY.md)
   covering every non-platform inventory function that is not verified,
   with its category and reason.
2. Close what can be closed exactly: e.g. `sfence` (a no-op for this
   single-threaded model? only with evidence), approximate reciprocals
   (rsqrtps/rcpps/rcpss: the host CPU result can be recorded and replayed if
   the recorder runs the same instruction on the host rather than in
   Unicorn: evaluate), segment overrides (`fs:` TLS/SEH access as a platform
   event), the x87-input cases, the fixture-less functions with targeted
   strategies (outside-image: trace a sample again after L11/L12 fixes).
3. Re-scan, record, adopt and report as before.

## Rules

As in L12 (`docs/tasks/L12-remaining-lift-rejections.md`, "Rules"). The head
seeds `LIFT_DATA=E:/ddda-re/notes/native-L13`.

## Results

Implemented an exhaustive private census and strict completion check in
`research/native/residual_census.py`; summary and regeneration instructions
are in `docs/INVENTORY.md`. It includes thunks, preserves per-seed recorder
evidence and body keys, identifies platform overlaps, and fails if any
unverified row lacks an accounted category. No shared inventory status was
rewritten to improve coverage.

**Starting census:** 832 non-platform/unverified inventory rows, comprising
588 non-thunks and 244 thunks (724 untouched, 59 documented, 49 translated).
The populations reconcile exactly: 244 excluded thunks, 493 genuine recorder
failures, 86 lift rejections, four quarantines, five explicit platform models.
Four of the old 497 missing candidates were already verified hand ports:
`0x008D4840`, `0x008FBDD0`, `0x0095A810`, `0x009C5890`. The scan now includes
unverified thunks and skips verified inventory rows. The five model overlaps
are `0x00CF73F0`, `0x00DE0B80`, `0x00DE0B90`, `0x012EFE20`, `0x012EFEA0`;
their later code citations explain the inventory's `translated` status.
Nothing is missing because of an unexplained size filter or presumed data.

**Exact closures:** SFENCE emits a real host SFENCE with a compiler memory
barrier; it is not discarded. Intel's store-ordering definition and the
synchronous `Mem` contract are documented in NATIVE_PIPELINE.md. All three
previous SFENCE rejects now lift (`0x00E76F20`, `0x011A7430`, `0x012C4B30`).
FLDL2E and FLDLN2 execute through the existing saved x87 hardware state;
synthetic tests compare memory and 80-bit return values. This exposes the
next unsupported operations, FYL2X at `0x00E2253C` in `0x00E22520` and
FRNDINT at `0x01035DD6` in `0x010359F0`, rather than claiming those whole
functions closed.

**Recorder diagnosis:** 24 addresses evenly sampled from L12's outside-image
list, objects-v1 seed 1, three-second bound, all reproduced outside-image
failures. The callee at `0x00E0BED0` contains a bounded switch at `0x00E0BF48`;
`static_pop` previously abandoned it and guessed zero cleanup. Cleanup
discovery now follows only bounded switches through the existing strict
recognizer (table-only inference disabled), requires consistent reachable
returns and rejects overlapping instructions. The 512-instruction bound and
unknown-flow zero fallback remain. Two of the same 24 inputs now succeed;
22 still reject. This refines recorded mock ABI choices, not callee execution
or a recovered general ABI. No original instruction/input or validity guard
was changed. Residual traces include returns with bad stack context and
internal entries such as `0x00D97031`, whose epilogue expects an existing frame.
These are not relabeled as invalid inventory entries without further evidence.

**Evaluated but not admitted:** `probe_approx.py` compares three legacy SSE
approximation instructions using 128 deterministic finite input vectors per
operation, MXCSR `0x1f80`, with host assembly and Unicorn 2.1.4. **128/128
vectors differ for each of RCPPS, RCPSS and RSQRTPS**. The JSON retains operands
and both outputs. Recording on the host is feasible for that CPU, but changes
the oracle and requires CPU provenance plus a cross-CPU replay/determinism
contract. It is not silently normalized into the current fixtures. FS accesses
are exception-chain reads/writes (`0x00CEE947`, `0x00CEE95F` and related bodies),
requiring guest thread/SEH state rather than host FS substitution. CPUID,
RDTSC and PUSHFD likewise retain their explicit missing-state/event reasons.
The remaining x87-input cases retain L12's conservative indirect-target/entry
stack analysis. These are documented unresolved execution contracts, not
claims of verified replacement behavior.

**Recording:** three deterministic seeds and a 20-second per-function budget,
six workers recycled after eight functions. Gains depend on pass order:

| Pass | Tried | Added functions | Added cases |
|---|---:|---:|---:|
| Random before switch-cleanup fix | 744 | 243 | 725 |
| Zero before fix | 501 | 3 | 9 |
| Small words before fix | 498 | 1 | 3 |
| Random after fix | 497 | 23 | 69 |
| Zero after fix | 474 | 3 | 9 |
| Small words after fix | 471 | 0 | 0 |
| Zero objects | 471 | 0 | 0 |
| Mixed objects | 471 | 0 | 0 |
| **Total** | | **273** | **815** |

The additions comprise all 244 thunks, three SFENCE bodies, and 26 prior
recorder failures. A thunk's fixture verifies its tail transfer, with the
callee either closed or explicitly mocked; it does not verify that callee.
None of the additions overlaps an already verified hand port. Existing
accepted function records remain unchanged. The controlled trace successes
are `0x0093BF30` and `0x00BA5210`.

**Final census:** strict `--require-evidence` passes with **559 entries**, all
non-thunks: 467 recorder rejections, 83 lift rejections, four quarantines and
five platform-model overlaps. No unexplained/unrecorded/unadopted entry remains.
The last mixed-object pass's primary failure counts (first seed, only the 467
current candidates) are outside-image 257, runaway 89, NaN/Inf 78, other 43.
Historical no-vector cache 450 -> 440 is a subset, not the census. The four
quarantines remain unchanged, including large-argument FSIN's known CPU/
Unicorn discrepancy at `0x00F276C0`; initializer `0x0137E490` still lacks a
usable fixture. Every baseline function record and case is unchanged.

| Measure | Before | After |
|---|---:|---:|
| Selected lifted entry points | 43,294 | 43,567 |
| Lifted cases | 128,772 | 129,587 |
| Verified non-thunk functions | 43,298 | 43,327 |
| Verified thunks | 0 | 244 |
| Verified non-thunk share | 76.5% | 76.6% |
| Verified summed non-thunk code bytes | 14,081,075 | 14,103,864 |
| Verified share of summed code sizes | 90.8% | 90.9% |
| Exhaustive non-platform/unverified census | 832 | 559 |
| Lift rejections | 86 | 83 |

The denominator stays **56,593 non-thunks / 15,516,093 summed bytes**. Verified
rows including thunks total 43,571, but that is not the conventional headline
coverage count. Private supplementary inventory is snapshotted for regeneration.

**Validation:** all **66 Python tests** pass (59 native, seven inventory).
The new census tests cover thunks, verified exclusions, platform overlaps,
stale trial keys and strict incompleteness failure. Switch-cleanup regression
checks bounded, unbounded, inconsistent and overlapping targets. Synthetic
SFENCE tests execute a non-temporal store, fence, and reload; four new x87
programs compare FLDL2E/FLDLN2 memory and 80-bit returns. Subset replay passes
**43,184 cases / 14,459 functions** and seven Rust units: compile 44.00s,
replay 11.24s, total 58.02s, minimum free RAM 20.41 GiB.
Final `DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative` passes all **32 Rust
tests**, including **129,587 cases / 43,567 lifted entries**. Compile 4m23s,
lifted replay 59.86s, monitored total 330.06s, minimum free RAM **12.73 GiB**.
Legacy tests retain their fixture-dependent behavior; required lifted replay
cannot skip. The final full-scan thunk-retention/verified-dependency regression
also passes; this scan-selection change does not alter the tested generated
manifest or call boundary. All evidence is under `E:/ddda-re/notes/native-L13`.

Four Cargo jobs, incremental off, at most six vector workers. L13's own
builds and vector runs were serialized. Other-session main-worktree builds
appeared during vector work and once during the full build; launches were
deferred when detected, and head was notified on the board. The zero-object
run finished before a checked stop could act, so no other process was killed.
Vector minimum free RAM was 13.55 GiB; the 6 GiB stop floor was never crossed.
No live-game or boot comparison was performed.

**Handoff:** private baseline/complete pattern-vector pairs, inventory copy,
census JSON, before/after scans, trial logs, 24-function traces, reciprocal
probe and resource logs. After review, head can adopt the accepted pair,
regenerate on main, then run `scripts/inventory.py build` against the usual
shared destination. Generated code and private evidence are not committed.
No shared native/inventory writes, native gate, Ghidra restart or push.
