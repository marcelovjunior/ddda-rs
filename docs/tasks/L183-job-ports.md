# L183: semantic ports of the main thread's game work

Owner: claude-9. Branch `task/L183-job-ports`, from `task/train5`
(a43d2366). Assigned by the head (L183, after train 8): the frame is
bound by the main thread's own job work (~13 ms of a ~31 ms frame), so
cut it with semantic (ABI-level) ports, biggest first, measuring each
port's per-call gain. Batches of 2-3 ports, each posted TRAIN READY.

## Where the time is (combo39 profile, stacks_c39on.json)

702 samples a thread over 30 s, so one main-thread sample is about
0.044 ms per frame. The main thread's busiest unported functions
(innermost guest frame) are flat:

| Function | Main-thread samples | ≈ ms/frame | What it is |
|---|---|---|---|
| 0x00DBE560 | 64 | (waits) | the job join: SetEvent/WaitForMultipleObjects (L172's subject) |
| 0x00DD42C0 | 42 | 1.8 | dispatch_event (the head ported it in train5) |
| 0x0049E920 | 18 | 0.8 | the nearest object of a type (batch 1) |
| 0x0109DAC0 | 12 (+14 on job threads) | 0.5 | an AABB-tree overlap walk with guest callbacks |
| 0x004172C0 | 9 | 0.4 | vf06 0x00417120's table scan (batch 1) |
| 0x00D1B840 | 8 (+6 on job threads) | 0.35 | a segment against a box (batch 1) |
| 0x0109D480 | 6 (+8) | 0.3 | the AABB-tree walk's other form |

Under the existing ports, the samples of `commit_primitive_state` and
`shader_lookup` are in Rust code, not in guest callees.
`intern_dependency_keys_with` takes 43 samples on its own, in a
recursion; the profile's ddplay.exe symbols do not resolve it. Under
`dispatch_geometry`, the guest callees are 0x0109DAC0, 0x00D53730,
0x0109D480 and 0x00D1B840.

## Batch 1 (`DDRUNTIME_SCAN=1`), crates/mtcore/src/game_scan.rs

Each is translated from the original (DDDA.exe 2017-12-13 Steam) and
compared at the ABI level against Unicorn runs of the original
(research/native/record_l183.py, vectors private in
E:/ddda-re/notes/native-L183). The test checks:

- EAX, the callee-saved registers, ESP and every byte outside the popped
  stack scratch;
- every register and the pushed words at each guest call, with the
  callee's recorded writes replayed;
- that no byte is read that the original does not read.

Every conditional branch of the three functions is covered, except the
listed impossible ones.

| Port | Cases | Lifted ns/call | Port ns/call | Gain |
|---|---|---|---|---|
| 0x00D1B840 segment vs box | 1,500 | 375 | 34 | 11x |
| 0x0049E920 nearest object (≤256 candidates) | 1,394 (12,669 guest calls) | 2,427 | 165 | 14.7x |
| 0x0049E920, more than 256 candidates | 44 of the above | 832,000 | 109,000 | 7.7x |
| 0x004172C0 scan | 1,500 (44,323 guest calls) | 6,900-10,400 | 1,800-3,300 | 3-4x |

The bench is private (C:/ddda-targets/L183/bench): the lifted bodies
against the ports on the recorded vectors, in a paged guest memory,
with the guest calls stubbed. Timings are noisy with other agents
running.

Mutation check: 14 mutations of game_scan.rs. 13 are caught. The
survivor, `hi_bound >= b` in the slab, is equivalent: on equality it only
changes the sign of a zero `hi`, which is only ever compared.

Details the ports keep:

- 0x00D1B840 leaves the LAHF flags of its last `UCOMISS ux, 0` in AH on a
  miss; EAX is the segment pointer with that AH.
- The originals read their stack arguments only where they use them
  (`t_out`, `this`, the search parameters), and so do the ports.
- 0x0049E920 types each object through three `vf04` calls. A getter that
  is `mov eax, imm32; ret` is read instead of called (same result, no
  other effect); any other getter is called as the original calls it,
  with its registers.
- With more than 256 candidates, the original's bubble sort runs past its
  array into its own frame. The port does that sort on the guest stack,
  then pops the registers from the frame as the original does. Cases
  where the overrun moves the return address crash the original; the
  recorder skips them and the port asserts.
- 0x004172C0 keeps the keyed table's keys it has read (a prefix, as the
  original reads them) until a call to 0x005B4800, which may change the
  table. That call takes its state in EAX and `ret 8`, and is made with
  all of the original's registers.

Recorder notes:

- 0x004172C0 dereferences `[player+0x2E64]` unchecked: every player has a
  holder.
- Unicorn keeps translations of code rewritten between runs. The getter
  page's cache is dropped each run.

## Batch 2 (also `DDRUNTIME_SCAN=1`), crates/mtcore/src/tree_query.rs

The two box queries of the collision AABB tree that 0x010A8570 calls.
They are the leaves under `dispatch_geometry` and the main thread's
0x01166C00 / 0x00472AD0 subtrees.

- **0x0109D480** tests an axis-aligned box per lane.
- **0x0109DAC0** tests a segment (its ends at +0x10 and +0x20) against
  the children's boxes. It uses the face axes and the segment-edge cross
  axes, with a per-lane slack.

Both share one walk:

- The record stack is global memory, one buffer per slot and nesting
  depth, so its records are compared.
- Each passing leaf gets a guest callback `fn(arg, index, 0)` (`ret 0xc`,
  ctx in ECX); the walk ends on the first nonzero result.
- A slot from 0x19 up falls back to a guest call.

| Port | Cases | Lifted ns/call | Port ns/call | Gain |
|---|---|---|---|---|
| 0x0109D480 | 1,500 (242 guest calls) | 331 | 98 | 3.4x |
| 0x0109DAC0 | 3,000 (463 guest calls) | 1,781 | 294 | 6x |

The bench figures are rough: its leaf-callback stub restored ESP wrongly
in that run, so callback-heavy cases are approximate.

Tests:

- Every branch is covered.
- At guest calls, ECX, ESP, the pushed words and memory must match (an
  MSVC callee depends on nothing else). The test file's scratch window is
  8 KiB, because 0x0109DAC0's own frame is 0x1474 bytes.
- 9 mutations: 7 caught. The 2 survivors are equivalent:
  - `result |=` vs `=`: the result is 0 before every call.
  - the node-bit byte mask `0xff` vs `0x0f`: the bits sit in the low
    nibble.

Recorder notes:

- 0x0109DAC0 spans four code pages, runs `__alloca_probe` (0x012F2280),
  and calls 0x00422F80, an x87 fabs that quiets a signalling NaN.
- Its centre/slack constants (0x018D0630/0x018D0660) are zero in the
  image and set at run time. The recorder uses 0.5 and small slacks,
  sometimes per lane.
- The record-buffer table runs into the depth counters after 50 entries,
  so depths are 0 or 1.

## Results

- Batch 1: tested in isolation (above). Not run in game. Needs a train
  run with `DDRUNTIME_SCAN=1`.
- Batch 2: tested in isolation (above). Not run in game. The head moved
  claude-9 to L172 at this point. Next candidates if L183 resumes:
  - 0x00D53730 (box clamp, x87 return);
  - the Rust cost in `intern_dependency_keys_with` (43 job-thread
    samples);
  - 0x00FF2EE0, 0x00FEB1F0, 0x00FE66A0.
