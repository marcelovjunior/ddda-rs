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

Game raw-PC/caller capture and remaining-runtime optimization/benchmarks
are pending. No new runtime improvement, FPS gain, image equivalence or
original-game comparison is claimed by this checkpoint. Next: head
capture, then rank exact backend/`0x1038` memcpy callers and claim only
the affected runtime files before editing.
