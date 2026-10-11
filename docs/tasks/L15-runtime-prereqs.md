# L15: runtime prerequisites: original allocator bodies and the FS segment

Owner: Codex (same session). Branch `task/L15-runtime-prereqs` from the head's
`main` after L14 is merged, worktree `E:\ddda-rs-wt\L15`.

## Why

After L14, 55,884 of 56,593 functions are verified (98.7%). Among the 161
lift rejections, the `segment override` ones (10 in the CRT range, 9
earlier) are all `fs:` accesses: the thread environment block (TEB), mostly
the structured-exception-handling chain at `fs:[0]`. They include the
CRT's `__SEH_prolog4` (0x012F71E0) and `__SEH_epilog4` (0x012F7225), which
`___tmainCRTStartup` (0x012EF8A7) and many CRT functions call. The runtime
(crates/ddruntime, now with guest threads) needs these bodies: it will give
every guest thread its own TEB in guest memory.

## Task A: original bodies at the five platform-model addresses

Lifted code calls the five models in `crates/ddnative-base/src/platform.rs`
directly (`platform::mt_alloc(m, cpu)` etc.). The head adds a runtime
override there: when the runtime installs it, each model call goes to the
dispatcher at the model's address instead (tests and the recorder keep the
models). For the allocators that address must then hold the **original**
body: `pool40_alloc` 0x00CF73F0, `mt_alloc` 0x00DE0B80, `mt_free`
0x00DE0B90 (they use the game's allocator object at 0x018D0AA0 and the pool
at 0x01877DA0; a replacement allocator would corrupt blocks the game frees
through that object directly). memset 0x012EFE20 and memcpy 0x012EFEA0
may stay models in the runtime, but lift them too if they lift.

1. Lift and record these five bodies as ordinary functions (their own
   instructions, callees mocked or closed as usual) under a distinct
   original-entry contract: their callers keep calling the model, and no
   existing record or case changes. L14 found 0x012EFEA0 rejects an
   unproven indirect switch table: resolve it or record it with evidence.
2. Make sure the generated program's lifted table (`lifted::all()`) includes
   them, so `Registry::with_lifted_program` binds them.

## Task B: the FS segment

1. `x86::Cpu` gets `pub fs_base: u32` (default 0). An `fs:`-prefixed
   memory operand's linear address is `fs_base + effective address`
   (wrapping); `ds`/`es`/`ss` overrides stay flat as today. Emit it through
   the existing address path so generated code stays readable.
2. Recorder (`harness.py run_sparse`): give every vector the same fixed
   TEB base (choose an address no other sparse region uses, e.g.
   `0x7FFD_E000`, and document it), set it as FS's base in Unicorn (a GDT
   entry, or `UC_X86_REG_FS_BASE` if Unicorn 2.1.4 honours it in 32-bit
   mode: verify which), and record TEB bytes read/written exactly like any
   other memory. Generated tests set `cpu.fs_base` to that constant. No
   vector format change unless unavoidable (then say why).
3. Re-scan and record the segment-override rejections; report which now
   verify. `__SEH_prolog4`/`__SEH_epilog4` adjust the caller's frame
   (they push and pop on the caller's stack and return through it):
   confirm the recorder's own-return/stack checks accept that pattern or
   explain precisely what they need. Do not special-case addresses.
4. Optional, only with evidence: the deleting destructor 0x00DB8CD0 is
   blocked because its callee tail-jumps through the IAT slot of
   `LeaveCriticalSection` (0x0139D164) and the mock's stack cleanup is
   unknown. If the mock can take an import's documented stdcall argument
   size (KERNEL32!LeaveCriticalSection: 4 bytes), propose the smallest
   general mechanism (a table of import argument sizes with its source)
   and apply it.

Out of scope: exception dispatch itself (RaiseException, unwinding), which
the runtime implements.

## Rules

As in L12 (`docs/tasks/L12-remaining-lift-rejections.md`, "Rules"). The head
seeds `LIFT_DATA=E:/ddda-re/notes/native-L15`. Keep CARGO_BUILD_JOBS=4 and
at most six vector workers; never run two heavy jobs at once. Report in
Results below with before/after counts.

## Results

(to be filled by the owner)
