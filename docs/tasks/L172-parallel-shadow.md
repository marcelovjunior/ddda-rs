# L172: private parallel shadow execution

Owner: codex-11 (lead), with codex-7 on recording lanes and stream equality.
Branch/worktree: `task/L172-parallel-shadow`, `E:/ddda-rs-wt/L172`.
Base: `task/fps-line`; dependencies merged from `task/L162-job-wake` and
`task/L169-shadow`. User assignment: execute the critical uSkyStableShadow
job in parallel through private recording lanes and prove serial command
stream equality, opt-in `DDRUNTIME_PARALLEL_SHADOW=1`.

## Design posted to head

Board #13922 selects the existing DB63E0 parallel protocol, with a PRIVATE
nested scheduler and contiguous unit ranges, followed by lane-index merge.
EFDC70 cascades remain ordered: EFD240 mutates shared shadow resource state,
so independent cascade execution is not established. Codex-7 owns the lane
layout, strict command snapshot, original FED3B0 fixtures, and
`L172-recording-lanes.md`. Codex-11 owns runtime orchestration and integration.

The global scheduler is already processing the outer batch. Neither its
queue nor global nesting guard may be repurposed. Source addresses are from
DDDA.exe 2017-12-13 Steam, inspected read-only through the canonical Ghidra:

- DB63E0 chooses serial if global 018D0BB0 is nonzero or scheduler+98 is one.
  Its parallel branch initializes lanes with FED200, publishes eight pointers
  at scheduler+1BDD0, stride 0x20, and queues DB7620 unit callbacks.
- DBE560/DBD8D0 share queue count +8C, claiming counter +90, mode +94 and
  worker slots. Modes zero through three use differing callback arguments.
- FED3B0 concatenates recording lanes at stride 0x33C84 in lane order,
  including relocation of type6 nested command arrays. Arbitrary work
  stealing does not preserve serial command order.
- 107FF00/107FF30 dispatch the singleton at 018D1848, sPrimitive, through
  vtable methods +2C/+30: E61010/E68390. E68390 queues additional scheduler
  kind2 sorting (111E180) and kind3 primitive drawing (10B7440). The singleton
  holds shared pools, lane pointers and merge cursors. A private scheduler
  alone does not isolate those operations.
- EFDC70 calls EE5CE0 and EFD240 for each cascade, then EEA950. EFD240 writes
  resource use stamps and invokes virtual methods; parallel cascades would
  also traverse the same model objects.

## Implemented proof path

`ddruntime/src/parallel_shadow.rs` provides confined, private host-thread
execution. This is host infrastructure, not translated scheduler code:

- One immutable, explicitly captured input; absent pages refuse execution.
  No fallback to live memory and no speculative writes to the running guest.
- Up to eight contiguous lane ranges, bounded per-lane memory accesses,
  joined in lane order, with every worker joined even when another fails.
- Per-lane writes and read-before-write tracking. Native `Pages` accessors
  fall back through the tracking Mem interface; exposing the original raw
  page table would bypass this gate.
- All-or-nothing merge into a new snapshot. Cross-lane read dependencies,
  differing shared writes, wrong input snapshots and permuted lanes reject.
  Identical write-only stamps commute. Only explicitly proven popped-stack
  scratch may be excluded; memory at/above returned ESP must remain checked.
- `audit` runs a confined serial path and private parallel path from the same
  immutable input, invokes an explicit ordered merge, and compares the
  complete consumed command payloads with `mtcore::shadow_lanes`. Unknown
  records, incomplete captures, mutations and command-order changes reject.

`DDRUNTIME_PARALLEL_SHADOW=1` now installs a **proof runner**, requiring
Cargo feature `ddruntime/parallel-shadow`. The live traversal runs serially
once. Its Mem journal captures initial pages and validates every subsequent
read against captured input plus its own writes, preserving u16/u32 access
widths. It seeds the original eight inline lane contexts, both entry arenas,
the scheduler, the TEB's stack range, all primitive pools and the exact
primitive manager arenas documented by codex-7. Missing/inconsistent input
refuses the audit; there is no lazy fallback to later live memory.

New helper threads run original raw bodies against immutable captured input.
A scoped TLS resolver precedes the cross-shard cache and local global
overrides. Their external handler permits only raw bodies, private scheduler
execution and Mem-only memcpy/memset. It does not install a Runtime, live
Registry, machine clock, native sink or OS services. Unknown imports reject
before invocation. Private DBE560 dispatch uses DBD8D0's mode1/2/3 thiscall
arguments, per-lane context slots, contiguous job partitions and dependency
checked lane-order merge. Deeper scheduler recursion is refused.

The serial replay must first match the live serial ABI, command stream and
all journaled final memory except popped stack scratch. Only then are the
serial and parallel streams compared, followed by conservative final-memory
and ABI comparison. The latter still includes private scheduler/lane state;
it may reject an otherwise equal stream. A mismatch is a refusal, never an
accepted result. All live results remain serial regardless of audit outcome.
The requested faster production publication is **not implemented/accepted**.

Diagnostic bounds are explicit: 64 MiB input pages, 2 MiB tracked byte entries
per lane/journal, 40 million byte accesses per replay lane, at most eight
selected traversals (default one), two replay workers. These are host safety
bounds, not inferred game limits. Hitting one refuses rather than truncating
an accepted comparison.

## Default-path cost

Head authorized the narrow runtime routing seam in board #14018; claude-2
agreed to the x86 cold-path hunk in #14042. The existing external dispatcher
is untouched. An opt3 isolated benchmark found all nine pairs slower with
an unconditional TLS check (old 3.765–5.961 ns, new 3.943–8.040 ns; noisy
ratios 1.008–1.889). Accordingly the call hooks are behind a Cargo feature;
ordinary builds compile exactly the previous call-routing path. The replay
APIs alone do not enable hooks. Setting the environment switch on a build
without the feature refuses startup explicitly.

## Head-owned proof run and offline reproduction

Build only in the head's shared train/heavy slot; the agent did not build
shards or the game-required example:

```
cargo build -p ddplay --profile play --features ddruntime/parallel-shadow
cargo build -p ddruntime --example shadow_replay --features parallel-shadow
```

For one head-owned real run, enable the normal parallel runtime plus:

```
DDRUNTIME_PARALLEL_SHADOW=1
DDRUNTIME_PARALLEL_SHADOW_FIRST=300
DDRUNTIME_PARALLEL_SHADOW_COUNT=1
DDRUNTIME_PARALLEL_SHADOW_DUMP=E:/ddda-re/notes/native-L172/replay
```

FIRST counts shadow-scoped DB63E0 calls, not Presents; adjust from the head's
route if necessary. COUNT defaults to one and clamps to eight. Log lines
begin `parallel-shadow`, name the private dump and report `publish=serial`.
An empty stream never counts as acceptance. No speed/FPS comparison should
use this intentionally expensive proof run.

The dump includes the frozen page permissions/bytes, explicit CPU wire state
and live serial writes. Files are bounded, created without overwriting and
remain private game-data artifacts. Once the head builds the offline example:

```
shadow_replay.exe E:/ddda-re/notes/native-L172/replay/shadow-PID-INDEX.ddshadow
```

It creates no session or game window. It checks captured serial execution and
parallel execution from the same input, permitting subsequent diagnosis
without another game launch. Header, page permissions, size, duplicate
addresses, truncation and trailing bytes are validated. Retain the head's
binary/environment receipt and hash the corpus; no real corpus exists yet.

## Remaining production gates

1. Obtain the head's nonempty real capture and close any unsupported import,
   missing input or dependency refusal using source-backed private semantics.
   Uncontended allocator critical sections now have a private guest-memory
   model; foreign owners/waiters reject. Shared allocator dependencies still
   require source-specific handling and are never ignored.
2. Classify/restore private scheduler and lane bookkeeping without excluding
   shared allocator or resource changes. No arbitrary memory exclusions.
3. Prove real same-run serial/parallel command, resource state and ABI equality
   before publishing candidate output or advertising TRAIN READY.
4. Head alone measures whole-shadow time and matched image/FPS acceptance.

## Results

Head capture run cap172 reached gameplay but produced zero files and zero
parallel-shadow lines. Hook installation, first shadow entry, first scoped/
unscoped traversal and sample start now log explicitly to distinguish a
missing environment switch from a routing/selection failure. Awaiting launch
receipt and rerun. Pre-service full feature-light gate:277 passed,0 failed,
7 ignored; final focused replay gate:10 passed both with/without feature.


Serialized synthetic end-to-end replay now checks four ordered query entries
and rejects command, resource, MXCSR and unsupported-target mutations. Private
Enter/LeaveCriticalSection resolves only from captured IAT slots referenced
by 0107FB57/0107FBAE, updates the runtime lock words exactly, and rejects
foreign owners, waiters and unmatched leaves. Ten focused replay tests pass,
including differential recursive service checks and shared allocator
read-dependency rejection. This is runtime-contract/synthetic evidence, not
real shadow equality. No live publication is enabled.


Publication preflight is implemented separately from the live hook. It checks
all candidate read/write locations and optional same-run serial baseline,
including permissions, before any store. It requires an externally proven
exclusive boundary; optimistic preflight alone is not concurrency safety.
Three new refusal/success tests pass (11 executor tests total). Live use is
still gated on actual capture/source evidence.


Continuation: replay now checks the full x87 image, saved/live MXCSR and
thread control state in addition to its existing ABI/stream/memory gates.
Capture format v2 retains live outgoing MXCSR separately from the saved
CPU value; v1 refuses because that evidence was absent. Five focused
replay tests pass, including distinct saved/live MXCSR round-trip and
11 deliberate FP/control-state mutations. Real capture remains pending.


Default light gate before the final capture-file addition: **271 passed,
zero failures, seven ignored**. Final focused shadow gate: **17 passed**
(includes eight executor tests, three private replay tests, one bounded
file round-trip/corruption test and five prior FEE320 capture tests). Base:
**41 passed, zero failures, one ignored**. Feature-enabled routing: **two passed**, plus one ignored timing benchmark.
Final feature-enabled light: **272 passed, zero failures, seven ignored**. Earlier inherited
Present/tail timing failures both passed isolated retries. Synthetic tests
are explicitly not live shadow input or original-game acceptance.
Codex-7's original merge and comparator evidence is recorded separately in
`L172-recording-lanes.md`.

No game, GPU test, shard build, push, installation write, private data copy,
or decompiler dump committed. Production TRAIN READY is not claimed.
