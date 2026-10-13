# L150: mapped upload pages

Owner: codex-11. Branch: task/L150-upload-belt, worktree E:/ddda-rs-wt/L150.
Base: task/fps-line, with L147 b633d131 + ff8f9c4d cherry-picked as
3530a567 + 4be80083 (profiler conflict retains the newer bounded resolver).

## Scope

Opt-in `DDGPU_UPLOAD_RING=1`: replace queue staging on uniform/inline
rings, dynamic vertex/index uploads, and image updates with reusable
MAP_WRITE | COPY_SRC pages. No game data, game runs, or lifted builds.
GPU tests require the shared pause to be lifted and a 60-second cap each.

## Ordering and ownership

- Producer fills mapped source regions directly. Constant/inline destination
  offsets are fresh until the existing ring wrap flush; their copies form
  a prefix before all draw chunks. Adjacent compatible ranges coalesce.
- Updates of existing buffers/textures record copies at the command position.
  The existing open-resource hazard flush is retained. New buffers' initial
  copies precede subsequent updates. Row padding follows wgpu's 256-byte
  copy requirement; compressed textures count block rows and volumes count
  slices. Conversion arithmetic is shared with the legacy queue path.
- Pages are unmapped before the complete batch enters L124/L137's finisher.
  The finisher owns the pages until submission and registers work-done there,
  never on the producer before a deferred submit. Completion remaps pages;
  only a successful mapping returns a page to the producer pool. No raw
  pointers or mapped/GPU overlap. Failed maps discard the page.
- Pending texture creation after a readback caller's initial flush is flushed
  before the independent readback encoder. Raw Vulkan chunks follow the same
  prefix and command ordering as wgpu chunks.
- L148 coordination: codex-9 board #12447/#12456/#12463 permits disjoint
  upload/flush/finisher/cache additions. Its directly mapped primary Vulkan
  uniforms take precedence; L150 only stages the ordinary fallback ring.

## Results

**Implemented and tested, opt-in only.** Source commit `575bcba0` on
`task/L150-upload-belt`. Prerequisites also present locally:

- L147 `b633d131` / `ff8f9c4d` -> `3530a567` / `4be80083`.
- L143 `c7f7e7c8` -> `cd880f82`.
- L148 `93d55d1f` / `5f8c3931` -> `b2b91b8f` / `0f20cf52`.

The head can pick `575bcba0` onto the equivalent integrated prerequisites;
there is no need to pick duplicate prerequisite patches. Source changes
contain no game data or private research artifacts. No game or shard build,
no game run, and no push.

### Implementation details

`UploadRing` retains a whole-page `BufferViewMut` during producer recording,
using wgpu 29's safe `WriteOnly` interface; no read-capable Rust reference is
manufactured over write-combining memory. The constant packer uses this same
interface for CPU, L148 mapped-primary, and L150 staging destinations.
Aligned buffer ranges coalesce only when actually contiguous: untouched
GPU padding is not overwritten just to reduce the number of copies.

The pool grows in 1 MiB pages, with larger pages for individual oversize
uploads. This is an allocation policy, not a game-derived constant. Pages
are retained for reuse after successful completion/remapping; in-flight
pages cannot be recycled. The fallback path no longer allocates the legacy
48 MiB uniform / 4 MiB inline CPU vectors when the option is enabled.
Primary Vulkan uniform storage and its existing GPU wrap fence are retained.

There is an additional ring-only native-worker ordering fence: sampled-view
preparation can consume pending image uploads after the job's initial flush.
Those copies are flushed before `Recorder::finish_parts` and its independent
consumer submission. The native recorder's own upload implementation remains
unchanged; this task replaces the profiled `WgpuBackend` upload paths.

### Verification

- `cargo test -p ddgpu -p d3d9cmd --lib`: **26 ddgpu + 13 d3d9cmd pass**.
  Full `cargo test -p d3d9cmd` also passes (13 unit tests; no doc tests).
  The existing scalar constant ABI oracle checks every compact prefix and
  raw register bits through the adapted packer.
- **24 selected GPU cases pass**, no failures, skips, or timeouts. Each
  process checks `pause.py status`, aborts if a pause begins, and has a hard
  60-second limit. Total case wall time 129.226 seconds; longest 57.643 s.
  Six new cases cover a 9 MiB buffer lifecycle, 20 texture layout cases
  (BC1/BC3, float16, mip levels, array layers, 3D slices), and legacy-vs-ring
  mixed uploads/draws with 1/2/4 encoding workers and raw Vulkan.
- Existing gates cover pending/existing image uploads; three copy-order
  cases; four draw/format cases; L137's 1/2/4-worker sequence; three native
  worker ordering cases; render-to-texture/cube targets; raw passes/query
  fallback; and **144,000 changing-constant draws with two mapped wraps**.
  The D3D9 random-state/format suite reports zero differences in its 150
  attempted cases; the host driver refuses formats 20/21 there. Independent
  new tests exercise format 21 byte-exactly.
- `cargo test -p ddruntime --no-default-features --no-fail-fast`:
  **203 pass, one inherited fullscreen-gamma failure, five ignored**.
  Counts exclude child-process copies of x87 service tests. No runtime
  source edits. The failure is
  `a_gamma_ramp_is_shown_on_a_full_screen_swap_chain_only`, already reported
  by L147/L148, unrelated to staging.
- An initial broad `--tests --examples --no-run` also encounters the existing
  `module_cost` example's missing `crate::pipeline_log` module. Relevant
  tests and both requested benchmark examples build successfully; that
  unrelated example was not changed.

BLAKE3 readback receipts:

| Case | Bytes | Hash |
|---|---:|---|
| Mixed sequence, queue/ring, wgpu/raw, 1/2/4 workers | 2,304 | `62b32852fb606eafb70ea131eaa10fdc18176a7adb14a9c03bda9621c3702608` |
| Twenty texture layout payloads | 2,360 | `a98b880fd0cd41c17d75d0162cbb36d481c69fc4c8f89b874a75b7224c63b0a7` |
| Buffer lifecycle cycle 0 | 3,145,728 | `efb848cf9983de379cc73dc4a502ba8fe9a56c765b7f8b7e3e351f32e478142b` |
| Buffer lifecycle cycle 1 | 3,145,728 | `a8e8b80425a176910d506f7e09cde376661befabc058d5daa1eb348e4c900853` |
| Buffer lifecycle cycle 2 | 3,145,728 | `fc8dfc7a274a72f780fb115d692b1bb564483cfa66bfd41b9517e7b890f377f0` |

The buffer lifecycle allocates four pages and reuses eight. Texture layout
checks allocate one and reuse nineteen. The mixed sequence uses 2-3 pages
and reuses 57-58; raw Vulkan records 22,128 staged producer bytes versus
132,112 with ordinary wgpu because primary uniforms bypass staging.

### Microbenchmarks

Same binaries, three alternating off/on pairs on the shared machine, no
concurrent builds owned by this agent. Workspace code uses dev opt-level 3,
third-party dependencies the repository's dev opt-level 2. No gameplay/FPS
claim. Each run has the same pause check and 60-second cap; all 18 runs pass.

Build/run commands (set `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`,
`CARGO_TARGET_DIR=C:/ddda-targets/L150` for every cargo invocation):

```text
cargo build -p ddgpu --example upload_ring_cost --example backend_cost --config profile.dev.opt-level=3
C:/ddda-targets/L150/debug/examples/upload_ring_cost.exe
C:/ddda-targets/L150/debug/examples/upload_ring_cost.exe --ring
# Each combination of DDGPU_UPLOAD_RING=0/1 and DDGPU_VK=0/1,
# DDGPU_ENCODE_THREADS=2:
C:/ddda-targets/L150/debug/examples/backend_cost.exe 5000 20
```

Upload medians below measure producer work, excluding finish/submit/GPU wait.
One upload corresponds to one synthetic draw's block in the constant case;
MB means 1,000,000 payload bytes. Constants compare the existing aggregate
CPU packing + one queue write against mapped producer fills; dynamic buffers
compare individual queue writes against individual encoder copies.

| Payload | Bytes/upload | Queue ns/upload | Ring ns/upload | Queue ns/MB | Ring ns/MB |
|---|---:|---:|---:|---:|---:|
| Dynamic buffer | 256 | 2,315.9 | 481.9 | 9,046,602 | 1,882,267 |
| Dynamic buffer | 4,096 | 2,338.7 | 736.6 | 570,967 | 179,833 |
| Dynamic buffer | 65,536 | 12,921.6 | 11,090.6 | 197,168 | 169,230 |
| Dynamic buffer | 1,048,576 | 182,852.1 | 177,102.1 | 174,381 | 168,898 |
| Aggregate constants | 256 | 60.2 | 403.4 | 235,065 | 1,575,883 |
| Aggregate constants | 4,096 | 775.6 | 693.8 | 189,364 | 169,394 |
| Aggregate constants | 65,536 | 12,836.1 | 11,053.4 | 195,863 | 168,661 |
| Aggregate constants | 1,048,576 | 224,797.9 | 176,756.2 | 214,384 | 168,568 |
| Texture | 16,384 | 5,246.6 | 3,288.8 | 320,228 | 200,733 |
| Texture, padded copy rows | 65,792 | 13,270.6 | 11,293.8 | 201,705 | 171,658 |
| Texture | 131,072 | 26,047.1 | 22,333.1 | 198,724 | 170,388 |

Buffer/texture readbacks assert every payload byte. All three off/on pairs
have matching hashes per workload. Representative dynamic 4 KiB hash:
`12528320069edd8a8508a52b4c463e786d0b59664dd9947e7f336a1d1d9c76f3`;
padded-row texture hash:
`74d189eab29a44c15a0f6938ff09394570d81a53769a04c8db2f485a13073fa6`.

Backend medians (5,000 synthetic game-like draws x 20 measured frames):

| Path | Ring | Producer us/draw | Total us/draw, GPU waited | Direct allocations/draw |
|---|---|---:|---:|---:|
| wgpu | off | 4.560 | 5.944 | 0.055 |
| wgpu | on | 4.740 | 6.538 | 1.953 |
| Raw Vulkan | off | 2.650 | 3.170 | 0.057 |
| Raw Vulkan | on | 2.730 | 3.178 | 0.057 |

Ordinary wgpu total pairs are 5.944->7.744, 6.318->6.538,
5.448->6.342 us/draw: a regression in every pair. Constant destination
alignment gaps prevent coalescing most exact range copies; approximately
two wgpu copy commands/allocations per draw replace the prior aggregate
write. Small constant uploads regress even when contiguous. Texture creation
also ends an open pass to insert a copy (62 vs 45 passes including warmup).
Raw Vulkan retains L148 primary uniforms and shows no clear total-time gain
or regression (3.170->3.156, 3.124->3.178, 3.264->3.296 us/draw).
Deferred end-to-end medians are 6.054->6.508 us/draw for wgpu and
3.318->3.408 for raw Vulkan; no deferred-path speedup claim.

All twelve backend runs share image hash:
`c8cba3a37a66ce9d6d733b5e0bd0aa7adbb715bba16843ec73d6a4cd0a364a40`.
SHA-256 executable receipts:

- upload_ring_cost.exe:
  `7d1b7ce5c8c4960ea849082e9257cb5d5c9bd01ae14794f5f471f7f1b5096242`
- backend_cost.exe:
  `8e43ce8e337678bba80769f8936a375322e2bcc7b1f3a7cb27876096d767ccca`

Logs and bounded runners remain outside the repository under
`C:/ddda-targets/L150/` (`gpu-results.json`, `bench-results.json`,
`bench-summary.txt`, `L150-runtime.log` is in the parent targets directory).

### Limits and next action

Keep `DDGPU_UPLOAD_RING` **off by default**. Byte/order validation is green,
small dynamic uploads improve, and texture producer work improves, but the
ordinary constant path regresses and there is no overall raw-backend gain.
Do not promote it from these synthetic numbers. The next action is head's
optional same-scene image/loading A/B with `DDGPU_VK=1` and upload ring
0/1; if pursuing ordinary wgpu, first remove the per-range encoder cost
without weakening byte preservation. No original-game comparison was run
by this agent.
