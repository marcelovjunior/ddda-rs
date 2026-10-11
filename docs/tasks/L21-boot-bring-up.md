# L21: boot bring-up (drive the game from start-up toward the title screen)

- **Owner:** claude-2
- **Branch:** `task/L21-boot` (worktree `E:\ddda-rs-wt\L21`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

Every piece of the platform now exists in some form. The runtime has:

- threads and files;
- windows (user32);
- D3D9 with the wgpu backend;
- input, XAudio2 and the L17 services.

The lifted program is 98.9% verified. What is left is the critical path: run
the real start-up (`Session::start`, which runs the CRT, then WinMain) and
remove whatever stops it, one stop at a time, until the game presents
frames of its title screen.

## Task

Run the boot, read where it stops, fix it, and run it again:

- `cargo run -p ddruntime --example winmain` (headless).
- `cargo run -p ddplay` (windowed, the wgpu backend).
- Both go through the heavy slot.

Each stop has one of these causes:

1. **A missing or wrong Windows service, or a runtime gap** (for example a
   kernel object, a file path in the overlay, a message order, or a thread
   handover). Fix it in `crates/ddruntime` with Windows' documented or
   measured behaviour and a test, as in L17A. Ownership elsewhere:
   - D3D9 files (`d3d9*.rs`) belong to claude-5 (L20).
   - `audio.rs` belongs to claude-4 (L19).
   - Movies belong to Codex (L18).
   - Steam belongs to claude-10 (L26).

   Message the owner (`ListAgents`, SendMessage) with the exact stop and
   evidence, and carry on with the next stop if you can step around it
   locally, without committing the workaround.
2. **An unverified or unlifted function.** The runtime refuses to run code
   that has not been verified.
   - The CRT's SEH helpers (`__SEH_prolog4` 0x012F71E0 and friends) and the
     worker loops 0x00DAC090 / 0x00DBA9A0 are being verified by claude-3
     (L17 B/C, session `l17bc-ab`). Its provisional data is in
     `E:/ddda-re/notes/native-L17`.
   - To look ahead before the head adopts it, you may generate from that
     data in your worktree (`research/native/lift.py gen` with
     `LIFT_DATA`/`INVENTORY_DB`/`INVENTORY_EXTRA` pointing at it; see
     `docs/NATIVE_PIPELINE.md`). Never commit such generated code.
   - Report every other unverified function you meet to the head with its
     address and what calls it. Lifter problems go to claude-6 (L22).
3. **A behaviour difference** (the game takes a path the real game does not
   take). Find the reason with Ghidra and the original's documented
   behaviour, and fix the runtime side.

Keep a boot log in Results: each stop, its cause, its fix (commit) or
owner. Also record the step reached (the `Session::start` report) after
each fix.

## Done when

The game reaches its title screen in `ddplay`, or the remaining stops are
all owned by other agents and reported. Update `docs/RUNTIME.md`'s status
and the "Drivers" paragraph.

## Results

### Boot log

Headless runs use `cargo run -p ddruntime --example winmain` in the heavy
slot (`CARGO_TARGET_DIR=C:/ddda-targets/L21`). Each entry gives the step
`Session::start` reports, the stop, its cause and its fix or owner.

1. main 5985a5b (L17A merged). Step `__mtinit` (the second CRT step;
   `__heap_init` passed). Stop: call to untranslated code at 0x012F71E0
   (`__SEH_prolog4`), called from just before 0x012F95C2. Cause: an
   unverified CRT SEH helper (cause 2). Owner: claude-3 (L17 B/C), whose
   provisional data is used below for look-ahead only.
2. Generated (locally, uncommitted) from `E:/ddda-re/notes/native-L17`
   (58,989 entries: the verified baseline plus 76 provisional additions,
   among them `__SEH_prolog4`/`__SEH_epilog4`). Step `__mtinit`. Stop:
   guest read of 4 bytes at 0x00000000 inside `__SEH_prolog4`. Cause:
   runtime gap (cause 1). `push dword ptr fs:[0]` read FS:[0], but guest
   threads ran with FS base 0 and had no thread information block. The exe
   uses FS only at offset 0: Ghidra finds 53 instructions, all SEH chain
   links. Fix: e754622 gives every guest thread an NT_TIB page (winnt.h:
   ExceptionList = 0xFFFFFFFF end marker, StackBase, StackLimit, Self) at
   0x7FB00000 + 0x1000 per thread, mapped when the thread starts, and sets
   its FS base. Test `every_guest_thread_gets_a_thread_information_block`.
3. With e754622: `__mtinit`, `__RTC_Initialize` and `__ioinit` pass. Step
   `__setargv`. Stop: call to untranslated code at 0x012FB049
   (`__setmbcp`), called from just before 0x012FB1F3. Cause: the inventory
   gives `__setmbcp` status `platform` (rule "CRT/library range"). All 116
   such CRT functions have no lifted body and are outside the residual
   census. Besides `__setmbcp` they include `___updatetlocinfo`,
   `__control87`, `___set_fpsr_sse2`, `___tzset`, the 64-bit division
   helpers, the stdio/lowio layer and about 60 unnamed functions in
   0x012E0EA0..0x0132A390. Only memset/memcpy have platform models.
   Reported to the head with this list: lifting them is native-workflow
   work (head / claude-6), not a runtime service.
