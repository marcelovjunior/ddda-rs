# The 1:1 pipeline: how to port a DDDA.exe function

This is the working manual for the systematic rebuild (METHODOLOGY.md,
"Systematic rebuild"). Every function ported this way is a whole,
line-for-line translation of the original, checked by a differential test
that runs the original's own instructions. Read this before porting
anything; INVENTORY.md has the numbers and the map.

## Rules

1. **Whole functions.** Every branch, in the original's order. No partial
   ports, no "not yet" branches, no stand-ins. If a branch reaches code you
   do not own, that callee goes behind an explicit trait or closure (see
   "Callees"), and the vectors record that the original reached it.
2. **Original state, original offsets.** Ported state is named fields that
   carry the original's offset in their name (`flags_273c`, `status_2e34`),
   with a doc line per field. Do not reshape the original's data into a
   "nicer" model in ddnative; the game layer above can wrap it.
3. **Bit-exact arithmetic.** Single-precision SSE in the original is `f32`
   in Rust in the same operation order; no fused multiply-add, no
   reordering. Lifted x87 uses the full hardware state model below;
   hand-ported f32 copies use `ddnative::x87_copy` (see pitfalls).
   Integer wrap-around is `wrapping_*` where the original can
   wrap.
4. **Verified, not assumed.** A port counts only when its differential test
   replays vectors recorded from the original and passes. Commit only
   behind a passing `cargo test -p ddnative`.
5. **Nothing from the exe in the repo.** Vectors, disassembly, family
   lists and the inventory database stay under `E:/ddda-re` (private).
   The repo holds the ports, the tests (which skip without the vectors),
   and the scripts that regenerate everything from the user's own exe.

## Setup

| thing | where |
|---|---|
| unpacked analysis exe (Steamless) | `E:/ddda-re/bin/DDDA.exe.unpacked.exe` |
| Python with capstone 5 + unicorn 2 | `E:/ddda-re/tools/pyenv/Scripts/python.exe` |
| Ghidra project (DTI-labelled) | `E:/ddda-re/ghidra/DDDA.gpr` (headless; JDK 21 at `C:\Program Files\Eclipse Adoptium\jdk-21.0.12.101-hotspot`) |
| inventory database | `E:/ddda-re/notes/inventory/inventory.sqlite` |
| family files | `E:/ddda-re/notes/inventory/{shape,exact}_families.json`, `E:/ddda-re/notes/native/families/*.json` |
| platform list | `E:/ddda-re/notes/native/platform.json` |
| vectors | `E:/ddda-re/notes/native/vectors/*.json` |

Rebuilding the map from scratch (only needed if the Ghidra project
changes):

```
# 1. export all functions + call edges (read-only, ~1 min)
set JAVA_HOME=C:\Program Files\Eclipse Adoptium\jdk-21.0.12.101-hotspot
analyzeHeadless.bat E:\ddda-re\ghidra DDDA -process -noanalysis -readOnly \
    -scriptPath E:\ddda-rs\research\ghidra -postScript ExportInventory.java E:\ddda-re\notes\inventory
# (delete a stale E:\ddda-re\ghidra\DDDA.lock first if no java process runs)
# 2. families and the platform layer (~10 min)
PY research/native/families.py shapes
PY research/native/families.py exact
PY research/native/families.py platform
# 3. the tracker
python scripts/inventory.py build
python scripts/inventory.py partial      # optional: completeness of 'translated'
python scripts/inventory.py attribute    # optional: classes for unlabelled code
```

`PY` = the pyenv Python above. `scripts/inventory.py` uses the system
Python (stdlib only).

## Tools

### scripts/inventory.py (system Python)

| command | what it does |
|---|---|
| `build` | loads the export, marks `translated` (Rust cites the address), `documented` (docs cite it), `platform` (platform.json + the CRT block >= 0x012E0000), `verified` (a `Translated from` line in `crates/ddnative/src`, plus every member of a family whose representative is ported) |
| `summary` | counts and bytes per status |
| `next N` | untranslated functions most called by translated ones (dependency frontier) |
| `partial` | how many `translated` functions are really complete (all callees translated, no gap note nearby) |
| `classes N` | coverage per DTI class |
| `attribute` | infers classes for unlabelled (`Global`) functions by contiguity and callers |
| `systems` | coverage per game system (families of class names) |
| `ready N` | non-platform functions whose direct callees are all verified (or none): portable now |

### research/native/families.py (pyenv Python)

| command | what it does |
|---|---|
| `shapes` | groups functions whose bytes match after masking in-image 32-bit constants (vtables, globals, tables) and rel32 call/jump targets; slots come from capstone's encoding offsets, so only real immediates/displacements are masked |
| `exact` | records each family's slots and every member's slot values (`exact_families.json`) |
| `platform` | classifies boilerplate families as platform (rules below) |
| `show N` | the N largest families that are neither platform nor verified |

### research/native/harness.py (pyenv Python)

```python
import harness as h
h.disasm(addr, length=0x200, stop_at_ret=True)   # text listing
h.code(addr, n)                                   # raw bytes from the image
r = h.run(addr, ecx=0, args=(), scratch=b"", regs=None, allow=(), max_insn=100000, poke=None)
# r = {"eax", "edx", "esp_delta", "saved": [ebx, esi, edi, ebp], "scratch": bytes, "insns"}
```

- Every PE section is mapped at its image address; the original's code,
  constants and tables are real. Stack at 0x0F000000, scratch memory at
  `h.SCRATCH` (0x0E000000, 1 MB) holding `scratch`; build your objects in
  it and pass pointers into it.
- `ecx` is `this` (thiscall). `args` are pushed as 32-bit stack arguments
  after the return address. `regs` sets other registers (e.g.
  `{UC_X86_REG_EAX: ptr}` for functions that take their object in EAX).
- `esp_delta` is the callee's stack cleanup: `ret 8` gives 8. Assert it:
  it confirms how many stack arguments the function takes.
- A direct `call` (E8) to an address not in `allow` stops the run with
  an error naming the target. That is how a vector records "the original
  reached callee X"; catch the `RuntimeError` and check the target.
- Indirect calls (`call eax`, vtables) go wherever the memory says. Point
  a fake vtable in scratch at a **real original function** to compose two
  originals (the `uCharacterBase::vf111` test calls the original
  `uNpc::vf180` this way), or keep the branch out of the vectors.
- `poke={addr: value}` writes a 32-bit value into the image before the
  run: use it to point a global (e.g. a manager singleton at 0x018FA4BC)
  at a fake object in scratch.
- **Verified callees run for real.** When a function calls one that is
  already verified (e.g. every enemy action entry calls `cActionBase::vf05`
  0x007F1890), pass `allow={0x007F1890}`: the original callee executes
  inside the vectors, and the port calls the verified Rust function at the
  same point. This is how ports compose.
- **Scratch is executable.** For a *virtual* callee that is not the code
  under test (e.g. an owner's `getDTI` at vtable +0x10), write a tiny
  fixture into scratch (`B8 <imm32> C3` = `mov eax, imm32; ret`) and point
  the fake vtable at it. The port takes that callee's result as a closure.
  Say so in the generator's docstring.

## Workflow

1. **Pick.** `python scripts/inventory.py next 30`, or
   `PY research/native/families.py show 30` for families (best yield), or a
   class you are rebuilding. Skip platform code (below). Prefer functions
   with no direct calls first; then ones whose callees are already
   verified or can sit behind a trait.
2. **Read the original.** `h.disasm(addr, size, stop_at_ret=False)`: use
   the function's full size from the inventory, since many functions have
   several `ret`s. Note every memory access (offset, width), every branch,
   constants read from the image (print them: `struct.unpack('<f',
   h.code(addr, 4))`), the calling convention (`ret N`, EAX inputs), and
   what is returned (often only AL is meaningful).
3. **Port** into `crates/ddnative/src/<module>.rs` (module per class
   family: `action`, `character`, `enemy`, `model`, `particle`,
   `player_action`, ...). Add the line
   `// Translated from 0x00XXXXXX (DDDA.exe 2017-12-13 Steam), Class::vfNN.`
   directly above the function: the tracker reads this line.
4. **Record vectors.** A script `research/native/vectors_<topic>.py` builds
   memory images in scratch, runs the original over edge cases (zero,
   all-ones, each flag bit alone, boundary values, NaN/inf, null
   pointers, absent objects), and writes JSON to
   `E:/ddda-re/notes/native/vectors/<name>.json`. Fill objects with
   **garbage** (random bytes) so the test also proves which bytes the
   original leaves untouched; put guard words just outside written blocks.
5. **Replay.** Add a test to `crates/ddnative/tests/differential.rs` that
   loads the vectors with `vectors("<name>")` (returns `None` and the test
   passes trivially when the private file is absent) and asserts the port's
   output equals the original's for every case.
6. **Declare and gate.** Put `// verifies 0x00XXXXXX ...` directly above
   the test (every address the test proves; for a family, its
   representative). Then run `scripts/native_gate.sh && git commit ...`.
   The gate runs the tests with `DDNATIVE_REQUIRE_VECTORS=1` (a missing
   vector file fails instead of skipping), rebuilds the tracker, and fails
   if any `Translated from` port lacks a declared test. The tracker credits
   a port only when both lines exist.

## Porting conventions

- **Calling conventions.** thiscall: `this` in ECX, stack args, `ret N`
  cleans N bytes. Some functions take the object in EAX (the extended
  action constructor). Byte arguments arrive in a 32-bit slot whose upper
  bits are garbage; the port takes `u8` (the vectors pass random upper
  bits to prove it).
- **Returns.** If the original only sets AL (`mov al, 1` / `xor al, al`),
  the port returns `bool`/`u8` and the test compares `eax & 0xFF`.
  Pointer returns become an enum naming what is pointed to
  (`character::NpcFlags::{Intel(addr), Own}`).
- **Pointers.** A null check in the original is an `Option` in the port.
  When the original checks a *computed* address for zero (a wrap-around
  case, e.g. `uPlayerBase::vf85`), keep that check with `wrapping_add`.
- **Exe tables.** Tables the original indexes in its own image (e.g.
  `uPlayerBase::vf100` at 0x014F2AF0) are read through a closure
  parameter; the game layer supplies it from `ddexe` at run time. Never
  copy table contents into the source.
- **Globals.** Singletons the original reads (scene state at 0x018FA4BC,
  effect owner 0x018D2818) are plain parameters of the port; the vectors
  set them with `poke`.
- **Callees.** A callee you do not port is a trait (`player_action::Effects`
  for the effect release 0x00427560) or a closure (`npc_flags` in
  `damage_edges`), called at the same point and under the same
  conditions as the original, with the same arguments. The vectors record
  the arguments the original reached it with.
- **Floats.** `x87_copy` for every `fld dword` / `fstp dword` pair;
  `f32` SSE arithmetic in the original's order (`(1 - t) * a + b * t`
  computed as the original's instructions do).

## Vector file format

A JSON array of cases; each case holds the inputs and an `out` (or named
output fields), with every float as its 32-bit pattern (u32), byte images
as hex strings, and the original's return in `ret`/`al`/`eax`. Keep the
generator deterministic (`random.Random(seed)`), so a rerun reproduces the
same file.

## Families

A family is a set of functions with identical bytes except at parameter
slots: 4-byte in-image immediates/displacements (vtables, globals, tables)
and rel32 call/jump targets. One parameterized port covers it; every
member is still replayed by the family test.

- Membership must be **exact** (same bytes, same slot layout). The earlier
  loose grouping (scanning any 4-byte window for an address) admitted 15
  constructors that write a float at another offset; the per-member test
  caught it. `families.py` now uses capstone's encoding offsets.
- A family file `E:/ddda-re/notes/native/families/<name>.json` holds
  `{"representative": addr, "members": [...], "rule": ...}`. When the
  representative's `Translated from` line is in ddnative, `inventory.py
  build` credits every member.
- The family's varying constant is a port parameter (the vtable for the
  constructors); the vectors record each member's value
  (`struct.unpack_from("<I", h.code(addr + slot_offset, 4))`).

## The lifter (main engine)

### Supplementary inventory entries (L10)

`scripts/discover_functions.py` reads the installed PE, the original
`functions.tsv`, and a lifted manifest. It emits a **private**
`extra_functions.tsv` and a sibling JSON report with input hashes. It never
contacts Ghidra. Evidence records distinguish C/C++ initializer slots,
RTTI-backed virtual slots, and decoded direct-call instructions. Initializer
bounds are checked against the driver at `0x012EDA5E`; see
[INVENTORY.md](INVENTORY.md#supplementary-discovery-l10) for the admission
rules and limitations. Bulk entries and evidence are never committed.

`INVENTORY_DB` selects the database for **all** lifter entry and noreturn
lookups, scanning and vector workers. `LIFT_DATA` alone does not select an
inventory. For an isolated worktree in PowerShell (research Python as `PY`):

```powershell
$env:LIFT_DATA = 'E:/ddda-re/notes/native-L10'
$env:INVENTORY_DB = "$env:LIFT_DATA/inventory.sqlite"
$env:INVENTORY_EXTRA = "$env:LIFT_DATA/extra_functions.tsv"
# INVENTORY_EXPORT defaults to the shared, read-only Ghidra TSV directory.
PY scripts/discover_functions.py --lifted "$env:LIFT_DATA/patterns/lift.json" --out $env:INVENTORY_EXTRA
PY scripts/inventory.py build
PY research/native/lift.py scan-extra
$env:LIFT_WORKERS = '6'
PY research/native/lift.py vectors
PY research/native/lift.py gen
$env:CARGO_BUILD_JOBS = '4'
$env:CARGO_INCREMENTAL = '0'
$env:DDNATIVE_REQUIRE_VECTORS = '1'
cargo test -p ddnative --test lifted --test units
PY scripts/inventory.py build
PY scripts/inventory.py summary
```

`scan-extra` retains the existing selected bodies and mocking boundaries
verbatim, restores their closed x87 contracts in dependency order, and
scans missing supplementary entries in external-call mode against that
same closed set. Each decoded body is released immediately. Its rejection
report is `patterns/lift_extra_rejections.json`. New entries do not become
closed callees automatically, so old fixtures stay valid. This measures an
incremental selection, not a fresh full scan of every old function.
`scan` remains available for a complete boundary refresh and uses inventory
status rather than an address ceiling: game initializers above
`0x012E0000` must not be excluded as CRT. Refreshing boundaries requires the
normal vector invalidation and replay checks below.

Run discovery again on the expanded lifted manifest to expose additional
direct-call targets or constructor-backed tables. Keep the discovery input
manifest if exact reproduction is required (its hash is in the report).
Do not overwrite a discovery table with a narrower subset manifest.
Builds and vector runs are sequential; watch available RAM and stop the
task's process tree if it falls below 6 GiB. No shared gate is needed for
private verification.

### Translation and verification

`research/native/lift.py` translates functions into literal Rust, one
Rust statement group per x86 instruction, and `harness.run_sparse`
verifies them with automatic vectors. A function lifts when every
instruction is in the supported subset and every `call` (or jump out of
the function, i.e. a tail call) goes to a function that is itself lifted;
indirect calls and unrecognised indirect jumps reject it. This is how most functions get verified; the
hand-written ports above are for the important ones, where a readable
typed API matters.

**Function bodies.** `decode_body` follows reachable instructions from the
entry, following fall-through, direct branches and recognized switch cases.
Inventory `size` is only a hint: Ghidra's exporter uses
`getBody().getNumAddresses()`, which counts bytes but does not describe a
contiguous address span. Bodies can skip data, extend beyond that hint or
share an interior epilogue with another function. A direct branch to a
different inventory entry remains a tail call. Returns and known noreturn
calls end a path. The narrow noreturn set is resolved from inventory CRT
names (`_abort`, `_exit`, `__exit`, `__amsg_exit`); other termination routines
are not inferred. Recording rejects those paths, and generated code panics
if a mocked noreturn callee returns.

Discovery is deterministic and checks executable PE sections, instruction
alignment and a 20,000-instruction limit. A proven adjacent switch bound
permits remote cases; without a bound, table discovery stays in the band
before the next inventory entry, plus already discovered shared code.
Tables are capped at 4,096 entries. These limits reject ambiguous code.
Patterns store instruction starts in `body`, merged byte `body_ranges` and
`noreturn_sites`. The recorder and trace tool use the same body membership
for external calls and own-return checks. Nested calls track return PC and
stack depth so a shared callee epilogue is not mistaken for the root return.

**Generated code.** `crates/ddlift/lift_00` through `lift_15` hold the
lifted bodies in `src/generated/lifted_NNN.rs` (400 functions per module).
The fixed shard is `(address >> 20) % 16`: 1 MiB address bands, independent
of lifting rounds or the selected function set. `ddnative-base` holds
`Mem`, `Cpu`, `Flags`, x87 and platform models; every shard depends only on
that base. `ddnative` keeps the hand ports and re-exports the base modules,
preserving existing imports. Its generated `lifted.rs` is a small facade
with the public `f_ADDRESS` entries, `all()` and `dispatch_table()`.
Each lifted function is `fn(&mut dyn Mem, &mut Cpu)`:

- `x86::Cpu` is the whole machine state: the eight general registers,
  xmm0-7, the flags and the x87 stack. Calling conventions disappear:
  arguments are wherever the original reads them (registers, or the
  stack, which lives in `Mem` at `esp`). Generated functions update the
  borrowed `Cpu` in place; they do not unpack and copy the whole state.
- A lifted `call T` pushes the return address into `Mem` exactly as the
  CPU does. Within a shard it calls `super::lifted::f_T(m, cpu)`; across
  shards it uses `x86::call_lifted` and an immutable function resolver.
  A tail `jmp T` (or a conditional one) runs T and returns. This resolver
  is separate from `call_ext`: direct lifted calls never become mocks.
  Because the callee sees the full state, even
  whole-program-optimised calls that pass values in odd registers stay
  exact. `Cpu::from(Entry)` / `cpu.exit()` convert to the `Entry`/`Exit`
  pair the tests and hand ports use.
- Registers are fields; 8/16-bit registers are masked views (`al` =
  `cpu.eax & 0xFF`). Basic blocks become `loop { match pc { ... } }`.
  A sole entry block ending in `return` with no `pc`, `continue` or `break`
  uses a straight-line body; all other functions retain the dispatcher.
- Switch jumps lift only in the absolute dword form `jmp [index*4 + TABLE]`
  (no base register or segment override). `switch_targets` reads TABLE through
  `harness.code`, stops at the first target outside the function, and requires
  a nonempty table whose every target is a decoded instruction start. All
  targets become block leaders. An immediately preceding `cmp index, N;
  ja/jae default` must agree with the table length. With an intervening
  `movzx index, byte ptr [index + INDEX]`, the bound instead sizes INDEX;
  every byte in that image range must select an existing dword entry.
  Without that recognised bound shape, discovery uses the table alone.
  Rust executes `pc = m.u32(address); continue;`, including the original
  wrapping address calculation and byte remapping instructions. It never
  embeds table values or substitutes a Rust array lookup. The default
  `match pc` arm panics on an unknown target.
- Flags: each flag-setting instruction stores `Flags::new(kind, a, b,
  result, bits)`; `jcc`/`setcc` call `fl.cond("be")` etc., which computes
  CF/ZF/SF/OF/PF exactly (add, sub/cmp/neg, logic/test, inc/dec keeping
  CF, shl/shr/sar, comiss/ucomiss). For 32-bit `imul`, CF/OF track signed
  product overflow.
- SSE scalar ops use `x86::addss(a, b)` etc. (std::arch intrinsics): the
  original's operand order, so NaN propagation matches the CPU. Packed
  add/sub/mul run the scalar op per lane.
- x87: saved hardware state preserves extended values, rounding, precision
  control, tags and status. Integer/real loads and stores, arithmetic,
  comparisons and stack operations execute on the host x87. See
  "Exact x87 arithmetic (L05)" below for supported forms and limits.

**Codegen conventions (L03).** The private `patterns/lift.json` cache
continues to hold the local-register statement groups produced by `lift()`.
`gen` / `rust_fn` lower those groups onto `cpu.eax`, `cpu.xmm[n]`, `cpu.fl`
and `cpu.st`. Whole-identifier substitution leaves strings untouched, and
removes only the exact `SAVE` / `LOAD` templates. Do not change the cache's
call paths or call-boundary metadata for an emission-only optimization:
`callees()` and vector-cache invalidation depend on them. Existing prepared
caches can be regenerated directly, without re-disassembly or new vectors.

The in-place state preserves instruction and memory-access order. An
indirect call still evaluates its target before pushing the return address;
normal calls, tail calls and platform/external handlers see the entire
current state. Safe Rust's exclusive `&mut Cpu` prevents a `Mem` observer
from simultaneously inspecting it. State after a panic is not a valid
machine result; discard it (in-place emission can leave partial updates).

Exact add/sub/and/or/xor templates use `Flags::alu_*` helpers, returning the
masked result and recording the same flag operands, result and width. This
also covers cmp/test, whose result is discarded. Operands are evaluated
once, left to right, before the store; the store's address expression stays
in its original position. Unrecognised templates remain literal. There is
no flag liveness optimization: all flag writes remain, including those
observed by a callee or after a return.

Run `PY -m unittest discover -s research/native -p test_lift_codegen.py` for
synthetic lowering and ordered-memory/full-state call regressions. The
runtime fixture uses `rustc` and temporary files under the worktree's one
`target` directory. Follow it with the required full prepared vector replay;
the fixture is not a replacement for original-exe comparison. See
[L03 results](tasks/L03-lifter-codegen-size.md#results) for measured sizes,
build settings and limits.

The workspace test profile sets `ddnative` to `debug = 1`: function/line
information remains for backtraces, but local-variable debug metadata is
omitted. Dev and release profiles keep their defaults. To inspect locals
in a test debugger, override with
`cargo test -p ddnative --config profile.test.package.ddnative.debug=2`.
The measured default module size (400), codegen-unit setting and optimization
level were retained; smaller modules, fewer codegen units and opt-level 1
did not improve the subset build. Removing all debug info saved too little
over level 1 to justify losing line information.

**Sparse vectors** (`harness.run_sparse(addr, seed)`): every general
register random, ESP in a random stack page holding the return address,
every page mapped on first touch (exe pages from the image, any other
page seeded random bytes). Hooks record each byte read before it was
written (the inputs) and the final value of each byte written (the
outputs), plus all registers, xmm0 and the x87 top (read with an `fstp`
run on a fresh page afterwards). Rust replays them over `mem::Sparse`,
which **panics on any read of a byte the original did not read**; the
test compares all eight registers, xmm0, st0 and the exact set of written
bytes.

Cases are rejected (another seed is tried) when:

- the original loads a NaN or infinity float from memory, or runs an SSE
  arithmetic instruction with two NaN operands (Unicorn/QEMU picks the
  result NaN by the x87 rule, larger payload; SSE hardware returns the
  first operand);
- it runs more than 20,000 instructions (runaway loops over garbage counts);
- it maps more than 256 distinct pages (`harness.MAX_PAGES`: a loop over a
  random count of random pointers costs minutes per run otherwise);
- it executes code outside the exe image (a mocked callee's stack pop was
  wrong and a `ret` went astray, or a jump through random data), or returns
  with ESP not 4..0x404 above its entry value;
- it ends with an x87 stack fault, or the emulator faults.

L05 explicitly initializes the control word to `0x037F` and tags to empty.
The installed Unicorn build actually resets those registers to `0x0000`
and `0x5555` (confirmed by register reads and a synthetic `fnstcw`), so
assuming hardware reset defaults is insufficient. It checks occupied/empty slots before
each x87 instruction. Unicorn does not reliably flag a ninth push. Six
new arithmetic callers exposed this: loops call `0x012F05C0` with a value
in st0, but the generic external-call mock does not consume that value.
The original conversion routine pops it at `0x012F05D2` on its SSE path
(its fallback also consumes it); this needs an explicit platform model.
The stack eventually fills; hardware produces an indefinite value while
Unicorn overwrites slots without raising SF. Those runs are rejected,
not translated to reproduce the overflow. Modelling x87-consuming external
callees remains separate work; other usable paths can still be compared.
New cases additionally carry `st0_ext` (80-bit mantissa/exponent),
`x87_control` and `x87_depth`, checked by generated replay. Older cases
retain their original float-return comparison; they are not retroactively
claimed to check full extended precision.

Functions with no usable case are dropped from the batch, not counted;
`patterns/lift_novector.json` remembers them so later runs skip them.

For boundary fixtures, `run_sparse(..., zero_inputs=True)` (or
`LIFT_ZERO_INPUTS=1` with `lift.py vectors`) zeros initial general registers
except ESP, non-image sparse pages and mocked return values. Executable
image bytes, the stack/return setup and the original instructions remain
unchanged. This supplies empty counts and switch index zero instead of
almost always taking a default branch on full-width random values. Such
cases are tagged `"zero_inputs": true`; they are additional inputs, not a
model of real game state or a substitute for random/other-branch coverage.
The option affects only missing vectors. For a private retry in another
input mode, first remove those addresses from the private no-vector cache;
keep successful cases and report functions that still have no usable case.

**Structured fixtures (L09).** `run_sparse(input_strategy=...)` offers
three versioned, opt-in distributions for inputs that otherwise drive
unbounded loops. These are synthetic test distributions, not claims about
real object layouts, allocator behavior or game constants:

- `small-v1`: non-image pages contain seeded 32-bit words chosen from
  0..31, with zero/one favored; initial registers and mocked integer returns
  are aligned multiples of four in 0..28 (usable as pointers or counts).
  Mocked float results are -1, 0 or 1.
- `zero-objects-v1`: initial registers and mocked EAX point into a bounded
  32-page synthetic object region at `0x20000000`; non-image pages and
  other mocked returns are zero. Object addresses can alias.
- `objects-v1`: ECX starts at a synthetic object, other registers are
  small, and non-image words mix small values with object pointers (one
  choice in eight). Mocked EAX is an object pointer, ECX/EDX are small.

Each page is filled once, before the guest reads it, using a seed and page
address. The recorder never changes a previously read value to influence
a branch. Image bytes and original instructions are preserved. The vector
records `input_strategy`, complete consumed input bytes and mocked returns;
the same strict Rust replay checks them. `zero_inputs` and `input_strategy`
are mutually exclusive. Default random fixtures keep their original input
generation. All existing rejection guards remain active.

`research/native/structured_inputs.py` provides private investigation:

```powershell
# Set private LIFT_DATA and INVENTORY_DB first; DB must include supplementary entries.
PY research/native/structured_inputs.py prepare
PY research/native/structured_inputs.py record --strategy random --seeds 3 --seconds 20
PY research/native/structured_inputs.py adopt --strategy random
PY research/native/structured_inputs.py record --strategy zero --seeds 3 --seconds 20
PY research/native/structured_inputs.py adopt --strategy zero
PY research/native/structured_inputs.py record --strategy small-v1 --seeds 3 --seconds 20
PY research/native/structured_inputs.py adopt --strategy small-v1
# Repeat record/adopt with zero-objects-v1 and objects-v1 as desired.
# Adoption is provisional: gen and required Rust replay must pass afterwards.
```

Preparation scans missing entries with existing closed callees and releases
each decoded body immediately. It includes high-address supplementary
entries. An optional private `structured_quarantine.json` array excludes
known mismatches. Strategy-specific JSONL files retain each attempted seed,
raw rejection reason and successful cases, allowing interrupted trials to
resume by body key, requested seeds and budget. Different strategies never
share failure caches. Changed closed-call boundaries reject recording or
adoption; use a fresh trial directory and prepare again. Successful
adoptions leave existing selected bodies and cases unchanged. The ordinary
`lift_novector.json` cache is not used to suppress these explicit trials;
accepted addresses are removed from that historical failure list. Adoption
history is cumulative across resumed chunks. The final trial remainder,
rather than the old cache alone, is the authoritative missing-candidate set.

Each function has a wall-clock budget as well as instruction/page limits;
`run_sparse(max_seconds=...)` rejects at an instruction boundary when the
deadline expires and never resumes after a fault. Worker recycling and
garbage collection still apply. The command requires an explicit private
`LIFT_DATA`, refusing the shared native directory. Keep builds and trial
runs sequential and monitor free memory as for ordinary vector recording.

**Rounds.** `scan` lifts in rounds: round 1 gets the leaves, each later
round the functions whose callees all lifted earlier (the `callable` set
grows inside a round too). It stops when a round adds nothing, so call
chains of any depth lift bottom-up and recursion never does. Each entry
in `patterns/lift.json` lists its `calls`. A vector for a caller runs
the original callee code in Unicorn, so the caller's test checks the
composition too.

**Calls to untranslated code (ext).** After the rounds, `scan` makes a
last pass with `ext=True`: any other call (direct to an untranslated
function, indirect through a vtable or function pointer, through the
import table) and any outward jump becomes `x86::call_ext(m, cpu,
target)`. Such functions are marked `"ext": true` and are never callable
by others (a caller treats them as untranslated too). Indirect *jumps*
must pass the same switch-table recognition even in this pass; other
indirect jumps stay rejected, never becoming mocked external tail calls.

Their vectors come from `run_sparse(addr, seed, end=..., lifted=...)`,
which **mocks** each call or jump from the function's own code to
anything outside it that is neither lifted nor a platform model: the
callee does not run; the recorder logs `{target, regs (all eight at the
call), out}` in the vector's `calls` and returns random eax/ecx/edx,
random finite xmm0 lanes and the callee's stack pop (direct targets:
the `ret N` of the target; indirect: the bytes pushed by the function
since its previous call, after the register-saving prologue). The test
installs a replay as the `call_ext` handler (`x86::set_ext_calls`): it
checks the target and every register at each call (so vtable lookups and
register arguments are verified), reads the return address like the
mock did, and applies the same results; a vector also fails if a
recorded call was never made.

Why this is sound: a literal translation must behave like the original
for *every* callee behaviour, so verifying it against arbitrary mocked
callees verifies it for the real one. What it does not verify is the
callee; that is the callee's own vector. A wrong pop guess only makes the
case degenerate (it usually faults and is rejected), never a false pass.
Floats returned in st0 (MSVC returns float and double there):
`x87_depth_check` walks the control flow from x87 depth 0. A direct call
to a closed lifted function adds that function's net x87 effect at its
`ret` (`X87_NET`, computed when it lifts; inconsistent rets reject callers
with `x87-callee`). A mocked call adds 1 where the rest of its basic block
consumes st0 before pushing anything (`returns_float`): those call sites
are stored per function as `"x87_push"` (return addresses). There the
recorder's mock pushes a random finite float through a trampoline on its
own page (`fld dword [X87_SLOT]; jmp [X87_SLOT + 4]`, `harness.X87_TRAMP`)
and records it as `"st_push"`; the replay pushes the same bits onto
`cpu.st`. Any other x87 read or pop at depth 0 still rejects
(`x87-return`). Unicorn does not flag the stack underflow itself, so this
check must stay static.

A platform model's own address is never lifted (`scan` skips
`harness.PLATFORM`): `run_sparse` runs the model at that address, so the
vectors would describe the model, not the code.

Cached vectors go stale when a function's call boundary changes, e.g. a
callee becomes lifted and the caller now calls it directly instead of
mocking it. `vectors` drops (and re-records) cases of a closed function
that contain mocked calls, and cases of an ext function that mocked a
target which is now lifted. `relift` drops functions that no longer lift
under a stricter lifter, and then their callers.

Body-aware vectors also carry a versioned `body_key` over instruction
starts, noreturn sites, direct callees, external-call mode and x87 return
sites. A changed key invalidates successful cases and cached recording
failures (`lift_novector_keys.json`), including interrupted progress. Legacy
vectors have no key and are re-recorded by default. An explicit private
migration may retain cases only after checking unchanged emitted arms and
call boundaries; the required Rust replay must still pass.

**Memory.** L05's monolithic library peaked at 17.82 GiB for about 123 MB
of generated Rust. L06 separates it into 16 crates (one address band is
currently empty). See [L06 measurements](tasks/L06-lifter-crate-split.md#results)
for per-process peaks and clean/one-shard rebuild timings at two and four
jobs. Each crate sets `[lib] test = false`; unit tests remain in
`ddnative/tests/units.rs`, so libraries compile once. Test-profile debug
line tables (`debug = 1`) apply to every shard and the base.
The gate defaults to `CARGO_BUILD_JOBS=4` and `CARGO_INCREMENTAL=0` to bound
the disk cache. Never run two builds, or a build and a vector run,
concurrently.

`lift.py gen` writes only changed files, removes stale generated modules,
and removes the old monolithic `lifted_NNN.rs` bodies. Regenerating the same
input leaves Cargo's fingerprints intact; changing one body rebuilds its
shard and the dependent facade/test, not unrelated shards. Inserting a
function can shift 400-function module boundaries inside its own shard.
After merging source-only crate skeletons, run `gen` before Cargo: private
generated contents are required and are not committed with the skeletons.
The inventory scans the new shard directories for `Translated from` and
the existing integration test for `// verifies`, ignoring stale old bodies.

The facade's public entries, `all()` and `dispatch_table()` install the
resolver automatically. A caller entering a raw `ddlift_*` function must
call `ddnative::generated::lifted::init()` first. The resolver is global,
immutable and reentrant; it contains no machine/player state. External
call handlers remain thread-local. A missing lifted target panics; it
never falls back to an external mock. Cached patterns and vectors keep
their previous call boundaries, so splitting needs no new recordings.

**Unicorn's code-hook size is not always an instruction length.** The
`size` passed to a `UC_HOOK_CODE` callback is sometimes a huge sentinel
value. Decoding with it read about 4 GB in one call: that, not a leak, is
what took vector workers to 3-4 GB and made Claude Code stop jobs for low
memory. Every decode in `harness.py` reads at most 15 bytes and stays on the
mapped page (`ins_len`).

**Vector runs and memory.** Each Unicorn instance gets a 32 MB translation
buffer (`harness.TCG_BUFFER`; the 1 GB default let workers reach 4 GB).
`lift.py vectors` collects garbage after each function, replaces a worker
after 8 functions (`chunksize=1`, `maxtasksperchild=8`) and gives each
function at most `LIFT_FN_SECONDS` (default 20) of seeds: loops over a
random count of random pointers can take minutes per seed and never give a
vector. Use `LIFT_WORKERS=6` while anything else runs, and keep a watch on
free memory; Claude Code stops background jobs when the machine runs low.

**Commands.** `PY lift.py scan` (lift everything it can; prints each
round and the rejection reasons), `PY lift.py vectors` (parallel;
`LIFT_WORKERS=4` to leave the PC usable), `python lift.py gen`, then
`scripts/native_gate.sh`. `PY lift.py one 0xADDR` prints one function's
Rust (leaf form: calls reject).

`vectors` only records functions that have none in `vectors/lift.json`:
a vector depends on the original code, seed, input mode and, for external-call cases,
the mocked-call boundary. Ordinary vectors stay valid when the lifter
changes, but do not reuse external-call vectors when a newly callable
function changes which callees execute versus get mocked. `vectors`
invalidates changed body keys, cases with mocked calls in a newly closed
function, or a mocked target that has become closed. Other changes to the
mocking contract still need explicit private-cache invalidation. Functions with no usable vector are
dropped, then (repeatedly) every function that calls a dropped one,
since its Rust would not compile. Never run `gen` between `scan` and
`vectors`: the test requires a vector for every generated function.

**Extending it.** Rejections are counted by reason (`call` = a call to
something not lifted, `icall` = indirect call, `indirect` = an unrecognised
indirect jump, `jump` = a direct jump out of the function to something not
lifted, or into an instruction). Switch recognition is deliberately narrow:
register jumps, base-relative tables, targets outside the function and
targets in the middle of instructions are not new switch forms to accept
without evidence. A byte index table's bounds constant is **not** the
number of dword entries. Keep table discovery separate from the runtime
memory read, so sparse vectors verify the bytes the CPU actually consumed.
Run `PY -m unittest discover -s research/native -p test_lift_switch.py` for
synthetic recognizer checks, then scan, record original-exe vectors and
replay the generated Rust for every newly admitted function and its callees.
Random vectors can take only the default branch: inspect table-read coverage
before claiming switch-path comparison. Functions dropped for lack of a
usable vector are liftable, not verified. To isolate a caller whose callees
lack usable vectors, a separate private batch may use `lift(..., ext=True)`
with an empty callable set and record with an empty lifted set, then merge
the resulting functions and cases for replay. This checks the caller's
instructions with mocked callees; it does not verify those callees. Never
reuse the caller's earlier vectors across that change of call boundary.
To support a new
instruction, add a branch in `lift()` that emits its exact semantics
(including flags), regenerate, and let the vectors judge. Ideas in order
of yield: indirect calls through vtables (needs a dispatch table from
address to lifted function), calls into hand-ported or platform code
(the allocator: a Rust model with the same observable effects),
remaining indirect forms, reciprocal approximations, and remaining x87 forms.
DDDA's D3D creation flags preserve FPU precision; see the evidence below.

**Integer and SSE extensions (L08).** `ddnative-base/src/integer.rs`
implements ADC/SBB at 8/16/32 bits, one-operand MUL/IMUL and DIV/IDIV,
16-bit two/three-operand IMUL, rotates, double shifts, bit scans and bit
tests. CMOV reads its source even when its condition is false. XCHG reads
both operands before writing either, including an address register exchanged
with memory. CBW/CWDE/CWD preserve unaffected register bits. Bit-string
memory operands use a signed register index to select the addressed word;
immediate indices wrap within the operand. The recorder pre-maps that
effective address as well as ordinary memory operands.

Undefined arithmetic flags from the new operations are represented by
`PARTIAL` with a defined-bit mask. Reading an unavailable flag panics
in replay, and the recorder rejects that execution path. Defined flags are
checked against host instructions in the base-crate tests. Zero-divisor and
quotient-overflow paths produce no vectors; the Rust divide helper reports
an error instead of trapping the host. A zero bit-scan source and oversized
16-bit double-shift count also reject. This deliberately makes no promise
about CPU-specific values of undefined flags or destinations.

MOVS/STOS and their REP forms execute sequential memory accesses, including
overlap, a zero count, 32-bit address wrapping and either direction. CLD/STD
update a separate `Cpu::df`, which arithmetic does not overwrite. Explicit
16-bit string addressing and unsupported segment overrides still reject.
Guest memory execution remains serialized; this is not a guest-thread
atomicity or cache-ordering model.

`sse2.rs` uses legacy SSE/SSE2 inline assembly with fixed operand order for
packed memory arithmetic, double arithmetic, conversions, comparisons,
unpacks, shuffles and packed integer operations. MOVD/MOVSD, partial XMM
moves and sign-mask extraction preserve or clear upper lanes as specified.
These helpers use masked round-to-nearest MXCSR `0x1f80` and restore the
host word; the recorder initializes the same state. Guest MXCSR changes
remain unsupported. Reciprocal/reciprocal-square-root approximations remain
rejected: CPU-dependent approximation bits cannot be equated with QEMU's
results. The existing two-NaN recorder exclusion also covers new double
arithmetic. Hardware semantics follow the
[Intel SDM, volume 2](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html).

Run `cargo test -p ddnative-base --test instructions` for small hardware
tests, and `PY -m unittest discover -s research/native -p 'test_lift*.py'`
for synthetic emitted programs and recorder checks. These run without the
full generated library. Full required lifted replay is still necessary
before accepting a new private batch. `scan` also saves complete reason
counts in private `patterns/lift_rejections.json`.

**Pitfalls.** Unicorn's UNMAPPED hook is not reliable: for SSE stores and
even plain byte stores it can map the page and still raise. Resuming after
such a fault is wrong too: QEMU keeps EFLAGS lazily, and a stop/restart
between a `cmp` and its `jcc` loses the comparison (three bad vectors came
from that, e.g. `0x00D4E940`). So `run_sparse` maps every page an
instruction will touch *before* it runs (`premap`: memory operands, not
`lea`/`nop`/prefetch, plus the stack just below ESP) and never resumes: a
fault rejects the case.

The function's own `ret` must return to the caller (`run_sparse(...,
size=)`, passed by `lift.py vectors` for every function): a store through
a random pointer can overwrite the return address, and the original then
runs on into image data until it happens to reach the sentinel. Such a
case is rejected (`0x00C1D790`, `0x00D9EF20`, `0x00FB9A10` had only such
vectors). A call to the function's own entry (recursion) is mocked like
any other call: the lifted code makes it through `call_ext`.
Unicorn caches translated code per page: never write new
code into a page that has already been executed or translated (the st0
capture first ran on the return page and silently executed stale bytes).
Generated code is literal, not idiomatic; refactoring a function into a
typed API is welcome later, but the vectors must keep passing.
Closed sparse runs also reject execution outside the mapped exe sections
(except the deliberate st0 capture trampoline). A fragment entered with
the wrong stack can return into seeded data and eventually reach the return
sentinel; that is not a valid original-exe vector. L01 exposed such an older
cached case at `0x00786D74`. Clear invalid cached cases rather than changing
the translation to reproduce execution of random bytes.

## Pattern families

Some code repeats one block with a varying *field offset* (not an
in-image constant), so `families.py` does not group it. `research/native/
patterns.py` holds strict recognizers: each parses the disassembly
instruction by instruction and returns the per-function parameters, or
None if anything falls outside the pattern.

| pattern | functions | port |
|---|---:|---|
| `drop_dead_refs`: clear each held unit reference whose unit's state (+4, low 3 bits) is not 1 or 2 | 23 | `unit::drop_dead_refs` |

`python patterns.py drop_dead_refs` writes the matches to
`E:/ddda-re/notes/native/patterns/drop_dead_refs.json` (list of
`{addr, name, offsets}`); `python patterns.py vectors` records vectors;
a family file `families/drop_dead_refs_family.json` (representative =
lowest address) lets the tracker credit every member. Raw pattern lists
must stay out of `families/` (the tracker reads every file there as
`{representative, members}`).

## Private file layout

```
E:/ddda-re/notes/inventory/  inventory.sqlite, functions.tsv, calls.tsv,
                             shape_families.json, exact_families.json
E:/ddda-re/notes/native/     platform.json
    families/*.json          {"representative", "members", "rule"}: tracker credit
    patterns/*.json          raw recognizer output (per-function parameters)
    vectors/*.json           differential vectors, one file per test
```

## The platform layer

Code Rust replaces instead of translating. `families.py platform` and
`inventory.py build` classify it:

| rule | example |
|---|---|
| deleting destructors (family named `dtor_deleting`) | 3,926 functions |
| DTI new-instance allocators: the first call is the allocator selector 0x00CF5E20 | 2,863 |
| construct-if-memory wrappers (`if (mem) return ctor(mem); return 0`, 21 bytes) | 352 |
| base destructor chains (`mov [ecx], vtable; jmp base`, <= 12 bytes) | 23 |
| MT allocator / memory-manager entry points (0x00CF5E20, 0x00DE0B80, 0x00DE0B90, 0x00CFE960, 0x00CFE9A0) | 5 |
| the C runtime / library block, 0x012E0000 and above | 5,588 |

What platform code *does* still matters to the port: the in-place
constructor that runs after the allocator is game state (it is ported);
only allocation, freeing and type registration are replaced.

### Platform models (calls from lifted code)

Lifted code may call a platform service when that service has a model in
`crates/ddnative-base/src/platform.rs` (table `MODELS`; the same addresses are
in `harness.PLATFORM`). A model starts where the original starts (the
return address at `[esp]`), does the service's observable effect, sets
eax and ends with the original's `ret`; every other register, the flags,
xmm and x87 are left alone. While recording vectors, `run_sparse` runs
the identical model (Python, `on_platform`) in place of the original, so
the vector verifies the *caller's* code around the call.

| address | model | effect |
|---|---|---|
| 0x00CF73F0 | `pool40_alloc` | the global pool of 0x28-byte nodes (critical section, free list): eax = a fresh block |
| 0x00DE0B80 | `mt_alloc` | MT allocator, `ecx` bytes: eax = a fresh block |
| 0x00DE0B90 | `mt_free` | MT allocator free of `ecx`: nothing observable |
| 0x012EFE20 | `memset` | CRT memset(dst, c, n), cdecl, eax = dst |
| 0x012EFEA0 | `memcpy` | CRT memcpy(dst, src, n) with memmove overlap rules, eax = dst |

Fresh blocks come from `Mem::alloc`; `Sparse` (and `run_sparse`) hand them
out with the same bump rule from `mem::HEAP_BASE` = 0x0C000000 (16-byte
steps), so a replay sees the addresses the vector saw. memset and memcpy
are checked against the real CRT code (both SSE2 paths, overlaps) by
`tests/platform.rs`, vectors from `research/native/vectors_platform.py`.
The allocators are replacements by design: no vector can compare them
with the original's pool bookkeeping, which Rust does not keep.

To add a model: write it in `platform.rs` (+ `MODELS`), mirror it line
for line in `harness.run_sparse`'s `on_platform` (same reads in the same
order: the recorded inputs must match what the Rust reads), add it to
`harness.PLATFORM`, rescan. Reject impossible inputs in the Python model
(memset/memcpy over 64 KB) rather than modelling them.

## Pitfalls (all hit for real)

- **Unicorn and x87 signalling NaNs.** Unicorn keeps a signalling NaN
  through `fld`/`fstp`; the CPU quiets it (sets 0x00400000). Checked on
  the maintainer's Ryzen 7 5700X3D. Vectors for x87 paths use quiet NaNs
  only; `ddnative::x87_copy` is tested against the CPU itself with inline
  assembly (`lib.rs`, `x87_hardware`).
- **First `ret` is not the end.** `disasm` stops at the first `ret` by
  default; multi-exit functions need `stop_at_ret=False` and the
  function's full size.
- **Callee counts miss virtual calls.** The inventory's callee edges are
  direct calls only; a "leaf" can still `call [reg]`. Check the code.
- **Invalid fixtures.** If the original would crash on an input (e.g. a
  null joint array with a real joint index), the emulator faults; that
  input is not part of the contract. Exclude it with a comment saying why.
- **Loose families.** See above: only exact families.
- **Chained commits.** A `;` after a failing test still commits. Gate.
- **Silently skipped tests.** A test whose vector file is missing used to
  pass trivially. That happened once: a generator crashed (its fixture was
  smaller than the object's highest field), the test was never added, and
  the port was committed and counted as verified. Since then the gate
  requires vectors and the tracker requires a declared test. Size fixtures
  to the highest offset touched, plus slack.

## Verified ports (as of 2026-10-10)

| address | function | port | vectors |
|---|---|---|---:|
| 0x00AC5100 | cPlAction::vf06 (base update of every player action) | `player_action::base_update` | 720 |
| 0x004C6B50 | uObjModel::vf85 (world origin) | `model::world_origin` | 25 |
| 0x007F1890 | cActionBase::vf05 (base entry: status-bit promotion) | `action::base_enter` | 52 |
| 0x0089EFE0 | cActionBase::vf08 (clear current status) | `action::base_exit_status` | 4 |
| 0x00572860 | cAIActionInterfaceDemo::vf06 (interface state) | `action::demo_state` | 12 |
| 0x010FA570 | cParticleManager::vf08 (counter reset) | `particle::reset_counters` | 60 |
| 0x010FA5B0 | cParticleManager::vf10 (flag clear) | `particle::clear_flag` | 60 |
| 0x00850100 | uCharacterBase::vf111 (damage edges) | `character::damage_edges` | 648 |
| 0x00BB4730 | uNpc::vf180 (NPC flag word) | `character::npc_flags` | 4 |
| 0x00B88AB0 | uPlayerBase::vf18 | `character::player_vf18` | 7 |
| 0x00B53190 | uPlayerBase::vf207 | `character::player_vf207` | 12 |
| 0x00B6AE40 | uPlayerBase::vf187 | `character::player_vf187` | 12 |
| 0x00B531B0 | uPlayerBase::vf100 (exe table) | `character::player_vf100` | 20 |
| 0x00B84300 | uPlayerBase::vf85 (joint 3 world row) | `character::player_joint3_position` | 12 |
| 0x00AAEF50 | uEnemy::vf195 (blend loop) | `enemy::blend` | 12 |
| 0x008C7780 + 956 | action in-place constructor family (957 classes) | `action::construct` | 957 |
| 0x004CBBE0 + 271 | extended action constructor family (272) | `action::construct_extended` | 272 |
| 0x004DFA00 + 29 | action constructor with +0x8C (30) | `action::construct_with_8c` | 30 |
| 0x00ACC4A0 | cPlActTired::vf09 (message handler) | `action::tired_message` | 7 |
| 0x00B44FB0 | cPlActWpnWandBase::vf10 (held references) | `action::wand_base_refs` | 60 |
| 0x00BB2150 | uNpc::vf118 (death reported once) | `character::npc_death_once` | 27 |
| 0x004F57E0 + 22 | drop_dead_refs pattern (23 action vf10s) | `unit::drop_dead_refs` | 276 |
| 0x004E0EB0 + 15 | enemy-family action entry (cEm0600 ... cEm9807, cEmDieBone) | `action::enemy_action_enter` | 160 |
| 0x008D4840 | cEm0100Action::vf05 (entry of every goblin action: velocity/acceleration reset) | `enemy::goblin_action_enter` | 80 |
| 0x00AC4F70 | cPlAction::vf05 (entry of every player action: landing bits, push rank 13, capsule from adjust 0xD3/0xD4) | `player_action::enter` | 200 |

`python scripts/inventory.py summary` gives the live count.

## Exact x87 arithmetic (L05)

`Cpu.st` holds an x87 FSAVE image: eight 80-bit values, tags, TOP,
control word and status. On x86-64, each supported instruction swaps in
that state with `FNSAVE`/`FRSTOR`, executes the original opcode with its
original register operands (or a bounded private memory operand), saves
the resulting state, and restores the host x87 state. SSE and MXCSR are
untouched. Memory operands still read/write through `Mem` at their exact
original widths. No float intermediate is narrowed to f32. The host
must be x86-64; unsupported hosts fail explicitly, without a lossy fallback.
See the [Intel instruction reference](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2a-manual.pdf)
for FNSAVE/FRSTOR.

Supported families include integer loads/stores; real dword/qword/tbyte
loads and stores; stack copies/exchanges; add/subtract/multiply/divide,
including integer-memory, reverse and pop forms; ordered/unordered
comparisons (status-word and integer-flags forms); square root, sine,
cosine; sign/absolute value; control/status words; and SAHF/LAHF.
The depth check requires every referenced stack slot, accounts for all
pops, and rejects overflow. The instruction emitter rejects environment
loads/stores that would expose host instruction/data pointers. Unmasked
x87 exceptions are also unsupported: the recorder rejects such FLDCW
inputs and the Rust helper rejects them before changing host state.
Masked exceptions, rounding, precision control, and condition bits use
hardware semantics; emulator discrepancies must not redefine them.
Run `PY -m unittest discover -s research/native -p test_lift_x87.py` for
synthetic instruction replay and stack-fault/return-contract regressions,
and `cargo test -p ddnative --test units` for precision-control, rounding,
comparison and host-state preservation tests. Keep builds and vector runs
sequential; use two Cargo jobs and four vector workers on the 32 GB host.

**DDDA's D3D flags (exe evidence).** In the function at `0x00DB1480`,
`Direct3DCreate9` is called at `0x00DB1650` through the import thunk
`0x012E1B78` (IAT `0x0139D420`). The returned interface lives at object
`+0xA0`. Its vtable slot `+0x40` is called at `0x00DB1960`, with device
behavior flags `0x46` (push at `0x00DB192A`) or `0x26` (push at
`0x00DB194B`). Both include `D3DCREATE_FPU_PRESERVE` (`0x2`). Thus this
creation path **preserves** the calling thread's control word; it does
not establish single precision. This follows Microsoft's
[D3DCREATE contract](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3dcreate).
The runtime must carry each thread's actual control word through platform
calls. These flags alone do not establish the incoming control word for
every thread. Sparse vectors and default `Cpu` explicitly start at `0x037F`,
extended precision and nearest-even rounding; that is a fixture initial state,
not a claim about the game's live thread state.

## Debugging a failing lifted function

1. `LIFT_DEBUG=0xADDR DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative
   --test lifted`: prints that function's panic message (e.g. `call target`
   with left/right, `registers at the call`, `unrecorded call`), or its
   register and memory differences.
2. `PY research/native/trace_one.py 0xADDR [seed]`: records one vector,
   traces the original's registers at every block start, builds a
   throwaway test with the function's Rust (block trace via `LIFT_TRACE`)
   against the already-built crate (seconds), and prints the first block
   where the two disagree. Agreeing traces with a failing result mean the
   difference is in the last block or the final memory.
3. Check both the emitter and recorder. L08 found Capstone describing
   COMISD/UCOMISD memory sources as m128 although the instructions read m64;
   the emitter now overrides that width. Separately, large-argument FSIN
   can differ between Unicorn and the CPU: an isolated calculation from
   `0x00F276C0` produced `0x181c7560` in Unicorn and `0x181c74e9` on the CPU.
   That function is quarantined in the private mismatch list. Never bend
   hardware semantics to match an emulator discrepancy. Re-record cases
   when fixing the recorder or changing a recorded call boundary.

## What to do next

1. Lifter coverage (Codex briefs `docs/tasks/L0*.md`): remaining indirect
   jumps, x87 input contracts and usable fixtures for newly supported
   integer/SSE functions. L07 handles recursive body discovery; L08 adds
   carry/borrow, divide/multiply, string operations and SSE families.
2. The Phase 1 runtime: the memory model, a production `call_ext`
   dispatcher, platform services (file/archive I/O, D3D9 to wgpu, audio,
   input). Lifted code only becomes a game through it.
3. Hand ports stay for the functions where a typed, readable API matters;
   they keep their own differential tests (table above).
4. Keep INVENTORY.md and the numbers in this file current with each batch.
