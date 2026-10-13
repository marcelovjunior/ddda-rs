# L175: semantic material state and package lookup leaves

Assigned to codex-6 by head/user, 2026-10-13. Branch
`task/L175-semantic-draw-c`, worktree `E:/ddda-rs-wt/L175`.
Base `task/fps-line`; merged L171 template through `a4a4791d`.

Scope: complete ABI-level rewrites of EA23F0, FED190, E0F3C0 and
01008200, private original vectors, isolated timing, opt-in registration.
No game execution, shard build, GPU test, push, or installation writes.

## Implementation

`crates/mtcore/src/semantic_draw_c.rs` uses `Pages` for guest accesses:

| Original | Native function | Behavior |
|---|---|---|
| 00EA23F0 | `primitive_material` | Select material states; save previous state when enabled; allocate and populate constants; update dirty bits and per-primitive state. |
| 00FED190 | `begin_material` | Call material vtable +0x30; begin its state-save policy and initialize the scope cursors. |
| 00E0F3C0 | `package_key` | Reject excluded keys, then search six shader packages in original order. |
| 01008200 | `single_key` | Search a bucket chain; sign-bit indices mean absent; return a 36-byte record address. |

State/descriptor banks and float constants are read from guest memory;
only instruction-embedded masks, keys and offsets are translated constants.
FED190's virtual call and EA23F0's FED890 growth calls remain guest calls
with their original argument frames. E0F3C0 folds the pure 01008200 lookup
into native logic. This bypasses a separately overridden 01008200 while
inside the semantic E0F3C0; disable the parent semantic port if a mod needs
to replace that nested lookup's behavior.

EA23F0 retains x87 FLD/FSTP for custom floats and hardware UCOMISS for
comparison, including signaling NaNs and guest DAZ/FTZ. Small helpers name
state saving, state selection and buffer allocation; adapters read guest
arguments and return through the original stack cleanup.

All four register under `DDRUNTIME_SEMANTIC=1`; defaults unchanged. The
existing `retain` removes any instruction-level JOB_LOOKUP registration at
01008200 before the semantic replacement. Owner/head agreed additive
shared edits in board #13998/#14012.

## Results

Implemented and compared against isolated original instructions, using
synthetic object graphs, not recorded gameplay inputs:

- **2,226 original returns:** EA23F0 1,024; FED190 256; E0F3C0 256;
  existing L158 01008200 690. E0F3C0 executes the actual original lookup
  callee in the recorder. Virtual/growth calls have explicitly mocked,
  recorded ABI effects; real callee behavior is outside this leaf oracle.
- EA23F0 corpus reaches **712/712 decoded instructions**, all 32 switch
  inputs, both state-save modes, unchanged state, unaligned contexts,
  aliased source/output, zero buffers, and both growth return sites:
  EA27C8 **16** calls, EA2958 **62** calls. Instruction coverage is not a
  claim of exhaustive object graphs or concurrent interleavings.
- ABI checks compare EAX, EBX/ESI/EDI/EBP, ESP, all observed heap/global
  bytes and live stack bytes, and reject unexpected observable writes.
  Only discarded **stack** bytes are excluded; heap addresses below ESP
  are still compared. Call checks include target, ECX, ESP, argument words,
  and already-written non-stack memory. Caller-saved registers, flags,
  and XMM scratch are outside this ABI acceptance contract.
- Original-instruction recorder uses the existing `x87_host` **host-v1**
  oracle. Plain Unicorn incorrectly preserves signaling NaNs on FLD;
  both isolated lifted and semantic implementations quiet them on hardware.
  Signaling cases remain included, with oracle provenance recorded.
  Four x87 rounding modes and SSE rounding/DAZ/FTZ combinations are replayed.
- **Six isolated tests pass:** four semantic vector tests, original lifted
  replay, and serial/atomic production `PageTable` replay. Extracts only
  these four lifted bodies, including EA23F0's four split parts; no shards.
- Runtime light suite: **255 passed, zero failed, seven ignored**.
  Opt-in registration uniqueness/precedence test passes with
  SEMANTIC=1 and JOB_LOOKUP=1. Full mtcore/ddports: **26 passed, zero
  failed, nine ignored**. Final post-review focused suite: four passed.

Synthetic paired CPU costs: extracted lifted bodies at **opt2**, native
mtcore and host/base at **opt3**, atomic pages; eleven selected inputs per
function, seven alternating pairs, 10,000 iterations per sample. Timings
include identical restoration of written inputs and stack arguments; the
virtual/growth mock work is also timed. Shared-host noise remains.

| Function | Faster pairs | Median paired speedup | Range of per-input median speedups |
|---|---:|---:|---:|
| EA23F0 | 73/77 | 1.512x | 0.993–1.798x |
| FED190 | 68/77 | 1.140x | 1.103–1.737x |
| E0F3C0 | 77/77 | 2.734x | 2.317–4.591x |
| 01008200 | 62/77 | 1.141x | 1.006–1.501x |

Earlier FED190 samples included regressing input medians (0.776x), and its
cost is dominated by the mock: no general speedup claim for it. EA23F0's
final sample also has one effectively neutral/slightly regressing median.
No whole-job, FPS, image or gameplay acceptance is claimed.
Next action: head includes all four opt-ins in its accumulated train,
then checks real job timing and image acceptance.

## Reproduction and private receipts

```
E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py codex-6 -- E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/record_l175.py
python scripts/check_l175.py --bench
```

Cargo checks use `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`,
`CARGO_TARGET_DIR=C:/ddda-targets/L175`, and required vectors for mtcore.
Run `cargo test -p mtcore -p ddports` and
`cargo test -p ddruntime --no-default-features`. Raw vectors, original
bytes, disassembly and extracted bodies stay under the private research tree.

| Private artifact | SHA-256 |
|---|---|
| native-L175/00ea23f0.json | `095a6d525359e32f7ddbb91f57425266c9e7c8e5ddf6ea7d5fcebb47c9199099` |
| native-L175/00fed190.json | `eb025e447bea3f8062f8fb918c853fa47f8ebda5efc117790c598a34910eeae9` |
| native-L175/00e0f3c0.json | `da1265c37ebfa42ce7b4b209cd61c09da8779c028cc84ada8e3d3f52951f220e` |
| native-L158/job_lookup.json | `f067c47144e1a470e62db339b27bc7e785fc47138241aee2ee0a8747d3c0e5e9` |
| C:/ddda-targets/L175/isolated.log | `c5e6f9035a2374538288df61d4164056fa627958fab07882c962a7ce49901e94` |
| C:/ddda-targets/L175/runtime-light.log | `25701cd1722ee46e148a492e6f6b30b68af807819e188423cca9424ecc30ea86` |
