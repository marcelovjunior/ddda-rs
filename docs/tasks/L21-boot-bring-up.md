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
   work, not a runtime service. Owner: claude-3 (L28, on the native-L17
   data); claude-6 takes any lift rejections among them.

Look-ahead: the initializer census (`examples/initializers`, which runs
every static initializer without the CRT start-up first) completes 5,655
of 5,662. Untranslated targets:
- `__setmbcp` (above).
- 0x00CFC150, which builds a 4,096-entry x87 `fsin` sine table at
  0x018CBE20 and fills 0x4000 bytes at 0x018C7E20.
- 0x0137E490, the C++ initializer that runs the same work (call at
  0x0137E515).

The last two lift in L22 but had no fixture: their constant loop exceeds
the recorder's 20,000-instruction cap. The head decided that claude-6 adds
a recorder budget for loops with a statically proven constant bound (L22).
The census's one fault, C initializer 0x012F4FCE (`__initstdio`), is an
artifact: it calls `__calloc_crt` (0x012F8177) before the census has run
`__heap_init`.

Runtime services checked ahead of the CRT steps `__setargv`, `_setenvp`
and `__cinit`, against host measurements
(`research/runtime/windows/CpInfo.cs`, `ModuleName.cs`):
- GetCPInfo / IsValidCodePage: Windows accepts CP_ACP/OEMCP/MACCP/
  THREAD_ACP (0..3), 850, 932 (lead bytes 81..9F, E0..FC), UTF-7 (max 5)
  and UTF-8 (max 4) in GetCPInfo. The runtime accepted only 437 and 1252.
  Now a measured table; unmeasured pages stop.
- GetModuleFileNameA/W (`__setargv`'s argv[0]): when the path does not
  fit, Windows writes size - 1 characters and a NUL, returns size and sets
  ERROR_INSUFFICIENT_BUFFER. On success it sets the last error to 0. A
  non-module handle gives ERROR_MOD_NOT_FOUND. The runtime returned
  size - 1 and wrote the A form as UTF-8. Fixed.

Imports still without a service (static census: 276 imports, 67 unbound,
2026-10-11), with what reaches them:
- WMVCore `WMCreate*`: movies, Codex (L18). D3DX `D3DXGetShaderConstantTableEx`,
  `D3DXGetShaderOutputSemantics`, `D3DXSaveTextureToFileInMemory`: claude-5
  (L20).
- IMM32 (11 functions): only the IME manager at 0x01106680..0x01106D60
  and 0x01147CC0. The window procedure 0x00DF1550 calls it for
  WM_IME_STARTCOMPOSITION..WM_IME_COMPOSITION (0x10D..0x10F) and
  0x281..0x286. The headless desktop sends no IME messages, so this code
  is off the boot path.
- WS2_32 (19 functions) and `wsprintfA`: the socket factory 0x00D8B690
  (callers 0x00CEF430, 0x00D9BFF0) and 0x00DA3300/0x00DA3650. Probably the
  online client. If the boot reaches them, they get Windows' offline
  answers.
- SSPI (`AcquireCredentialsHandleA` ...), AVIFile/VFW (video capture),
  clipboard, `CreateProcessA` (the restart helper), `StringFromGUID2`
  (input device naming, 0x00DF599C), `RtlUnwind` and
  `UnhandledExceptionFilter` (exception dispatch, not modelled).

WinMain 0x004079E0's path: WMVCORE presence check, SteamAPI_Init,
`sApp::ctor`, then 0x00DF3AC0 (GetCurrentDirectoryA, a named mutex
CreateMutexA + ERROR_ALREADY_EXISTS check, window class and window
0x00DF31F0/0x00DF1200, LoadAcceleratorsA, the main object from the DTI),
then the main loop 0x00DF0D20. The services on this path exist.

Ahead of WinMain's allocator set-up (0x00740590 creates the "Temp",
"System", "Unit", "Effect", "GUI", "Array/String", "Collision" and
"Physics" heaps, about 244 MB), Ghidra shows `MtVirtualAllocator::vf05`
(and vf11/vf12) calling `VirtualAlloc(NULL, 0x20000000, MEM_RESERVE,
PAGE_READWRITE)`. 0x00D10E90 then commits 64 KB pieces at addresses
inside that reservation, and 0x00D10F60 and the destructor decommit and
release them. The runtime's VirtualAlloc ignored the address, did not
distinguish reserve from commit, and had 384 MB. Measured Windows'
behaviour (`research/runtime/windows/Virtual.cs`) and modelled it:
- MEM_RESERVE gives a 64 KB-aligned region whose pages fault until
  committed.
- MEM_COMMIT works on pages inside a reservation: rounded down, zeroed,
  and already committed pages keep their contents.
- MEM_DECOMMIT and MEM_RELEASE work as measured, as do the errors (487
  ERROR_INVALID_ADDRESS, 87 ERROR_INVALID_PARAMETER).
- Regions are placed bottom-up in 0x60000000..0x78000000 and
  0x80000000..0xFF000000. The exe is IMAGE_FILE_LARGE_ADDRESS_AWARE, so it
  has 4 GB on 64-bit Windows; GetSystemInfo now reports 0xFFFEFFFF.
- New `GuestMemory::unmap` for decommits.

IMM32 is on the path (found by claude-7's L35 map): 0x00DF1200 calls
0x01106C80 right after CreateWindowExW. That calls ImmGetDefaultIMEWnd,
ImmGetContext, ImmSetCompositionWindow (CFS_RECT), ImmSetCandidateWindow
(CFS_EXCLUDE) and ImmReleaseContext, plus ImmSetOpenStatus(FALSE) through
0x01106B30. Measured in a GUI process (`research/runtime/windows/Imm.cs`)
and modelled in `imm.rs` (2cad691). On Windows, ShowWindow also sends
WM_IME_SETCONTEXT (wParam 1, lParam 0xC000000F) and WM_IME_NOTIFY
(IMN_OPENSTATUSWINDOW) between WM_ACTIVATE and WM_SETFOCUS. The runtime's
ShowWindow still leaves them out, and the game's handlers show this changes
nothing: 0x011066D0 only clears the ISC_SHOWUI bits before DefWindowProc,
and 0x011066E0 changes state only for IMN 3/4/5. Recorded as a known
difference with no guest-visible effect.
`StringFromGUID2` (DirectInput enumeration callback near 0x00DF599C)
measured and modelled as well (d1a8d82).

4. Generated from native-L17 with claude-3's L28 group (`__setmbcp`, the
   stdio/lowio layer, `__onexit` and others; SEH helpers as closed
   callees). `Session::new` failed with a binding conflict: L28 lifts
   `__onexit` 0x012ED89C, where the runtime still bound its stand-in
   service. Cause: runtime (cause 1). Fix (4a95a52): `services::install`
   skips a SERVICES stand-in when the lifted program has the original body.
   All 45 runtime tests pass.
5. Real boot: `__heap_init` through `_setenvp` pass. Step `__cinit`. Stop:
   untranslated 0x012F032F (`__fpmath`), called from just before
   0x012EDA84 (`_FPinit`). Cause 2 (an MXCSR/x87 CRT function). Owner:
   claude-6 (L22, train 2). `__control87`, `___set_fpsr_sse2` and
   `__clrfp` follow on that path.
6. Frontier probe (`winmain --frontier`):
   - Every CRT step returns.
   - 5,660 of 5,662 initializers complete; 0x0137E490 and 0x0137E510 →
     0x00CFC150 (the sine table) wait for claude-6's constant-loop budget
     (train 2).
   - WinMain stops at untranslated 0x013054F0 (`__aulldvrm`, the CRT's
     64-bit unsigned divide), called from CRT formatting code just before
     0x012F8DAB. Owner: the platform-CRT group (claude-3, L28).
   - The run printed its report but never exited: the game's guest threads
     kept the host process alive. Fix (4a95a52): the examples end the
     process after the report, and runs use `timeout -k` on the program.
   - IMM32: `imm.rs` (2cad691) was removed in favour of claude-10's L43,
     which answers all 11 IMM32 imports. My measurements, including
     WM_IME_NOTIFY once the window has the focus, went to claude-10.
