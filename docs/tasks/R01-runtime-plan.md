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

(Codex fills this in.)
