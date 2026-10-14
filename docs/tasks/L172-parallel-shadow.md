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

2026-10-13 pinned749097e4/815df443 replay confirms direct cursor reader
is original DA7DB0 on both lanes, called by DB7620. Ghidra and generated
body agree: read cursor+headroom, unsigned compare against capacity.
Private resolver now runs that ORIGINAL body under scoped permission for
its aligned u32 counter load. It sees the synchronized transaction counter;
all flags/register effects still come from the original body. Byte reads,
ordinary writes, and other callers remain refused. Regression proves the
helper sees updates and permission ends on return;35 focused pass.
Next: offline rebuild/replay48124; no recapture required.

2026-10-13 pinned b8f57fb6/8750e8bf replay48124: serial/live66 PASS,
175 jobs, lane dependency merge PASSES. Full stream comparison now rejects
root entry2 record header. Added bounded key/header/count diagnostics;
no header masks or comparator relaxation.35 focused tests pass. Next:
offline rebuild/replay to identify exact bits and trace original consumer.

2026-10-13 source-backed sequence relocation helper (NOT WIRED):
FEF830/FFC8E6 store context+74 in header low15 and increment the full
context counter. CORRECTION: FED200 retains child counters; FED3B0 joins lanes in order.
The helper validates complete, unique local ordinal domains and parent
root/counter invariants, then rebases candidate low15 into serial ordinal
space in lane order and advances the parent counter. All other bits remain
exact; comparator unchanged. Nested kind6 records are traversed; associated
draw kinds4/5 conservatively refuse pending evidence. Tests cover nested
creation order,15bit boundary, preservation of all other bits, duplicate/
missing ordinals and no writes on refusal.37 focused pass/0 failed.
Complete default light at8750e8bf:290 passed/0 failed/7 ignored.
Awaiting initial R172 warm build, then exact header diagnostic BEFORE
integrating the helper; future offline builds self-served via head script.

2026-10-13 self-serve R1721f6e756f replay48124 exact diagnostic:
root2 key5B010B77 matches; serial header162AC vs parallel16923;66/66
entries. Only low15 differ. Correction to the unwired prototype: FED200
retains +74; it does NOT reset it. Corrected helper captures each lane
starting counter, computes wrapping full-counter deltas and relative low15
ordinals, then rebases to parent serial space. Integrated only after this
exact evidence; comparator remains byte/field strict. Synthetic integration
uses distinct retained lane counters and validates complete output, state,
FP and escape refusals.37 focused pass. Next: immediate self-serve build
and original replay; no new capture/head rebuild request.

2026-10-13 self-serve e8334107 replay48124:64 recording headers rebased;
serial/live66PASS,175jobs, lane mergePASS. Fullstream comparison advances
to root22 bindings; key580055EB and header162C0 match exactly. Added
bounded binding-kind/identity/length/first-byte diagnostics, comparator
unchanged.37 focused pass. Next: immediate offline rebuild/replay.

2026-10-13 f9cb8862 real replay: root22 Shader binding identities26/26
and388-byte snapshots match in size; first byte difference offsetA4 is
10/00. Descriptor decode maps it to fourth component of a16-byte float
constant (value index2), not pointer identity. Added bounded containing-word
diagnostic to trace the producer; no assumed padding/float tolerance.
37 focused pass. Self-build/replay next.

2026-10-13 CBSkinning root22 evidence: original FE49xx producer writes
xyz only; installed MFX declares scalar+float2 in a four-float allocation.
29198 installed shaders scanned,566 CBSkinning declarations; direct reads
use xyz only, but indirect constant accesses prevent blanket padding proof.
Added bounded v4 private capture shader bank from exact referenced host
objects under D3D lock; v3 remains readable. No comparator relaxation.
37 focused tests pass; format checks cover legacy, oversized and duplicate
shader banks. Next: head-only v4 capture with exact shader tokens; continue
consumer proof and publication boundary while waiting.

2026-10-13 v4 adea3d4b build completed32s (including ddplay), capture
requested from head. Added bounded state-difference categories independent
of first stream refusal, to expose the remaining publication state gate;
no category exempts any byte.37 focused tests pass. Next: replay48124
and trace non-command state while awaiting head-only v4 capture.

2026-10-13 replay3381896e: serial/live66PASS;175 jobs;64 headers rebased.
Independent strict state gate reports19650 command-arena differing bytes,
462 root-context bytes and195 other bytes (first render+33DC9, shared
command cursor). No outer-scheduler differences. Added bounded first24
other-state words to locate remaining producer/state dependencies.
37 focused tests pass. No state exclusions or shader masks introduced.


## HANDOFF to claude-9 (2026-10-13, head board #14487/#14488)

Head requested stop and takeover for quota. I discovered these messages via
`board.py show` at about 18:22 local; repeated inbox calls had returned no
unread messages. No further implementation is planned by codex-11.

Branch/worktree: `task/L172-parallel-shadow`, `E:/ddda-rs-wt/L172`.
Latest implementation commit: `428e3fc0`; this handoff is the next commit.
Repository was clean before this documentation edit. All source changes are
committed. Never push; commit as janne / marcelovjunior55@gmail.com.

### Exact current result

Private gameplay fixture:
`E:/ddda-re/notes/native-L172/replay/shadow-48124-0.ddshadow`
(v3, accepted capture, 16,223,235 bytes).
Latest replay log:
`C:/ddda-targets/L172/real-replay-state-words.log`.

- Original serial replay matches live serial: 66 entries, full ABI/FP and
  journaled memory (except popped stack).
- Private parallel schedule: mode1, 175 jobs, two contiguous lanes.
- Both lanes complete; dependency merge passes.
- 64 recording headers rebased from retained lane counter starts; header
  and sort key comparisons now pass up to the binding refusal below.
- Refusal: root entry22, `bindings`, Shader identity0x26 on both sides,
  encoded length0x184 on both sides, key0x580055EB, header0x162C0.
  Encoded offset0xA4 is a fourth float word: serial0x00E0F410 versus
  parallel0x64E53C00 (parallel residue varies with allocation scheduling).
- Independent state diagnostics, NOT acceptance exemptions: latest replay
  has 0x4B2A differing command-arena bytes (count varies with allocation),
  0x1CE root-context bytes, 0xC3 other bytes, zero outer-scheduler bytes.
  First root difference context+0x24. First other word is renderer+0x33DC8:
  serial0x146000, parallel0x147000. Following bounded other words at
  0x64673324, +8, ... contain relocated command pointers; inspect root
  entry arena before classifying. Nothing is masked or accepted yet.

### Binding hypothesis and evidence

Root22 original binding0x64E53AB0, metadata0x202298D8, 7 indexed values.
Value index2 points to0x64E53960 and supplies16 bytes; first three words
are0x3B400000,0,0. Fourth word was not written by the producer.
Metadata parameter IDs:245,240,1F5,153,152,15B,331 (hex).
MFX IDs are ONE BASED relative to the entry offset table: ID1F5 is
CBSkinning (table entry1F4), not CBJointMatrix.

Read-only installed `nativePC/usershader/BbsShaderPackage.mfx` declares
CBSkinning as4 floats, with fMatrixMapScale (scalar at0) and
fMatrixMapOffset (float2 at1). Original code around FE4960/FE4989 allocates
16-byte-aligned storage via context+24, puts it in context+11A0, and writes
only output[0], output[1], output[2]. FE4903 decompile returns the owning
function. Thus w is old command-arena residue. This is NOT a rounding error.

Private audit source `C:/ddda-targets/L172/shader-audit/src/main.rs` and log
`C:/ddda-targets/L172/shader-audit.log` scanned all installed sa/DX9 shader
packages:29,198 unique shaders;566 CBSkinning declarations. Their DIRECT
constant reads use only xyz, but some shaders have relative constant reads.
CTAB calls the slot CBSkinning__packed0, float4, so reflection alone cannot
prove w unused. Do not blindly mask w. Need exact shader consumer proof,
conservatively accounting for indirect registers and implicit matrix rows.

### V4 capture requested, not yet received

Commit `adea3d4b` adds bounded exact referenced D3D shader bytecode to the
private fixture. It clones immutable token arrays under the D3D object lock
after enumerating stream shader handles (DAA8B0 metadata+10/+14). Missing
host shader object rejects the capture. Version4 is DDSH172 followed by
byte4; it retains the v3 fields, then appends shader count and, for each,
handle, token count and tokens (u32 LE). Limit4096 objects and1M total
words. Reader accepts v3 with an empty bank. Equality remains strict;
shader bank is currently only captured, read, and its count logged.

Head request #14494 includes the ready game binary:
`C:/ddda-targets/r172/play-o2/deps/ddplay.exe` (R1725243c264, adea3d4b).
Capture env (normal head gameplay route):
```
DDRUNTIME_PARALLEL_SHADOW=1
DDRUNTIME_PARALLEL_SHADOW_AFTER_MS=60000
DDRUNTIME_PARALLEL_SHADOW_FIRST=0
DDRUNTIME_PARALLEL_SHADOW_COUNT=1
DDRUNTIME_PARALLEL_SHADOW_DUMP=E:/ddda-re/notes/native-L172/replay-v4
```
The new directory was absent at handoff. Only head runs ddplay/game.
This is capture-only serial execution, NOT TRAIN READY. Existing replay
still accepts v3 and reproduces the old refusal. Bank format tests cover
roundtrip, legacy v3, oversized counts/lengths, duplicate handles/truncation.

### Fast self-serve build/replay commands

Head #14473/#14474/#14477 explicitly authorized the dedicated warm build.
Do not edit R172 manually. The script merges the committed task branch
into E:/ddda-rs-wt/R172 and builds C:/ddda-targets/r172 (play-o2).
It does NOT merge latest fps-line on each invocation. No heavy queue is
needed for this authorized incremental loop. NEVER build the32 shards
elsewhere. Latest offline build5s; --ddplay build32s.

PowerShell, from this task worktree:
```powershell
$env:CARGO_BUILD_JOBS='4'
& 'C:/Program Files/Git/usr/bin/bash.exe' E:/ddda-re/tools/l172_replay_build.sh *> C:/ddda-targets/L172/replay-build.log
$env:DDRUNTIME_PARALLEL_SHADOW='1'
& C:/ddda-targets/r172/play-o2/examples/shadow_replay.exe E:/ddda-re/notes/native-L172/replay/shadow-48124-0.ddshadow *> C:/ddda-targets/L172/replay.log
$replayExit=$LASTEXITCODE
Get-Content C:/ddda-targets/L172/replay.log
```
Latest replay exe is R17202e10a58, includes428e3fc0. A log-read after an
exe overwrites PowerShell command exit status: inspect saved replayExit.
Use script `--ddplay` only when a fresh capture binary is required; head
still owns all game runs. Rebuilding just the example leaves ddplay alone.

Light tests (no GPU/game):
```powershell
$env:CARGO_TARGET_DIR='C:/ddda-targets/L172'
$env:CARGO_BUILD_JOBS='4'
$env:CARGO_INCREMENTAL='0'
cargo test -p ddruntime --no-default-features --features parallel-shadow shadow --lib
cargo test -p ddruntime --no-default-features -- --test-threads=1
```
Latest focused:37 passed/0 failed, log state-word-tests.log. Last full
light gate at8750e8bf:290 passed/0 failed/7 ignored; not rerun since then.
Do not claim later full-suite results. Respect head pause and GPU rules.

### Implementation map / gotchas

- parallel_shadow.rs: immutable pages, tracked LaneMemory transactions,
  private allocation/copy, contiguous lane executor, strict dependency
  merge. `publish` is tested but UNWIRED; preflight is NOT a lock.
- shadow_replay.rs: serial live journal, original raw-body private resolver,
  explicit captured-IAT CS/XADD imports, private manager and scheduler,
  shared transaction-local command cursor, original capacity check scope,
  stream/FP/state comparisons. ReplayResults currently discard LaneMemory
  read sets into write maps; production validation will need dependencies.
- shadow_replay_file.rs: bounded v3/v4 format, full CPU images, shader bank.
- shadow_replay_sequences.rs: original FEF830/FFC8E6 low15 recording order;
  FED200 RETAINS counters. Validate full unique relative ordinal domain,
  then rebase candidate headers into serial order; all other bits exact.
  Unsupported associated kinds4/5 reject; nested kind6 supported.
- Original DA7D70 InterlockedExchangeAdd at captured IAT139D0A8 uses one
  transaction-local mutex cursor. Ordinary lane access to that cursor
  rejects. Original DA7DB0 capacity check alone gets scoped synchronized
  u32 reads; raw body still runs and all other accesses stay tracked.
- FED200 needs the whole current render command arena seeded, not just
  serial-touched pages. New private manager prevents nested root aliasing.
  Private scheduler prevents clobbering the already-active outer queue.
- Consumed output pointing into private manager/scheduler/secondary arena
  rejects through PublishedView. Popped callback stacks are private.
- Unknown imports fail with target/returnPC; never fall through to real
  host services. Earlier rejected44100 capture must never become proof.
- The original final-memory gate is deliberately over-strict for relocated
  arena/context state. Current new diagnostics do not weaken it.
- The live guest lock does NOT exclude translated worker accesses in
  DDRUNTIME_PARALLEL mode. Head question #14491 asks for an existing
  exclusive renderer/worker boundary; no answer received. Do not wire
  preflight+stores and claim it is atomic without proving that boundary.
- Native source modules s9 (FED5D0/EA5B50) are owned by another head agent;
  read only. Stay within claimed runtime shadow files.

### Remaining work / acceptance

1. Get v4 capture, establish exact consumer-used components of the failing
   shader constant. Any normalization must be source-backed, conservative
   for indirect accesses, and regression-tested (including changed used
   components refusing). Or find an exact-byte candidate transformation.
2. Continue real offline stream equality to completion, closing every
   missing dependency/import. Classify relocation-only memory differences
   using original producer/consumer semantics; resource changes remain
   strict. Do not accept first matching subset or silently discard state.
3. Preserve validated candidate output and dependencies. Establish an
   exclusive production boundary, publish only proven non-private output,
   retain serial fallback before any partial stores, and add same-run
   serial==parallel check mode. No publication path currently runs.
4. Build and ask head for exact same-run correctness then live A/B with
   DDRUNTIME_PARALLEL_SHADOW=1; report FPS and DB7620 durations. Only then
   mark TRAIN READY. Performance is entirely unproven; tracked diagnostic
   replay is not the intended fast production implementation.

Implemented/tested diagnostics and synthetic replay are NOT original
serial/parallel equality, live publication, image equality, or speedup.

## claude-9 takeover (2026-10-13)

Replay fixture: v4 capture `shadow-24392-0.ddshadow` (79 entries, 174
jobs, 2 lanes, 14 captured shaders). Result: **the normalized serial and
parallel streams match on all 79 entries.** The next refusal is the
final-memory gate (below). Every rule below is exact or source-backed;
each accepted category is counted in the replay log.

### Comparison rules added

1. **Targets and Fixed bindings compare by content**
   (mtcore::shadow_lanes). 0x00DA6FE0 returns for the current pointer, and
   for any other pointer compares each view, the depth surface, the
   viewport and the scissor with the context's cached values, calling the
   device only on a change. 0x00DAA5F0 compares its blend, depth and raster
   objects and the factor, stencil reference and flip bit the same way.
   Equal bytes therefore apply identically, and lanes' local copies of
   serial-reused bindings are equivalent. Shader and stream bindings keep
   pointer identity. Covered by the lane tests (12 pass).
2. **Normalized replays.** `sSceneExt::vf16` 0x0049B1A0 builds its
   0x190-byte light constant block in four stack arrays (+0x20, +0xA0,
   +0x130, +0x1B0 of its frame) and copies them whole, filling only the
   found lights' entries. crates/mtdraw/src/lights.rs (L176) documents
   this. The rest is stack leftovers, undefined in the original and
   different on a lane's stack. That is where the `fWindParams`/`fWinds`
   tails and the c4 context-pointer word came from. The compared serial and
   parallel replays zero those arrays on entry (`NORMALIZE`, resolver
   wrapper). The plain serial replay is still checked against the live run
   (it passes).
3. **Residue-dependent bytes (definedness runs).** Two extra normalized
   serial replays fill the residue sources with 0x00 and 0xFF:
   - the command arena's free space (render +0x33DC0[ring] + cursor
     +0x33DC8 .. capacity +0x33DCC);
   - the 256 KiB of stack below ESP (`DDRUNTIME_PARALLEL_SHADOW_POISON_STACK`
     changes the depth).

   A binding byte that these runs change depends on memory the original
   never initialized, and only such bytes may differ. A misclassification
   can only refuse. Disable with `DDRUNTIME_PARALLEL_SHADOW_NO_POISON`.
   On this capture: 161 bytes, the CBSkinning w words among them.
4. Kept, but unused on this capture:
   - **unread constant components:** the bound shader, and every later draw
     of the stream until the register is uploaded again, provably never
     reads it (d3d9shader `float_reads`, conservative and tested;
     `float_reads_in_bounds` assumes relative indexing stays in its CTAB
     array, and is counted separately);
   - **relocated context pointer:** a word pointing into the root context
     serially, and at the same offset of a lane context in parallel.

### Evidence and diagnostics (debug switches, replay only)

- `DDRUNTIME_PARALLEL_SHADOW_WATCH=hex,..` (plus `_WATCH_READS`): report
  writes (and reads) of those words, with the recent resolver entries.
- `DDRUNTIME_PARALLEL_SHADOW_TRACE=function,offset`: log ECX and a field
  on each entry to a function.
- `DDRUNTIME_PARALLEL_SHADOW_SHADER_DIR`: write the captured shaders'
  disassembly.
- `DDRUNTIME_PARALLEL_SHADOW_ZERO_ARENA`: the zeroed-arena experiment.

The CBSkinning residue is the w word of `fMatrixMapScale/Offset`
(0x00FE4960's caller writes xyz only). The wind arrays come from vf16
(above). The shader 0x202C5550 reads `c1[a0.x]` / `c9[a0.x]` with a0 from
`floor(v * fWindNum)`, so static proofs of their tails are impossible:
negative starts reach c2. Normalization is used for that reason.

### Next

1. Final-memory gate. On this capture:
   - the command arena differs in 0x40AB bytes: the same contents at lane
     addresses;
   - the root context in 0x1CD bytes, from context+0x24 on;
   - other state in 0xD2 bytes: the renderer arena cursor (+0x33DC8,
     serial 0x155000 vs parallel 0x156000, page-granular lane refills) and
     relocated command pointers at 0x64473054...

   Classify each by its consumers, never by masking.
2. Publication boundary, the same-run check and live A/B, as the handoff
   above says.

### Offline proof PASSES (claude-9, head decisions #14516)

`serial/parallel proof passed` on both captures:

| Capture | Entries | Undefined stream bytes |
|---|---|---|
| v4 `shadow-24392-0` | 79 | 161 |
| v3 `shadow-48124-0` | 66 | 104 |

Acceptance = stream equality modulo undefined bytes, plus ABI/FP, plus
final memory within a closed set of region classes. Every class is
counted on every run; anything outside it still refuses.

**Definedness.** Four extra normalized runs: serial 0x00/0xFF, then
parallel 0x00/0xFF. They poison the command arena's free space and the
256 KiB of stack below ESP; the parallel runs also fill fresh private
storage (`parallel_shadow::PRIVATE_FILL`). A differing stream byte is
accepted only if it is undefined in BOTH executions, that is, its own
poisoned runs change it. A refused or misshapen poisoned run classifies
nothing.

**Final-memory region classes** (shadow_replay.rs, final gate):

| Class | Rule | v4 bytes | v3 bytes |
|---|---|---|---|
| Root context recording state | Differs only where the parallel run kept the input value. Includes the free part of the root context's own arena block (its +0x24 cursor to +0x28 end). DB63E0's parallel branch records into lane contexts; FED3B0 merges only their command arrays (compared through the stream). | 2374 | 2269 |
| Root-entry record pointers | The +4 of each 8-byte root entry: records at other arena addresses, followed and found equal by the stream comparison. | 342 | 292 |
| Fresh command-arena bytes | At or above the input cursor (render +0x33DC0[ring] + cursor); new allocations reached only through the stream. Below the input cursor stays exact. | 26337 | 23474 |
| Renderer arena cursor (render +0x33DC8) | Both runs advanced it and the parallel value stays within capacity; lanes refill whole pages (+0x1000 here). | 2 | 2 |
| Lane state-stack slots above the live top | sPrimitive ([0x018D1848]) has one state object per lane (+0x30 + 4 * lane, lane < +0xD0). vf11 0x00E61010 pushes onto every lane's object in parallel mode and onto the running one serially, through 0x0111D610 (12-byte slots from +0xC, count +0x54) and 0x01192260 on [object+4] (16-byte slots from +4, count +0x64), six deep. Slots at or above a final count that is equal in both runs are dead. | 39 | 49 |

**x87.** The FNSAVE environment (bytes 0..28: control, status with TOP,
tag word, pointers) must be equal. Register contents may differ only in
registers tagged empty, which no instruction can read.

Light gates: the focused `shadow` lib tests (37), the mtcore lane tests
(12) and the d3d9shader tests pass.

**Live validation-only mode.** It is the existing proof runner
(`DDRUNTIME_PARALLEL_SHADOW=1`). The live traversal runs serially and
publishes. Its captured input is then replayed privately (plain serial,
normalized serial, parallel, four definedness runs) and compared in the
same run. The log reads `parallel-shadow sample=N audit=Ok(entries)` or
`audit=Err(...)` with `publish=serial`, plus the per-class counts above.

### Live validation-only check PASSES (head run, #14544)

The run used binary R172 fb8320cd (, AFTER_MS
60000, COUNT 3) and serial publication; the full log is
E:/ddda-re/notes/runs/run_live172.log. All three samples report
:

| Sample | Entries | Undefined | Root left untouched | Relocated pointers | Fresh arena | Cursor | Dead slots |
|---|---|---|---|---|---|---|---|
| 0 | 278 | 395 | 2476 | 1302 | 96860 | 2 | 42 |
| 1 | 78 | 215 | 364 | 344 | 43661 | 2 | 42 |
| 2 | 66 | 110 | 2001 | 302 | 23806 | 2 | 34 |

These are proofs only: nothing is published from the parallel run, and
there is no speed-up yet.

## P2d: semantic ports of the shadow job's top levels (s1, 2026-10-14)

Module `mtcore::shadow_top` (branch `task/L172-p2d`), ABI-level ports in the
style of the L184 semantic ports (callee-saved registers, ESP, every guest
write above the returned stack, each guest call made as the original makes
it, through `shadow_top::call_guest`, which enters a registered native port
directly and otherwise `call_ext`):

| address | port | proof |
|---|---|---|
| 0x00EFD240 | `setup_pass` | the 96 original L169 vectors (`native-L169/shadow_pass.json`), 9/10 mutations (the tenth is equivalent: bit 0 of the shader tag is masked) |
| 0x00EFDC70, 0x00EFEF80 (jmp thunk) | `draw_cascades` | 2000 recorded cases (`record_shadow_cascades.py`), x87 balance checked, 8/8 mutations |
| 0x00EE2C90 | `draw_shadow` | 2000 recorded cases (`record_shadow_wrappers.py draw`) |
| 0x00DB7620 | `run_unit_job` | 2000 recorded cases (`record_shadow_wrappers.py job`), ends in a tail jump to 0x00BFC7D0 |

Registration is not in this branch (ddports): `0x00EFD240 -> setup_pass`
(replacing the instruction-level `shadow_pass::setup`), `0x00EFDC70` and
`0x00EFEF80 -> draw_cascades`, `0x00EE2C90 -> draw_shadow`,
`0x00DB7620 -> run_unit_job`. Known limit of the setup_pass vectors: the
pass index `k` (stored in `ctx[0x180]` only for the duration of one callee)
is 0 in all recorded cases, so only a callee could observe it. `setup_pass`
vs the instruction-level port on the same vectors: about 720 -> 480..730 ns
per call (the replayed callees dominate).

## Live publication: S1/S2 (claude-9, 2026-10-13)

State for a new head: branch `task/L172-parallel-shadow` (worktree
`E:/ddda-rs-wt/L172`), build with `bash E:/ddda-re/tools/l172_replay_build.sh
--ddplay` (merges the branch into `E:/ddda-rs-wt/R172`; check that it prints
`built ...`; a merge conflict there exits silently), live runs with
`C:/ddda-targets/L172/runs/s1.sh TAG [env...]` inside
`HEAVY_BIG=1 python E:/ddda-re/tools/heavy.py claude-9 -- bash ...`
(baseline C+X env of `E:/ddda-re/tools/headruns/README.txt`, heavy-window
fps and the publish counters land in `runs/measure_TAG.out`).

Env: `DDRUNTIME_PARALLEL_SHADOW=publish`, `_CHECK=N` (proof every Nth
traversal and the first, default 1000), `_AFTER_MS`, `_WAIT_US` (default
2000), `_HELPERS=0` (S1: every lane on the window's owner), `_DUMP=dir`
(every proof's capture, for offline replay).

Code: `crates/ddruntime/src/shadow_publish.rs` (window, private copies,
DBE560 dispatch, lanes as tasks), `waits.rs` (s6: parked registry, RAII
window, violation; s8: `help()` running tasks on the parked participants),
`shadow_replay.rs` (`traverse` routes publish decisions; the proof).

Mechanics: the traversal waits (bounded) until every other job participant
is parked (workers on start events, main on the outer join's done events),
opens the window (a participant that wakes is held at the gate and counted),
sets the top-level globals (0x018D0BB0/0x018D0404 = 0, 0x018D0B28 = private
scheduler copy, private manager copy), runs the original DB63E0; its DBE560
on the private scheduler partitions the batch into 2 lanes (same partition
as the proof) and runs each lane as a `help()` task: the parked workers take
them on their own TEB/stack with the owner's registers and x87 image. Then
the counters are rebased into serial order and everything is restored.

Guard: nothing publishes before a proof passes. A proof is refuted (serial
!= parallel stream, ABI, x87, final memory outside the classes) -> serial
for the rest of the run. Inconclusive (the capture or the serial replay
cannot reproduce the live traversal: `replay rejected`, `incomplete
capture`, `original serial replay ... differs from live serial`, ...) ->
counted, next proof 16 traversals later.

Proof fix: the replay runs the raw lifted bodies, the live run the ports, so
the capture can lack pages only the lifted bodies read (FUN_00ea23f0's
switch table in .text at 0xEA3110.., the IAT page 0x139D000). The proof adds
read-only exe image pages from the live image on demand and repeats.

| run | binary | result |
|---|---|---|
| s1a/s1b | S1 | first proof `MissingPage(0xEA3120)` -> off (s1b ran a stale binary: R172 merge conflict) |
| s1c | S1 + image pages | proofs 0..1200 pass (7x `audit=Ok(6)`), 20 windows / 418 units, woke 0, no crash; 1373 of ~1400 not exclusive; proof 1400 `original serial replay stream differs from live serial` -> off; heavy-window 29.67 fps |
| s2a | S2 + wait + inconclusive class, CHECK=200, dumps | publication works: 2195 windows, 118k units, 1431 lanes on parked workers, 0 task errors, woke 0; conclusive proofs all pass (Ok 58..78); 6.6 fps: every proof stalls the outer join for seconds |
| base2 | same binary, no publish | 32.73 |
| s2b / s1d | S2 / S1, CHECK=100000 | 20.59 / 21.00: x87 park/acquire 15-20% on every thread (a fresh x87 state per unit, acquired and leaked into the spill tables) |
| s2c | S2 + one x87 state per lane (b5ac50fd) | 30.33; 8191 windows, 716k units, 8699 helped lanes; costs left: the exclusivity wait (7 s per run) and the per-window copies (1.7 MB manager, 115 KB scheduler) |

Soundness gap (s2a): the inconclusive proofs (`original serial replay
stream differs from live serial`) all fall on one traversal kind (samples
N*200; the +17 retries pass), so that kind was published unproven.
Publication is now per recording context (the traversal's argument): a
context publishes only after a proof passed on it; unproven contexts get
proofs with a doubling backoff. The failing kind's captures are in
`C:/ddda-targets/L172/dumps` (private) for the serial-replay gap.

### Proofs off, fps (claude-9, head #14617 user order)

Guard proofs are opt-in (`DDRUNTIME_PARALLEL_SHADOW_CHECK=N`); default
publish mode captures nothing, never falls back (window errors counted),
waits 0 us for exclusivity (measured best: WAIT_US 0 33.73, 500 32.96, 2000
30.33). Partial manager copy (header + lanes 0..1) verified offline on the
two reference captures and dump 1417 including the definedness runs.

| run (binary 3d2999f8, C+X env) | heavy-window fps |
|---|---|
| s3a publish | 31.77 (2140 windows of 10752 traversals, 63k units, 3178 helped lanes, 0 errors) |
| base4 | 32.72 |
| s3b publish | 33.82 |

No measurable gain: only the light traversals (~30 units) find every
participant parked; the heavy ones overlap the outer batch and stay serial.
Reaching them needs the parallel branch while the outer batch runs (a
private scheduler view per thread instead of the global redirect, or the
lanes queued into the outer batch). Playtest launcher:
`E:/ddda-re/play/playtest_parallel.bat` (binary copy
`C:/ddda-targets/r172/runs/playtest`).

Open: the inconclusive kind (`original serial replay stream differs from
live serial`) is vertex constant c4.xyz at one entry (residue bytes at
0x64859E30 in dump 1400): the live-vs-serial check lacks the unread-constant
rule the serial/parallel check has.

Next action: the head's decision on the non-exclusive design above.

## View-mode root cause and why there is no safe speedup (shadow agent, 2026-10-14)

**Root cause of the view-mode crash (static, from the decompiles; the
crashing frames are consistent, not run-proven with a fix).** The crash is
on the *main* (or another outer-batch) thread, in a plain outer unit
(`DBE560 -> DBD8D0 -> DB7620 -> ... 0x00E75EA0/0x01253200`), reading
`null+0x24`: an outer job's sPrimitive lane state object was corrupted by the
shadow traversal. `DB63E0`'s parallel branch calls `0x0107FF00`/`0x0107FF30`
(sPrimitive `vf11` 0x00E61010 / `vf12` 0x00E68390) with the parallel flag:
- `vf11` pushes a list level (0x0111D610) on **every** lane object
  `[0x018D1848]+0x30+4*i`, `i < [sp+0xD0]`;
- `vf12` sums, merges (its own scratch at `sp+0x58..`) and pops (0x0111D650)
  **all 8** lane objects.
The private manager's lanes get lane id 0,1 (`FED200` stores the loop index at
context+0x58), and the units of every unit kind index the same global objects
by `ctx+0x58`. Outer jobs running on participants 0 and 1 use those very
objects, so begin/merge/end of the traversal consumes and pops their lists.
Exclusive mode is safe because nothing else runs. A correct non-exclusive
design needs (1) lane ids = ids of *idle* participants only (private
contexts' +0x58 patched after FED200), (2) `vf11`/`vf12` restricted to that
lane set (a masked port of vf12's 8 slot reads, scratch use and final pop
loop; vf12's scratch in `this` also needs a lock between two concurrent
windows), (3) `waits::help` is one-at-a-time (`assert !active`).

**Why it was not built (numbers, DDRUNTIME_PARALLEL_SHADOW_EXCLUSIVE=stats
and =skip, benchmark save, 2 runs each, fps = mean stage-probe over t=46-84 s
WITHOUT the >45 filter because the scene is fast now):**
- `skip` (traversal removed entirely, an upper bound) 64.5 / 62.3 vs stats
  (serial, timing only) 57.5 / 57.2: the *whole* traversal is worth at most
  ~10% fps; a 2-way parallel version could recover about a third of that
  (~3%), below the 7 fps run-to-run noise.
- Idle participants at traversal start (stats run): 0 parked for 9865 of
  ~14800 traversals and 8.4 s of 12.1 s total traversal time (5.4 s of it in
  the >1 ms ones); lanes bound to idle participants would only help the
  other third.  (A late-joining design, idle workers claiming lane ids when
  they park, could capture more but needs the masked vf11/vf12 above plus
  dynamic lane binding.)
- EE2C90 total: n=35923, 16.3 s per run (~2.2 ms/frame summed over threads).
Runs made before 01:45 on the Cassardis-gate save (st1, st2, off1, off2,
skip1) are discarded; they only showed the same shape (skip +6 fps over 42).

Debug switches added: `DDRUNTIME_PARALLEL_SHADOW_EXCLUSIVE=stats` (serial,
logs parked-workers histogram and EE2C90 time) and `=skip` (no traversal,
fps ceiling; breaks rendering of shadows).
