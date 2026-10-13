# L147: bounded CRT attribution and remaining copy costs

Agent: codex-11. Branch: `task/L147-nlg`. Assigned by head/user on
2026-10-13. Runtime-only; no game runs or lifted-shard builds by this agent.

## Brief

Resolve `_NLG_Return2` self samples in combo21/combo23 to the actual code
and callers, reduce evidenced remaining runtime copies with before/after
microbenchmarks, and fix `scripts/sample_profile.py` symbolization.
Build on L144 (`a2ddab7a`): its snapshot zeroing and DrawCall ownership
fixes are already done. Do not duplicate them. Focus on backend and guest
thread `0x1038` after a raw capture identifies the callers.

## Results: attribution checkpoint

- The text-only c21/c23 reports discarded PCs and displacement. Their
  aggregate percentages cannot identify exact sampled instructions/callers.
  Head capture requested on board #12261, updated command #12298.
- Offline exact combo23 EXE/PDB has no `_NLG_Return2` function. In installed
  `vcruntime140.dll`, `__NLG_Return2` is a one-byte RET at RVA `0x1bc30`.
  The following code is private memcpy implementations. The matching
  Microsoft public PDB (RSDS GUID/age
  `372A71F20351D8E03FDDEA56AAB7F2381`) resolves:
  `0x1bc50 memcpy_repmovs_amd`, `0x1bc70 memcpy_CopyDown_amd`,
  `0x1bd60 memcpy_sse2_ermsb_amd`, `0x1c440 memcpy_avx512_ermsb_amd`,
  `0x1c890 memcpy_sse2_ermsb_Intel`. These are host CRT RVAs, not original
  DDDA addresses or claims that a particular variant was sampled in c23.
- Crucial nuance: DbgHelp fabricates a `Size` for export-only symbols as
  the distance to the next export. A benign Python memcpy capture returned
  `_NLG_Return2+43`, `Size=5600`, `Flags=0x200` (SYMFLAG_EXPORT), despite
  the function being one byte. Checking Size alone would retain the bug.
- `profile_symbols.py` uses PDB extents (excluding synthetic export sizes),
  x64 unwind ranges and direct export jump thunks. Unknown code is labeled
  with module/function RVA, never silently assigned to the nearest export.
  Exact Microsoft PDB fetch is opt-in and GUID/age keyed. No hard-coded
  CRT address map or disassembly/game data is committed.
- The profiler retains L131 raw replay/guest attribution and L144
  `--callers`/ended-thread handling. Version 2 saves module identities,
  paths/bases, original PCs, raw DbgHelp names, displacements, sizes, flags
  and bound evidence. `--input --resymbolize` needs no live process and
  refuses mismatching image identities. Unknown leaves stay in the
  denominator and are explicitly of unknown wait/busy state.
- Verification: **17 synthetic Python tests pass**. A bounded one-second
  benign Python memcpy process produced 84 stacks; the old nearest-export
  attribution assigned **78/84 (92.9%)** to `_NLG_Return2`. Offline replay
  with the exact PDB correctly assigns the same **78/84** to
  `memcpy_repmovs_amd`. Process terminated afterward. This validates
  attribution, not game performance. Private receipts:
  `C:/ddda-targets/L147/synthetic-live.json`, `synthetic-resolved.json`.

## Head capture command

Run during representative gameplay, using the exact run's EXE/PDB directory:

```powershell
python E:/ddda-rs-wt/L147/scripts/sample_profile.py --name ddplay.exe --seconds 30 --depth 64 --symbols <run-directory> --fetch-symbols vcruntime140.dll,ntdll.dll,win32u.dll --save-stacks E:/ddda-re/notes/runs/L147-raw.json --callers 12 > E:/ddda-re/notes/runs/L147-profile.txt
```

No new runtime environment flags are needed. Preserve the default c23
environment (including existing L140 flags and two encode threads), or
capture combo24 after L144 if that is the next available head run. Public
PDB download/resolution happens after sampling; raw evidence is saved first.

## Remaining

The c21/c23 text cannot be retrospectively resolved. A new head capture is
still requested to measure current callers/weights after L143/L144/L146.
The older c17 raw capture supplied actionable evidence below, so the
runtime optimization did not have to wait. No game FPS gain or comparison
against the original game's full image is claimed.

## Results: c17 callers and upload-copy removal

Located `E:/ddda-re/notes/runs/profile_c17draw.json` (L131's older raw
capture). Its saved caller symbols are sufficient to identify the callers
even though version 1 omitted module metadata. For the CRT variant names
only, module base `0x7ffcb1f70000` is **inferred** from its exact exported
memcpy-thunk sample `0x7ffcb1f8f020`, the current installed CRT export RVA
`0x1f020`, and 64 KiB module alignment. This depends on the installed DLL
matching the captured DLL; it is not the stronger captured-identity proof
provided by version 2. All 258 NLG samples then fall inside exact PDB
function extents: 224 `memcpy_avx_ermsb_amd`, 34 `memcpy_repmovs_amd`.
No sampled instruction is in the one-byte NLG function. Private analysis:
`C:/ddda-targets/L147/c17-callers.txt`, `c17-resolved.txt`,
`c17-crt-inferred.json`.

| Thread | NLG / non-wait samples | Saved immediate/caller-chain evidence |
|---|---:|---|
| d3d9-backend | 80 / 550 | 39 Queue::write_buffer (21 resource upload, 18 Ring::upload); 17 constant packing; 7 Queue::write_texture; 5 write_image row packing; 3 buffer upload Vec clone; 9 other |
| guest 0x1038 | 17 / 355 | all 17 X87::memory, including callers f_01320320, f_01320590, f_01320700, f_0131a5e0, f_013201c0 |
| guest 0x1034 | 76 / 1009 | 46 read_bytes in upload_reads; remaining snapshots, constants, nDraw and deferred copies |

L143 already removes constant-packing/per-draw costs; L144 removes the
snapshot/DrawCall copies and improves read_bytes. The x87 findings went
to codex-7 on board #12333: L146 already has a typed-word prototype with
9,216 full-state/access cases and 64 fault cases passing, but production
adoption/gating belongs to that agent. No x87 source was changed here.

This batch removes the evidenced **8/80 backend NLG samples' two caller
paths**, not all memcpy calls:

- `WgpuBackend::upload` pads its already-owned buffer Vec in place instead
  of cloning it before `Queue::write_buffer`. Existing small-buffer mirrors,
  resource-size decisions, upload ordering and finisher waits remain.
- `write_image` borrows tightly packed Direct-format image bytes instead of
  extending a second Vec row by row. Padded rows retain the old compaction
  path, converted formats retain conversion, and pending texture uploads
  retain their ownership. Queue staging still copies the supplied data.
  The installed wgpu 29.0.4 queue API documents immediate staging-copy
  ownership; real readback tests also validate that the upload can be dropped.
- No resource/cache invalidation API changed. Shared `lib.rs` upload and
  write_image sections were agreed with L148 owner codex-9 (#12346).
  The branch includes L144 ancestry via merge `8f60df9c`.

### CPU preparation microbenchmarks

`rustc --edition=2021 -O crates/ddgpu/examples/upload_cost.rs -o
C:/ddda-targets/L147/upload-cost.exe`, then three alternating old/new-first
runs (`--new-first` on the middle run). Uses production preparation helpers
and the previous implementation, a counting System allocator and black_box.
GPU staging, submission and producer allocation are excluded. Measured on
the shared machine while other light builds existed; these are preparation
costs, not end-to-end upload times or FPS predictions.

| Payload | Before median us | After median us | Before -> after allocations/reallocations |
|---|---:|---:|---:|
| aligned 4 KiB buffer | 0.078 | 0.004 | 1 -> 0 |
| aligned 64 KiB buffer | 61.390 | 0.004 | 1 -> 0 |
| tight 64 KiB image | 2.718 | 0.006 | 7 -> 0 |
| tight 4 MiB image | 1671.489 | 0.005 | 11 -> 0 |
| padded 64 KiB image (16 padding bytes/row) | 2.979 | 2.915 | 7 -> 7 |

Large-buffer allocation timings vary with the host heap; allocation/copy
elimination is the stable result. Padded-image before/after ranges overlap
(2.236..3.043 vs 2.189..3.205 us), so no padded-path speedup is claimed.
An exact-reserve experiment regressed that path and was not retained.
Receipts: `C:/ddda-targets/L147/upload-cost-{0,1,2}.txt`.

### Verification

- **17 Python tests pass** for attribution, replay, caller reporting, PE
  boundaries, thunks, forwarded exports and exact symbol-cache identity.
- **22 ddgpu CPU tests pass**, including 420 direct-image combinations
  (tight/padded rows, mip/block row sizes and volume depths) and 258 buffer
  lengths. Tight-image borrowing and retained buffer allocation pointers
  are asserted as well as byte equality against previous preparation.
- **12 focused GPU tests pass**, each serial with pause checked and a hard
  60 s subprocess timeout: new upload readback; all three copy-order tests;
  uploaded-texture, async-upload/draw, frames-vs-driver, random states and
  formats, deferred frame, indexed strips; render-to-texture and cube
  render-target comparisons. No timeout/failure. The existing random-format
  oracle reports zero differing cases out of 150 attempted, with driver
  refusal of formats 20 and 21 (not silently claimed as compared); the new
  byte-exact test separately checks format 21 tight and padded uploads.
- Runtime light after adopting L144: **202 distinct pass / five ignored /
  one inherited fullscreen-gamma failure**. The interlocked contention
  timing test failed in the parallel suite and passed isolated; no test or
  threshold was weakened. No shard/game build was performed.
- All build outputs, profiles and PDBs stay outside the repo. No game run,
  game-install modification, private research data commit or push.

Next: head captures combo24 with the corrected profiler to confirm the
remaining per-thread costs, and integrates the upload-copy batch. The large
Queue staging costs remain (mapped staging work belongs to L148); the
guest 0x1038 x87 optimization remains with L146.
