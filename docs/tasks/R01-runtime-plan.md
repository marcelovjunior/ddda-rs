# R01: Phase 1 runtime plan (design document, no code)

Owner: Codex (same session), after L08. Branch `task/R01-runtime-plan`
from the head's `main`, worktree `E:\ddda-rs-wt\R01`. Deliverable:
`docs/RUNTIME.md`, a plan the head reviews before anything is built.

## Why

About 57% of DDDA.exe's functions (47% of its code bytes) are translated
1:1 and verified per function, but they only become a game when something
runs them: the lifted code operates on `Mem` (32-bit addresses), a `Cpu`,
`call_ext` for untranslated targets and platform models. Today that
"something" exists only in tests (sparse memory and replayed calls).

## What the plan must decide (with evidence from the exe, not guesses)

1. **Memory model.** Lifted code uses 32-bit guest addresses. Proposal to
   evaluate: reserve a 4 GB host region and map guest addresses into it
   (image sections at their virtual addresses, loaded from the player's
   own exe at run time; heaps and stacks inside it), so `Mem` is a bounds-
   free offset add. Alternatives and their cost (page tables, checks).
   Alignment, atomics (`lock` prefixes, `xchg`, `cmpxchg`: does the
   lifter support them?), and how hand ports and lifted code share objects.
2. **Dispatcher.** The production `call_ext` (x86::set_ext_calls): address
   to function for every lifted function (ext included: tests run only the
   closed ones, production runs everything), hand ports, platform models,
   and a loud failure for untranslated code with the address and its name
   (inventory). Indirect calls through vtables and function pointers land
   here. Measure how often the dispatcher would hit untranslated code on
   the boot path (static reachability from WinMain through the call graph
   in `E:/ddda-re/notes/inventory/calls.tsv`, plus vtables).
3. **Threads.** The MT Framework runs worker threads (job system,
   streaming, audio, rendering). One `Cpu` per guest thread; what the
   game's thread entry points are; how `CreateThread`, events, critical
   sections, TLS and interlocked operations map to Rust.
4. **Platform services**, each with its exe evidence (import table, call
   sites): Win32 (file I/O, time, threads, sync, memory, window/message
   loop), Direct3D 9 (device creation at 0x00DB1960 uses FPU_PRESERVE; the
   path to wgpu: translate D3D9 calls and shader bytecode, or replace the
   renderer at a higher level), audio (which API: XAudio2, DirectSound?),
   input (XInput, DirectInput, raw keyboard/mouse), networking (for the
   later LAN co-op), and the C runtime (the 0x012E0000+ block is platform:
   which CRT functions are reached).
5. **Entry and boot order.** WinMain, the CRT start-up, the game's main
   loop and frame order (docs/METHODOLOGY.md Phase 1), what must run
   before the first frame, and which of those functions are not yet
   verified (list them with their rejection reasons).
6. **Existing code to reuse.** The repo's crates already read the game's
   archives, textures, models and motions and render with wgpu (ddview).
   Which parts the runtime reuses for platform services, which are
   superseded by lifted logic.
7. **Milestones.** A staged path with a measurable result per stage, e.g.
   (a) boot to the first frame with untranslated calls logged, (b) title
   screen, (c) load a save and walk; each stage lists the untranslated
   functions it needs.

## Rules

Read-only research in the exe (Ghidra MCP read tools, the inventory, the
call graph) plus the repo; no edits to the canonical Ghidra project. Cite
addresses for every claim; label anything unconfirmed. No code beyond
small measurement scripts (keep them under `research/runtime/` or in your
private notes). Same git rules as before (identity, no trailer, no push).

## Results

Delivered [RUNTIME.md](../RUNTIME.md) for head review, with no runtime
implementation. It evaluates a reserved 4 GiB arena against checked/page
table alternatives; specifies guest/host ownership, ABI and reentrant
dispatch requirements; identifies thread and callback entry points;
proposes platform, graphics/audio and existing-crate boundaries; and gives
staged gates from startup manifests to title/load/walk and co-op ownership.
Executable claims cite addresses, repo claims cite source paths, and
proposals and unconfirmed behavior are separated from measurements.

Key findings on `9091c6c`:

- The 31,580-entry lifted list has 14,186 closed and 17,394 ext entries.
  WinMain `0x004079E0` has 276 known entries in its direct graph closure:
  89 lifted, 187 absent (144 inventory-platform, 43 other). Adding the
  main constructor and inspected main vtable slots gives 1,591 known,
  991 lifted, 600 absent. These are static sets, not boot call frequencies.
- The CRT initializer loop at `0x012EDABC..0x012EDAC9` walks 104,456
  slots with 5,656 distinct non-null targets, only five inventoried.
  Game registration at `0x0136DDF0` demonstrates why the whole high-address
  block cannot be discarded as CRT. Five of six C initializer targets are
  also absent from the inventory. Initializer discovery is the proposed
  first prerequisite, ahead of a boot implementation.
- Complete installed/analysis `.text`, `.rdata`, `.data`, `.rsrc` raw bytes
  and all 273 normal imports match. Entry points differ. Runtime image
  data can come from the player's installation; BSS, imports, initializers
  and ABI state still need a loader/startup contract.
- Audio creation at `0x00DCE117` uses the XAudio2 2.7 COM identity;
  callback-based file completion at `0x00D0D548`/`0x00D0D552` requires
  alertable guest-thread delivery. Thread entry pointers and atomic import
  sites are listed in the design.
- Current `call_ext` is not reentrant while its handler dispatches another
  ext function; current fixture free is a no-op, and generated direct
  platform calls bypass the ext registry. The plan identifies these as
  prerequisites, not already implemented runtime services.

Validation: both small read-only scripts ran successfully against the
private inputs; original/full-detail and final/lightweight census results
agree on all four root summaries, import sites and atomic candidates.
Cross-checks pass for the complete 43-entry documented frontier, PE section
equality, initializer counts, Python syntax and local document links.
Private reports are under `E:/ddda-re/notes/runtime-R01`; scripts refuse
outputs under the repo/shared native input. No Cargo tests/build, vectors,
native gate, inventory rebuild or game execution was needed/performed.

Limit: Ghidra endpoint `127.0.0.1:8089` refused connections; the local bridge
also lacked its `mcp` dependency. Head was notified. Research continued via
read-only disassembly/inventory and prior addressed FRAME.md evidence;
no canonical project changes or server restart. No runtime functionality
or live boot comparison is claimed. Open review decisions are the initial
memory/thread access policy, D3D9 versus a separately researched higher-level
render bridge, Steam service policy and initializer/virtual-target discovery.
Source/docs only, required git identity, no trailer, no push.
