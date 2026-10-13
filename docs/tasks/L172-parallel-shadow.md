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

Second real v3 capture shadow-48204-300.ddshadow is complete (no rejection):
1,633 pages/13,226 write bytes, SHA256
c3414a55e0ea0a692d07c3f2aa2576cac3195d46be2e1ea89904bf32e74d7f80.
Original serial/live comparison passes for6 entries. Parallel stops at
CreateEventA (captured token0x7ff00490); no equality is claimed.

The captured root is exactly manager+d40+LANE_BYTES: initializing nested
children in place would overwrite the parent lane. Replay now copies the
complete manager header/inline contexts privately and retargets DB63E0 this
only; its original parent argument and object/list pointers are preserved.
PublishedView forbids references into both temporary manager and sort arena.
33 focused shadow tests pass, including unaligned private-copy isolation and
manager retargeting. Unsupported-import diagnostics now include guest return
address, so the event creation can be traced before implementing any service.
Next: head rebuild offline example only, replay the complete existing capture.


Nonempty-work gate now requires appended commands, not merely a nonempty
pre-existing stream. The real capture goes from2 to6 roots (four appended).
Fifteen focused replay tests pass, including refusal when an unchanged
nonempty prefix would otherwise look like successful shadow work.


Real v3 capture shadow-44100-300.ddshadow:6,759,804 bytes,1,632 pages,
13,226 written bytes; SHA256
4193551ef5af0d3a409f85f5ee5375a6b60dff89552ce91b12c1fcfe2a4fadef.
It is an outer lane (context+58=1), whose secondary arena at+54 is zero.
FED200 explicitly initializes child+54 to zero, so capture no longer treats
that absent serial input as a failed read. Parallel replay supplies bounded
private sort storage. FED3B0 copies roots/nested arrays back to the parent;
a read barrier requires every consumed output byte to leave private storage
before those private writes can be discarded. The original+54 is restored.
All other memory/ABI/resource gates remain. Private addresses are host-only
snapshot allocations, never reservations or writes in the live guest.

Old-capture serial execution completes; parallel first refuses missing page
0x647066f4 (sRender+0x433e04, read only by FED200). New capture preseeds the
exact FED200 frame-ring and shader header fields. Serial/live verification
now runs BEFORE parallel execution and reports its own result. Thirty-one
focused shadow tests pass, including arena bounds, output-reference rejection
and serial-mismatch-before-parallel checks. Head recapture is required;
old rejected input never becomes accepted. Production publication remains
open; no TRAIN READY or speedup claim.


Capture format v3 now saves bounded diagnostic fixtures even when preseed or
stream capture rejects. The journal retains its first failure while recording
later valid serial input/writes. The file contains the rejection reason;
offline replay can diagnose it but ALWAYS returns rejection, even if the
synthetic stream/memory comparison succeeds. Live execution does not replay
or publish rejected captures. Twelve focused replay tests pass, including
round-trip rejection metadata and diagnostic-match-cannot-accept coverage.
Head must rebuild both binaries with v3 for the queued capture.


First actual cap172 run (#14314/14315) confirms hooks and sample300 entry,
but rejects capture at address0 before writing a file. The old failure word
conflated host allocations with unreadable seeded pages and overwrote first
causes. Capture now retains the first reason/stage/address, reports rejected
seed bounds and host allocation sizes. Ten replay regressions pass; no guard
was removed. Next: head rerun to resolve the concrete capture blocker.


Final default light suite with --test-threads=1: **281 passed, zero failed,
seven ignored**. The preceding concurrent default run failed the inherited
Interlocked/Present timing assertions (129/2/5 library); both pass in the full
serial-test rerun. No checks were changed or skipped. Final L172 focused
coverage: 10 replay tests plus 11 executor tests.


Head corrected cap172 receipt (#14296): the build failed before launch on
a Cargo feature merge conflict; the existing run log was stale. No capture
run occurred. The head fixed the conflict and is rebuilding85868a8a. Hook installation, first shadow entry, first scoped/
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

2026-10-13 real replay follow-up: pinned a66f3de7 example verifies serial/live
6 entries from shadow-48204-300; private manager copy removes the previous
CreateEventA detour. Next refusal is InterlockedExchangeAdd, called at
DA7D80 through IAT 0139D0A8 (return DA7D86). Private replay now resolves
that captured slot to the existing pure Mem/Cpu service; no raw page table
means tracked reads/writes. Regression covers wraparound, old return value,
stdcall stack, preserved EBX and unchanged live memory. Focused shadow:33
passed/0 failed. No new original parallel equality claim yet.
Capture was t23s/169 draws with one queued job. Added diagnostic selection
DDRUNTIME_PARALLEL_SHADOW_AFTER_MS=60000 FIRST=0 COUNT=1 for a later sample
(FIRST counts scoped calls after the delay). Next: head rebuild offline
example, replay existing input, then obtain a representative multi-job capture.

2026-10-13 private scheduler follow-up: DB63E0/DBE560/DBD8D0 queue
storage is now copied to a bounded private arena and018D0B28 redirected
only inside replay. The existing outer scheduler bytes remain unchanged.
Nesting guards and global pointer must balance before restoring their
captured values; consumed commands may not refer to private scheduler
storage. Synthetic serial now invokes jobs directly while synthetic
parallel uses the private queue. Serialized regression additionally rejects
unbalanced guard, changed scheduler pointer, and scheduler-pointer escape.
Shared allocation dependency regression now invokes the actual tracked
InterlockedExchangeAdd service. Focused shadow:33 passed/0 failed.
The complete default light suite at661bdb1a:288 passed/0 failed/7 ignored.
Original parallel equality remains OPEN; head offline example rebuild next.

2026-10-13 original replay at head build aeada9d1 (includes1b384b1f):
serial/live6 entries PASS; private scheduler mode1/jobs1 gets past XADD.
Next refusal is MissingPage0x64d31820 at EA5B50, a fresh DA7D70 command
allocation. The serial root reused its current page (cursor advanced0x410;
render cursor stayed0x1e000). Capture now seeds the current DA7D70 arena
from render+33DC0+4*ring_index, index at+33DB8, capacity at+33DCC.
All bytes come from the same live capture; absent old-fixture bytes are
never fabricated. Regression covers inactive-ring exclusion and previously
unvisited pages, including an unaligned arena end, with no live writes.
Focused shadow:34 passed/0 failed. Next head action: rebuild ddplay plus
shadow_replay with ddruntime/parallel-shadow and run capture with
DDRUNTIME_PARALLEL_SHADOW=1 DDRUNTIME_PARALLEL_SHADOW_AFTER_MS=60000
DDRUNTIME_PARALLEL_SHADOW_FIRST=0 DDRUNTIME_PARALLEL_SHADOW_COUNT=1
DDRUNTIME_PARALLEL_SHADOW_DUMP=E:/ddda-re/notes/native-L172/replay.
Still no original parallel equality, production publication or TRAIN READY.

2026-10-13 gameplay capture shadow-48124-0 (16,223,235 bytes), head
fca9c47d build: serial/live66 entries PASS, private scheduler175 jobs. Both
callback lanes complete; merge refuses renderer command cursor64306CB9.
Closed with a transaction-local shared atomic counter for the exact
sRender+33DC8 InterlockedExchangeAdd used by DA7D80. Claims preserve
return-old/wrapping semantics and synchronize only within this replay;
allocation capacity checks remain in the original DA7D70 body. Ordinary
reads or writes of that counter by any lane reject with AtomicConflict.
All other read/write dependencies stay checked; only after successful
merge is the counter stored once in parent private memory. Allocation
order may vary across workers, as with the original atomic allocator;
strict consumed-stream comparison is still mandatory. Synthetic tests
cover disjoint claims, wraparound, unchanged input, and ordinary-access
refusal before any child writes are absorbed:35 focused pass/0 failed.
Next: head offline-example rebuild and replay48124; no new game needed.
Production publication/equality remain OPEN.

2026-10-13 head build3a4dfe16/c78206fc replay48124: serial/live66
PASS;175 jobs; now refuses AtomicConflict64306CB8. Ordinary cursor
access exists, so the atomic-only ownership assumption is not accepted.
Moved refusal to PrivateMemory read/write site to preserve its caller
backtrace; end-of-batch ownership check remains.35 focused tests pass.
Next: offline rebuild/replay identifies the exact ordinary access.
