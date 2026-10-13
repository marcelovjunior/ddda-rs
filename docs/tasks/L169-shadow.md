# L169: shadow traversal and pass setup

Owner: codex-9. Branch: `task/L169-shadow`, based on `task/fps-line`
at `c0a9a793`. Assignment: head #13628 and codex-11 family split #13641.
Scope: whole DB63E0/EFD240 ports, separate opt-ins, isolated original/lifted
oracles, and the evidence needed before considering traversal splitting.
No game, GPU, shard build, scheduler toggle, or installation modification.

## Implementation

- `DDRUNTIME_SHADOW_TRAVERSAL=1`: `sUnit::vf16`, DB63E0, in
  `mtcore::shadow_traversal`. Structured loops replace lifted block dispatch;
  the existing `Pages` accessors compile inside the optimized port. Both
  original serial and worker-queue branches remain, with identical filters,
  callback order, queue writes, guest spills and call boundaries.
- `DDRUNTIME_SHADOW_PASS=1`: EFD240 in `mtcore::shadow_pass`. Preserves
  aligned guest stack, three command-record variants, allocation/refill,
  shader lookup through guest data, virtual calls, optional secondary pass,
  and recording restoration. The original x87 copies and SSE comparison
  remain hardware operations; no float rewrite or buffer batching.

Both default off and register independently in `ddports`. No new memory
policy, worker count or generator option. Callee dispatch retains existing
overrides, including FED8F0's cross-shard call path.

## Evidence and exactness

Read-only Ghidra decompilation/disassembly of DB63E0 and EFD240 was checked
against the isolated lifted bodies. Private recorders execute original exe
instructions in Unicorn, with randomized synthetic objects/registers and
explicit synthetic callee effects. These are boundary-contract tests,
**not captured gameplay inputs or execution of the real external callees**.

| Gate | DB63E0 | EFD240 |
|---|---:|---:|
| Original returns, each checked against lift and port | 324 | 96 |
| Visited lifted basic blocks | 38/38 | 25/25 |
| Injected byte access/fault pairs against isolated lift | 8,462 | 7,198 |
| Original replay on production GuestMemory | serial + atomic | serial + atomic |
| Isolated tests | 4 pass | 4 pass |

Original checks include all integer registers, defined flags, XMM, MXCSR,
x87 image, every written byte (including spills), and every external-call
entry. Fault checks compare ordered byte accesses, writes, machine state
and callback sequence. EFD240 additionally compares XMM/MXCSR/x87 on faults.
The test normalizes reserved FSAVE words on restoring synthetic x87 state;
architectural control/status/tags and register payloads are compared.

Traversal covers empty lists, all rejection stages, optional group masks,
both serial selectors, queue path, 0/9/10/31/32/255 shift inputs, and callback
mutation of a later node's visibility. Setup covers recording on/off,
shader identity match/mismatch, refill/no refill, null output, six descriptor
counts including 65535, stack alignments 0/1/4/15, all optional-pass branches,
signed zero, finite signs, infinity and quiet NaN. Signaling NaN and arbitrary
callee behavior are not claimed covered; broader composed integration is
still the head's gate.

## CPU measurements

Isolated lifted bodies use opt1, port/base/production GuestMemory opt3,
atomic accesses enabled. Seven alternating pairs per setting, 50,000 calls
per sample; explicit synthetic callees include dispatch but no real model
or material work. The preemption seam fails closed if unexpectedly reached.

| Candidate | Settings | Faster pairs | Median speedup across settings |
|---|---:|---:|---:|
| DB63E0 | 12 | 82/84 | 1.22–1.89x |
| EFD240 | 24 | 151/168 | 1.07–1.28x |

Shared-host outliers remain; individual pairs can regress. These are
synthetic function costs, **not whole-callback or FPS gains**. Earlier short
samples were noisier; the table uses `bench-final.log` and
`pass-bench-final.log` only.

The raw c26vk stacks contain 314 DB63E0-inclusive samples across seven
threads. Their nearest visible lifted owners include DBE560 124, FEE320 22,
E12A20 17 and EA5B50 nine. Counts include nested waits and incomplete unwind
ownership; they do not establish DB63E0/EFD240 self time or predict a frame
gain. Codex-11 owns corrected c31 critical-family attribution.

## Can the traversal be split?

Not by enabling the existing queue inside the callback. DB63E0 tests global
018D0BB0 and scheduler +98 before choosing serial. DBE560 sets the global
nesting guard around its jobs (codex-11 #13641); this critical shadow call
is already a job. Reusing the singleton queue would introduce nested use
of the same job records and joins.

The original outer parallel path is evidence of a lane protocol, not proof
of independent nested work. DB63E0 calls FED200 to initialize context lanes,
publishes eight lane pointers at scheduler +1BDD0 through +1BEB0, invokes
107FF00 with mode 1, enqueues DB7620 entries, joins, then calls 107FF30 and
FED3B0. The serial path calls 107FF00/107FF30 with zero lanes. These wrappers
dispatch methods +2C/+30 on global 018D1848; their concrete implementation
must be included in any ownership proof.

FED3B0 merges lane command arrays in lane-index order (stride 0x33C84),
copies deferred command ranges, rewrites their pointers, and appends their
records to the destination. Therefore worker completion order alone is not
an acceptable merge rule. EFD240 also changes shared recording fields and
resource frame stamps. The traversal preloads each node's next pointer but
rereads later masks and group state after callbacks; snapshotting the full
eligible list can change observable results if callbacks mutate it.

A possible future experiment would need a separate nested job namespace,
independent recording contexts, proof of callback read/write independence,
and a deterministic merge matching original command/state order. Contiguous
list partitions may help define that order but are **not yet justified**.
No splitting or reordering is implemented. Next: head measures the two
independent ports, while the real-input corpus and FEE320/EA5B50 work locate
remaining self cost; only then capture callback memory ownership for a
nested-context design.

## Reproduction and private artifacts

Recorders use research Python through `E:/ddda-re/tools/heavy.py codex-9`:

```
research/native/record_shadow_traversal.py
research/native/record_shadow_pass.py
```

No shard build is involved in either isolated check:

```
python scripts/check_shadow_traversal.py --bench
python scripts/check_shadow_traversal.py --pass-setup --bench
```

Both commands use `C:/ddda-targets/L169`, four jobs, incremental off.
Generated extracted lift, original bytes and vectors stay private under
`E:/ddda-re/notes/native-L169/`. Compiler: rustc 1.99.0 (b940084d7).

| Private artifact | SHA-256 |
|---|---|
| DB63E0 original code range | `64a39341904d612cccdf598d14b5ca74625eadc10793239d53e1be7ee8587e6b` |
| EFD240 original code range | `9bfe545455fd071eeb98a6cfd6f2106f353b3b371cad6cd7239e0c7b4c189b49` |
| shadow_traversal.json | `d6d44a648230e50a2ee655a90c900e0d1465192c1b584fa15de0b5b260be97a5` |
| shadow_pass.json | `b4c27c0056ef51ca03a16aaa4127238c462e896b60820ce381188ccde8654c4c` |
| bench-final.log | `b1a0dc87a8a409f7b449c965c0eacd106bb56bca2fc1c18454c03108fd6c8f46` |
| pass-bench-final.log | `89ab4451489699269756434623b94f087ed1f5e79a075906b54ea1586a7227c0` |

## Results

Implemented and isolated-tested both complete functions. Runtime light:
**255 passed, zero failures, seven ignored**. Required-vector mtcore/ddports
suite with both switches enabled: **20 passed, zero failures, nine ignored**.
Eight isolated tests passed. No original-game image comparison or FPS
measurement by this agent. Head must run the combined native gate and
separate traversal0/1, pass0/1 gameplay comparisons with other settings fixed;
retain opt-in status until those are accepted.
