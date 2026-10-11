# L10: functions missing from the inventory (initializers, virtual targets)

Owner: Codex (same session). Branch `task/L10-inventory-gaps` from the
head's `main` (a507b2a or later), worktree `E:\ddda-rs-wt\L10`. This is
R02's first half (docs/RUNTIME.md, "Staged implementation").

## Why

R01 found that the CRT initializer loop (`0x012EDABC..0x012EDAC9`) walks
104,456 slots with 5,656 distinct non-null targets, and only five of them
are in the inventory (`E:/ddda-re/notes/inventory/inventory.sqlite`, built
from a Ghidra export). Ghidra never defined most static initializers as
functions; virtual-function targets that are only reached through vtables
may be missing too. Our percentages and the lifter both start from that
inventory, so everything those functions do (game state set up before
WinMain, e.g. registration at `0x0136DDF0`) is invisible today.

## Task

1. Discover function entries the inventory lacks, deterministically and
   from the installed image: the C and C++ initializer tables, vtables
   (RTTI-described tables in `.rdata` whose slots point into executable
   sections), and direct call targets inside lifted bodies that are not
   inventory entries. Classify each with evidence; leave out anything you
   cannot show is code.
2. Record them as a supplementary table the tooling reads alongside the
   Ghidra export (e.g. `E:/ddda-re/notes/inventory/extra_functions.tsv` plus
   a `scripts/inventory.py build` step), with the discovery source per
   entry. Sizes are hints only: L07's recursive descent finds bodies.
   Keep the canonical Ghidra project untouched (its MCP server is down).
3. Run the lifter scan over the new entries, record vectors for those
   that lift, pass the lifted test on your data. Report counts per source
   (initializers, vtables, call targets), how many lift and verify, and the
   new totals the inventory reports.
4. Note in docs/INVENTORY.md and docs/NATIVE_PIPELINE.md where the extra
   entries come from and how to regenerate them.

## Rules

As in L08 (`docs/tasks/L08-lifter-integer-sse.md`, "Rules"); the head seeds
`LIFT_DATA=E:/ddda-re/notes/native-L10`. The recorder's memory fixes are
in main (docs/NATIVE_PIPELINE.md, "Unicorn's code-hook size").

## Results

Implemented installed-PE discovery (`scripts/discover_functions.py`), the
supplementary TSV/SQLite evidence table, private inventory overrides, and
`lift.py scan-extra`. No Ghidra connection or runtime implementation.
Discovery checks initializer-driver operands, forward-decoded MT RTTI
registrations plus object-vtable stores/getDTI, MSVC RTTI hierarchy records,
and direct calls at lifted instruction starts. Arbitrary pointer runs are
excluded. MT store corroboration eliminates ambiguous parent-descriptor
slots that otherwise look like another table's getDTI slot.

The installed image hash is `19facc26...77b8`; its `.text`, `.rdata`,
`.data`, and `.rsrc` bytes still match the analysis image used by the
recorder. Two runs on the same inputs produced byte-identical TSVs. A run
on all newly liftable bodies found the same entry set (additional store
evidence only): no further missing direct-call targets.

| Discovery source | New entries | Liftable | Verified |
|---|---:|---:|---:|
| C initializers | 5 | 5 | 5 |
| C++ initializers | 5,651 | 5,651 | 5,650 |
| RTTI vtable targets | 438 | 416 | 406 |
| Direct-call targets | 0 | 0 | 0 |
| **Total** | **6,094** | **6,072** | **6,061** |

The 438 virtual targets include 433 MT and five MSVC targets. Examples
now verified include `aTitle` slot 8 (`0x00533360`), `aGameMain` slots 7/8
(`0x004FB960`, `0x004FBA60`), and shared target `0x00533840`. Correction to
R01's registration example: `0x0136DDF0` is inside the initializer whose
actual table entry is **`0x0136DDE0`**, now verified.

| Measure | Before | After |
|---|---:|---:|
| Inventory entries, including thunks | 50,767 | 56,861 |
| Inventory entries, excluding thunks | 50,478 | 56,572 |
| Summed inventory code-size estimates | 15,137,078 B | 15,514,032 B |
| Selected lifted functions with passing fixtures | 31,580 | 37,641 |
| Lifted cases | 94,088 | 112,266 |
| Inventory verified, including ten hand ports | 31,590 | 37,651 |
| Verified share of non-thunk entries | 62.6% | 66.6% |
| Verified share of summed code-size estimates | 72.7% | 73.4% |

All 31,580 prepared function records and 94,088 cases are retained
unchanged. Additions use ext mode against the original closed set, so no
old mocking boundary changes. The initial recording passed 6,047 additions;
zero-input retries recovered 14 more. This is an incremental selection,
not a fresh full scan of the old candidate universe.

**Remaining:** 22 new entries reject (21 unrecognized indirect jumps,
one branch into an instruction interior); ten vtable targets and C++
initializer `0x0137E490` have no usable fixture after both input modes.
The 22 undecoded bodies retain explicitly marked one-byte size hints;
other supplementary sizes sum reachable decoded instruction bytes.
Shared tails can overlap, so inventory bytes are not unique image coverage.
Tables whose object-vtable store is absent from the discovery input remain
unconfirmed and excluded. The old 935 full-scan rejections were not
recounted; adding 22 to that historical count would not be a fresh census.

**Verification:** seven synthetic discovery/inventory/incremental-scan
regressions and all 32 existing lifter Python regressions pass. The subset
replayed 60,547 cases across 20,247 functions (all additions plus existing
closed dependencies), with seven unit tests passing. Then one final full
`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative` passed all 32 Rust tests,
including the required 112,266-case lifted replay and seven unit tests.
Legacy hand-port tests retain their existing fixture-dependent behavior;
the lifted replay uses the private data and cannot skip. No live-game boot
or whole-program initializer-order correctness is claimed.

At four jobs, incremental off, the final build took **3m34s**, lifted replay
**48.60s**, total command **268.05s**, minimum observed free RAM **15.63 GiB**.
The initial full scan was proactively stopped because its cached rejected
bodies grew to roughly 12 GiB; minimum observed free RAM was 11.45 GiB,
never below 6 GiB. Only this task's process tree was stopped. The incremental
scan released each body and completed in 14s with 24.12 GiB free minimum.
Vector workers stayed at six; builds and vector runs never overlapped.

**Handoff:** the tested full pattern/vector pair, `extra_functions.tsv`,
input-hash report, rebuilt `inventory.sqlite`, raw incremental scan and
rejections, logs and memory metrics are under `E:/ddda-re/notes/native-L10`.
Keep `patterns/lift-baseline.json` to reproduce the precise discovery TSV;
using the accepted expanded manifest produces the same entry set here.
Only source/tests/docs are committed; generated worktree files remain
uncommitted. No shared inventory/native writes, gate invocation, or push.

To regenerate shared data **after review**, the head adopts the accepted
pattern/vector pair, runs `lift.py gen`, then runs:

```powershell
# PY = E:/ddda-re/tools/pyenv/Scripts/python.exe
PY scripts/discover_functions.py --lifted E:/ddda-re/notes/native/patterns/lift.json --out E:/ddda-re/notes/inventory/extra_functions.tsv
# Clear private INVENTORY_DB / INVENTORY_EXTRA overrides first.
PY scripts/inventory.py build
PY scripts/inventory.py summary
```

Detailed private commands and classification limits are in
`docs/NATIVE_PIPELINE.md` and `docs/INVENTORY.md`. Next action: review/adopt
this supplementary inventory and verified pair before the runtime boot
manifest work; investigate the 33 unverified discoveries separately.
