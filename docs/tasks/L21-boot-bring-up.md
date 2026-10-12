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

Static check of the risks in claude-7's TITLE_PATH.md section 3 (while
train 2 builds):
- Archive loading:
  - MtFile::vf05 opens a file synchronously (mode 0x11, plain ReadFile)
    or overlapped (mode 0x12, ReadFileEx at the OVERLAPPED offset plus
    SleepEx(INFINITE, TRUE), MtFile::vf07/vf08).
  - In mode 0x12 it calls GetDiskFreeSpaceA on the file's own full path
    and goes synchronous only if that succeeds. Windows fails it
    (ERROR_DIRECTORY, measured), so the original keeps the overlapped
    path. The runtime answered TRUE, a behaviour difference (cause 3).
    Fixed in 8460216.
  - The overlapped path's services (ReadFileEx completions queued to the
    thread, run at its alertable SleepEx) were already tested.
- Loader threads: 0x00DB95B0 and 0x00DB9C80 wait with
  WaitForMultipleObjects(bWaitAll = TRUE, INFINITE). The runtime's wait
  takes all objects in one step, only when all are signalled, and consumes
  auto-reset events then. That is correct.
- Render handshake: 0x00DAED10 polls the render thread's event with
  WaitForSingleObject(h, 0) in its frame limiter and waits INFINITE
  otherwise; waits hand the guest lock over, so the worker runs. It checks
  Present for D3DERR_DEVICELOST. TestCooperativeLevel answers "never
  lost" (d3d9dev, claude-5), so 0x00DAF7C0's sleep path is not taken.
- Second-instance check: 0x00DF3AC0's CreateMutexA + GetLastError() ==
  ERROR_ALREADY_EXISTS. `create_named` sets the last error to 0 for a new
  object, so no stale 183 can trigger the "already running" exit.

Merged `train/2` (f7ae7dd). Codex's L27 had added its own per-thread TEB
(`seh::initialize_teb` at 0x7E000000 + 0x1000 per thread), with the same
NT_TIB fields as e754622's block at 0x7FB00000. They are unified into one
block per thread: `seh::initialize_teb`, used by `Session::new`, guest
threads and SEH dispatch alike. e754622's `threads::init_teb`/`teb_of` are
gone; the FS:[0] evidence moved into `seh::initialize_teb`'s comment. The
TIB test checks the shared block.
7. main 45c07138 (train 2: L22 and claude-3's L28 adopted; the generated
   code is committed, no private data). Real boot: the whole CRT start-up
   passes, `__cinit` with all 5,662 initializers, and WinMain runs. Stop:
   untranslated 0x00DAFED0, called from just before 0x00DB045C.
   - 0x00DAFED0 is the partition step of an introsort (median of three via
     0x00DAFD10, records compared on two uint keys and a float). Its caller
     0x00DB0410 sorts 0x5C-byte records. The path is 0x00DB0DE0 ←
     0x00DB1480 ← `sRender`'s constructor 0x00DB1A50 (PIX options, then the
     device set-up).
   - Cause 2: it lifts but has no accepted fixture
     (`patterns/lift_novector.json`). Reported to the head for the recorder
     owners.
   - Runtime game tests 44/44. The exe exits cleanly now that its output
     goes to files.
   - Frontier probe, less faithful now since it skips `_FPinit`: WinMain
     stops at 0x01305585, reached by a computed call from just before
     0x012F8BF6. That is exactly the end of `__aulldvrm` (0x013054F0 +
     0x95): code past Ghidra's function end, L45's class. Sent to claude-6
     as a lead.

### Look-ahead from WinMain to the title (static, main 45c07138 + train 2)

Method: the direct-call closure in the shared inventory
(`E:/ddda-re/notes/inventory/inventory.sqlite`, read-only) from the title
path's roots, following each route only through verified functions and
recording the first unverified one. Roots: WinMain, app init 0x00DF3AC0,
the window procedure, the main loop 0x00DF0D20, `sBbsRpgMain`'s
constructor and frame (vf06), `sRender`'s constructor, `aInitialize` and
`aTitle` slots 5-8 and the first title state, and the eight guest thread
entries of TITLE_PATH.md section 1.7. Reasons come from
`patterns/lift_novector.json` (no fixture), the rejection and mismatch
lists and the quarantine files.

Not covered: calls through vtables and function pointers (most of the
game's dispatch), so this is a lower bound. The real boot will show more.

Imports: 139 are reached on these routes, and all have a runtime service
now. The last one, `CreateProcessA` (the self-restart helper 0x011797F0),
stops with a report naming the command line (e42d7b9f).
`UnhandledExceptionFilter` (`___report_gsfailure`) is L27's seh service.

42 unverified functions, by owner. Depth is call levels below the root.
"Not in the lift set" means status documented or translated with no
recorded rejection, so the lifter owner must say why.

Codex (lifts, no fixture): 30

| Reached from | Depth | Function | Name | Bytes | Called from | Note |
|---|---:|---|---|---:|---|---|
| aTitle state 1 | 3 | 0x004918D0 | FUN_004918d0 | 1596 | 0x004960E0 |  |
| aTitle vf6 | 2 | 0x004755E0 | FUN_004755e0 | 132 | 0x0043C580 |  |
| aTitle vf6 | 2 | 0x0048AC80 | FUN_0048ac80 | 1973 | 0x004965A0 |  |
| aTitle vf6 | 2 | 0x00494D80 | FUN_00494d80 | 976 | 0x004965A0 |  |
| aTitle vf6 | 6 | 0x0081C330 | FUN_0081c330 | 806 | 0x0081BA40 |  |
| main loop | 2 | 0x00DAE760 | FUN_00dae760 | 595 | 0x00DAF7C0 |  |
| render worker | 1 | 0x00DAA4B0 | FUN_00daa4b0 | 142 | 0x00DAC090 |  |
| sBbsRpgMain ctor | 1 | 0x00475400 | ctor | 400 | 0x0041B400 |  |
| sBbsRpgMain ctor | 1 | 0x00478600 | ctor | 649 | 0x0041B400 |  |
| sBbsRpgMain ctor | 1 | 0x0049F990 | ctor | 1255 | 0x0041B400 |  |
| sBbsRpgMain ctor | 2 | 0x00480240 | ctor | 222 | 0x004821A0 |  |
| sBbsRpgMain ctor | 2 | 0x0075BAD0 | FUN_0075bad0 | 909 | 0x00437BE0 |  |
| sBbsRpgMain ctor | 2 | 0x00DB8B90 | FUN_00db8b90 | 73 | 0x0044F500 |  |
| sBbsRpgMain ctor | 2 | 0x00DD5ED0 | FUN_00dd5ed0 | 1575 | 0x004AC8A0 |  |
| sBbsRpgMain ctor | 3 | 0x00419100 | FUN_00419100 | 50 | 0x00417AF0 |  |
| sBbsRpgMain ctor | 3 | 0x00480950 | FUN_00480950 | 565 | 0x00482760 |  |
| sBbsRpgMain ctor | 4 | 0x00490E80 | ctor | 412 | 0x00495FF0 |  |
| sBbsRpgMain ctor | 4 | 0x004915A0 | ctor | 746 | 0x00495FF0 |  |
| sBbsRpgMain ctor | 5 | 0x01104470 | ctor | 267 | 0x01105240 |  |
| sBbsRpgMain::vf06 (frame) | 2 | 0x01128EE0 | FUN_01128ee0 | 95 | 0x00DA9CD0 |  |
| sBbsRpgMain::vf06 (frame) | 2 | 0x01128F40 | FUN_01128f40 | 33 | 0x00DAED10 |  |
| sBbsRpgMain::vf06 (frame) | 2 | 0x01129310 | FUN_01129310 | 95 | 0x00DA9CD0 |  |
| sBbsRpgMain::vf06 (frame) | 2 | 0x01129370 | FUN_01129370 | 33 | 0x00DAED10 |  |
| sBbsRpgMain::vf06 (frame) | 4 | 0x00491340 | FUN_00491340 | 263 | 0x00498C20 |  |
| sBbsRpgMain::vf06 (frame) | 4 | 0x00493AA0 | FUN_00493aa0 | 3327 | 0x004964C0 |  |
| sBbsRpgMain::vf06 (frame) | 4 | 0x004A06B0 | FUN_004a06b0 | 481 | 0x004964C0 |  |
| sRender ctor | 2 | 0x00DA9EC0 | FUN_00da9ec0 | 1509 | 0x00DB1480 |  |
| sRender ctor | 2 | 0x01129030 | FUN_01129030 | 253 | 0x01129130 |  |
| sRender ctor | 2 | 0x01129460 | FUN_01129460 | 226 | 0x01129550 |  |
| sRender ctor | 4 | 0x00DAFED0 | FUN_00dafed0 | 883 | 0x00DB0410 |  |

claude-6 (not in the lift set; triage): 12

| Reached from | Depth | Function | Name | Bytes | Called from | Note |
|---|---:|---|---|---:|---|---|
| WinMain | 3 | 0x012ED9DD | ___crtExitProcess | 23 | 0x012EDAF5 | (status documented) |
| aTitle state 1 | 3 | 0x0040DDC0 | FUN_0040ddc0 | 212 | 0x0040DB40 | (status documented) |
| aTitle state 1 | 4 | 0x012E1A60 | FUN_012e1a60 | 47 | 0x0040DC50 | (status documented) |
| sBbsRpgMain ctor | 1 | 0x004AB440 | FUN_004ab440 | 285 | 0x0041B400 | (status documented) |
| sBbsRpgMain ctor | 1 | 0x00DFECF0 | FUN_00dfecf0 | 23652 | 0x0041B400 | (status documented) |
| sBbsRpgMain ctor | 2 | 0x00E0E470 | FUN_00e0e470 | 292 | 0x00E121D0 | (status documented) |
| sBbsRpgMain ctor | 3 | 0x00F9FBA0 | FUN_00f9fba0 | 288 | 0x00FA15E0 | (status documented) |
| sBbsRpgMain ctor | 3 | 0x012E1A13 | FUN_012e1a13 | 47 | 0x004201A0 | (status documented) |
| sBbsRpgMain ctor | 3 | 0x012F9D82 | FUN_012f9d82 | 253 | 0x012EFEA0 | (status documented) |
| sBbsRpgMain ctor | 5 | 0x012F1C9D | FUN_012f1c9d | 453 | 0x012F1C40 | (status documented) |
| sBbsRpgMain::vf06 (frame) | 6 | 0x00448500 | FUN_00448500 | 1936 | 0x00448DA0 | (status translated) |
| sRender ctor | 3 | 0x004080A0 | FUN_004080a0 | 42 | 0x00DB0DE0 | (status documented) |

### Look-ahead along vtable and pointer routes

The direct-call closure misses virtual and pointer dispatch, so it was
extended (same inventory, read-only, plus the installed exe's bytes):
- A function on the path reaches every inventory function whose address
  it holds as an immediate or absolute displacement (thread starts,
  callbacks, comparators).
- It also reaches every slot of each vtable it refers to: a `.rdata`
  address followed by consecutive words that are inventory functions in
  `.text`, DTI.md's length rule.
- `aInitialize`'s (0x0156CD80) and `aTitle`'s (0x0156F7A8) vtable slots
  are seeds.
- Each extra pointer or vtable step is one "hop"; routes still stop at
  their first unverified function.

| Hops allowed | Functions reached | Unverified (Codex / claude-6) |
|---:|---:|---|
| 0 | 1,673 | 42 (30 / 12): the tables above |
| 1 | 7,711 | 93 (64 / 29) |
| 2 | 14,304 | 138 (94 / 44) |

At two hops the closure already covers about a quarter of the program
(and half of all 283 unverified functions): every vtable a constructor
installs counts as fully used. So the one-hop additions below are the
likely next stops, the two-hop ones are candidates, and a third hop would
add little. "Reached through" gives the verified function the route last
passed and how it got there. 0x00520003/0x00520004 are odd-aligned
supplementary entries met only as immediates: probably not real call
targets, worth a check before queueing.

Imports on the two-hop routes: 194, all with a service except
`wsprintfA` (0x00DA3300 and 0x00DA3650, format `"%S"`). `ntohs` and
`WSAGetLastError` are bound by ordinal in `net.rs`. `wsprintfA` is now a
service for literals, `%%` and `%S`, measured on Windows 11
(`research/runtime/windows/Wsprintf.cs`): `%S` converts each unit as
WideCharToMultiByte(1252, 0) does, and the 1024-character limit drops an
argument that does not end below it, which ends the output. Other
conversions stop.

One pointer/vtable hop (likely on the path): 51

Codex (34):

| Function | Name | Bytes | Reached through | Note |
|---|---|---:|---|---|
| 0x0046A150 | FUN_0046a150 | 366 | 0x00469F80, call |  |
| 0x0046A2D0 | FUN_0046a2d0 | 609 | 0x00469C90, call |  |
| 0x00478050 | FUN_00478050 | 180 | 0x00501020, call |  |
| 0x0048AB50 | FUN_0048ab50 | 294 | 0x00496410, call |  |
| 0x0048CC00 | FUN_0048cc00 | 332 | 0x004918A0, call |  |
| 0x0048F330 | FUN_0048f330 | 713 | 0x00496410, call |  |
| 0x00495150 | FUN_00495150 | 964 | 0x00499060, call |  |
| 0x00495520 | FUN_00495520 | 2706 | 0x00496410, call |  |
| 0x004A0C00 | FUN_004a0c00 | 531 | 0x00499060, call |  |
| 0x0050A110 | FUN_0050a110 | 2612 | 0x00504B40, call |  |
| 0x00520003 | entry_00520003 | 219 | 0x00444C20, pointer |  |
| 0x00520004 | entry_00520004 | 218 | 0x00444C20, pointer |  |
| 0x005EC920 | FUN_005ec920 | 76 | 0x005EF790, call |  |
| 0x00606090 | FUN_00606090 | 98 | 0x006BB0C0, call |  |
| 0x00606130 | FUN_00606130 | 123 | 0x005F0830, call |  |
| 0x006066E0 | FUN_006066e0 | 62 | 0x006A4E20, call |  |
| 0x00620000 | entry_00620000 | 53 | 0x00444C20, pointer |  |
| 0x00620003 | entry_00620003 | 170 | 0x00444C20, pointer |  |
| 0x00620004 | entry_00620004 | 169 | 0x00444C20, pointer |  |
| 0x006335A0 | FUN_006335a0 | 104 | 0x006344F0, call |  |
| 0x00633EF0 | FUN_00633ef0 | 406 | 0x00413C80, call |  |
| 0x00670002 | entry_00670002 | 9 | 0x00444C20, pointer |  |
| 0x00670003 | entry_00670003 | 8 | 0x00444C20, pointer |  |
| 0x007A0003 | entry_007A0003 | 371 | 0x00444C20, pointer |  |
| 0x007A0004 | entry_007A0004 | 370 | 0x00444C20, pointer |  |
| 0x007D0005 | entry_007D0005 | 103 | 0x00444C20, pointer |  |
| 0x00815C60 | FUN_00815c60 | 18178 | 0x00812820, call |  |
| 0x00C40000 | entry_00C40000 | 342 | 0x00444C20, pointer |  |
| 0x00C9B350 | FUN_00c9b350 | 1859 | 0x0046B690, call |  |
| 0x00C9E0F0 | FUN_00c9e0f0 | 259 | 0x00C9DDF0, call |  |
| 0x00D29AF0 | FUN_00d29af0 | 1282 | 0x00C9EE90, call |  |
| 0x00DBA780 | FUN_00dba780 | 426 | 0x00488B70, call |  |
| 0x00F7DE40 | FUN_00f7de40 | 108 | 0x00F7E1D0, call |  |
| 0x011590E0 | FUN_011590e0 | 110 | 0x01159260, call |  |

claude-6 (17):

| Function | Name | Bytes | Reached through | Note |
|---|---|---:|---|---|
| 0x00408100 | entry_00408100 | 16 | 0x00DB0DE0, vtable 0x01559568 slot 2 | not in the lift set (status untouch |
| 0x004403A0 | FUN_004403a0 | 2691 | 0x0043CEB0, call | not in the lift set (status documen |
| 0x004709D0 | FUN_004709d0 | 288 | 0x004704E0, call | not in the lift set (status documen |
| 0x004734E0 | FUN_004734e0 | 418 | 0x00471D30, call | not in the lift set (status documen |
| 0x0047DC60 | FUN_0047dc60 | 394 | 0x0047D610, call | not in the lift set (status untouch |
| 0x004A2D30 | FUN_004a2d30 | 1313 | 0x004A21B0, call | not in the lift set (status untouch |
| 0x004E0004 | entry_004E0004 | 41 | 0x00444C20, pointer | not in the lift set (status documen |
| 0x004E0017 | entry_004E0017 | 22 | 0x004861E0, pointer | not in the lift set (status untouch |
| 0x00740000 | entry_00740000 | 1 | 0x00444C20, pointer | not in the lift set (status documen |
| 0x00740003 | entry_00740003 | 1 | 0x00444C20, pointer | not in the lift set (status untouch |
| 0x00740004 | entry_00740004 | 1 | 0x00444C20, pointer | not in the lift set (status untouch |
| 0x007D0003 | entry_007D0003 | 105 | 0x00444C20, pointer | not in the lift set (status untouch |
| 0x00D0C590 | FUN_00d0c590 | 577 | 0x00DBD750, call | not in the lift set (status documen |
| 0x00D8BCE0 | FUN_00d8bce0 | 469 | 0x00D08B40, call | not in the lift set (status documen |
| 0x00FEE130 | FUN_00fee130 | 480 | 0x00DAF000, call | not in the lift set (status documen |
| 0x012EE1A1 | vf14 | 42 | 0x0041B400, vtable 0x0155ace4 slot 21 | not in the lift set (status documen |
| 0x01304753 | FUN_01304753 | 327 | 0x01304690, call | not in the lift set (status documen |


Two hops (wider candidates): 45

Codex (30):

| Function | Name | Bytes | Reached through | Note |
|---|---|---:|---|---|
| 0x00416740 | FUN_00416740 | 378 | 0x00737900, call |  |
| 0x004EE3E0 | FUN_004ee3e0 | 437 | 0x004EE3B0, call |  |
| 0x004EE720 | vf05 | 1853 | 0x004EE180, vtable 0x0156bda8 slot 5 |  |
| 0x005F3450 | vf05 | 301 | 0x00E0CE80, vtable 0x0157e4c0 slot 55 |  |
| 0x006076F0 | FUN_006076f0 | 533 | 0x006E9B50, call |  |
| 0x00633A70 | FUN_00633a70 | 253 | 0x00633260, call |  |
| 0x00633B70 | FUN_00633b70 | 167 | 0x00633260, call |  |
| 0x0064DD20 | vf58 | 315 | 0x0064DC90, vtable 0x015811a0 slot 58 |  |
| 0x006692F0 | vf59 | 488 | 0x006677D0, vtable 0x01582508 slot 59 |  |
| 0x00677930 | vf60 | 647 | 0x00677880, vtable 0x01582f38 slot 60 |  |
| 0x00685030 | FUN_00685030 | 242 | 0x00685130, call |  |
| 0x00689960 | FUN_00689960 | 1944 | 0x006894C0, call |  |
| 0x0068A9F0 | FUN_0068a9f0 | 133 | 0x0068B340, call |  |
| 0x00691E50 | FUN_00691e50 | 816 | 0x006894C0, call |  |
| 0x00692A40 | FUN_00692a40 | 737 | 0x006928E0, call |  |
| 0x006A1770 | FUN_006a1770 | 168 | 0x006A1550, call |  |
| 0x006E0EB0 | vf05 | 310 | 0x006E4A00, call |  |
| 0x006F1590 | FUN_006f1590 | 223 | 0x006F04B0, call |  |
| 0x00705E00 | FUN_00705e00 | 462 | 0x00705010, call |  |
| 0x007BEA60 | FUN_007bea60 | 373 | 0x007BF180, call |  |
| 0x00811A90 | FUN_00811a90 | 2199 | 0x00812980, call |  |
| 0x00860A60 | vf25 | 27 | 0x00860660, vtable 0x0159ea38 slot 25 |  |
| 0x00874F90 | vf25 | 24 | 0x00859660, vtable 0x0159e760 slot 25 |  |
| 0x00B54BE0 | vf25 | 27 | 0x0078A5B0, vtable 0x015e87c0 slot 25 |  |
| 0x00C9BAA0 | FUN_00c9baa0 | 308 | 0x00737900, call |  |
| 0x00DC7D00 | FUN_00dc7d00 | 127 | 0x00DDDCC0, vtable 0x0142b0d8 slot 14 |  |
| 0x00DC8270 | FUN_00dc8270 | 935 | 0x00DD1F60, call |  |
| 0x00FB9A10 | vf14 | 86 | 0x00FC8AE0, vtable 0x01438cb8 slot 116 |  |
| 0x011251F0 | FUN_011251f0 | 943 | 0x011EC4C0, call |  |
| 0x011591C0 | vf11 | 93 | 0x01159260, vtable 0x01443fec slot 11 |  |

claude-6 (15):

| Function | Name | Bytes | Reached through | Note |
|---|---|---:|---|---|
| 0x00410000 | vf10 | 136 | 0x010784B0, pointer | not in the lift set (status untouch |
| 0x00476D60 | FUN_00476d60 | 425 | 0x00435A10, call | not in the lift set (status untouch |
| 0x00534658 | entry_00534658 | 256 | 0x00D0C7E0, pointer | not in the lift set (status documen |
| 0x0064E2B0 | vf05 | 1887 | 0x0064DFE0, vtable 0x01581348 slot 5 | not in the lift set (status documen |
| 0x0066006E | entry_0066006E | 11 | 0x00CFFB00, vtable 0x01407180 slot 35 | not in the lift set (status untouch |
| 0x00670069 | entry_00670069 | 516 | 0x00CFFB00, vtable 0x01407180 slot 36 | not in the lift set (status untouch |
| 0x0069002E | entry_0069002E | 46 | 0x00CFFB00, vtable 0x01407180 slot 37 | not in the lift set (status untouch |
| 0x0069006E | entry_0069006E | 260 | 0x00CFFB00, vtable 0x01407180 slot 38 | not in the lift set (status untouch |
| 0x006F0063 | entry_006F0063 | 1 | 0x00CFFB00, vtable 0x01407180 slot 34 | not in the lift set (status documen |
| 0x007DF790 | FUN_007df790 | 933 | 0x007DCF30, call | not in the lift set (status documen |
| 0x00DBC3B0 | FUN_00dbc3b0 | 400 | 0x00DBC550, pointer | not in the lift set (status untouch |
| 0x00E0D750 | vf10 | 136 | 0x00E0D1A0, vtable 0x0142e220 slot 10 | not in the lift set (status documen |
| 0x00E0D940 | vf10 | 1599 | 0x00E0D240, vtable 0x0142e278 slot 31 | not in the lift set (status documen |
| 0x0109DAC0 | FUN_0109dac0 | 10305 | 0x010A8570, call | not in the lift set (status untouch |
| 0x010A3C00 | FUN_010a3c00 | 10283 | 0x010A8920, call | not in the lift set (status untouch |

8. main e3d0fdd6 (train 3), previewed on Codex's frozen `native-L28c`
   (59,534 functions generated locally, not committed), which records
   0x00DAFED0. Runtime game tests 46/46. The real boot passes 0x00DAFED0.
   Stop: untranslated 0x00DA9EC0, called from just before 0x00DB187E (in
   0x00DB1480, `sRender`'s set-up). Cause 2: it lifts but has no fixture
   (`lift_novector.json` in native-L28c). Owner: Codex. It was in the
   look-ahead table (sRender ctor, depth 2).
   The frontier probe's 0x01305585 is the CRT's `_fptrap` (push 2; call
   `__amsg_exit`), reached through the `__cfltcvt_tab` slots that
   `_FPinit` replaces (claude-6). The probe skips `_FPinit`, so it is not
   a real-boot issue.
9. Preview on native-L28c (59,547 records, with Codex's 0x00DA9EC0,
   0x01129030, 0x01129460, 0x01104470; generated locally and restored).
   Runtime game tests 46/46. The boot passes all of `sRender`'s set-up.
   Stop: untranslated 0x00419100, called from just before 0x00417AF6 (the
   archive manager set-up 0x00417AF0 in `sBbsRpgMain`'s constructor). It
   is a 50-byte loop that registers ten tables of (id, entry) pairs at
   0x018D9288. Cause 2: no fixture in native-L28c. Owner: Codex.
   - Ten more constructor-route functions from the look-ahead are also
     still without fixtures there: 0x00475400, 0x00478600, 0x0049F990,
     0x00480240, 0x00490E80, 0x004915A0, 0x00DD5ED0, 0x0075BAD0,
     0x00DB8B90 and 0x00480950.
   - claude-6's not-lifted group on the same route is unchanged:
     0x004AB440, 0x00DFECF0, 0x00E0E470, 0x00F9FBA0, 0x012E1A13,
     0x012F9D82, 0x012F1C9D.
10. Preview on native-L28c (59,559 records: Codex's 0x00419100 and 11
    constructor/title functions; generated locally, restored). Runtime
    game tests 46/46. The boot passes the archive manager and more of
    `sBbsRpgMain`'s constructor.
    - Stop: "read of undefined x86 flag 0x1" (CF, the lifter's PARTIAL flag
      state) in the lifted CRT `_qsort` 0x012EEB50. Guest frames: _qsort ←
      0x00E0F590 ← 0x00E11E30 ← `sShader2`'s constructor 0x00E121D0 ←
      `sBbsRpgMain`'s constructor 0x0041B400 ← 0x0041B3C0 ← app init
      0x00DF3AC0 ← WinMain. Registers at the fault: eax 0xA89, ecx 0xA8A,
      ebx 4, esp 0x7EFFD008, ebp 0x7EFFD114, esi 0x653E9654,
      edi 0x653E6C30.
    - 0x00E0F590 sorts 4-byte entries with the comparator 0x00E0F520
      (verified; CMP then SBB, well-formed). In `_qsort` and `__shortsort`
      (0x012EEAC0) every CF read follows a CMP, so the source document
      (the disassembly) shows no undefined read. The fault comes from the
      lifted body's flag model (e.g. after DIV / INC, after the comparator
      call, or a branch join). Cause 2 (lifter). Owner: claude-6.
11. Scratch preview (not committed): task/L21-boot + task/L46-x87-state
    (c61bbf35, INC/DEC leave an undefined CF undefined), generated from
    native-L28c. Runtime game tests 46/46. The boot passes `_qsort`.
    - L27b's per-instruction steps are opt-in since 24c4f5c4
      (LIFT_INSTRUCTIONS=1). Before the head's b84b3e44, a gen without
      steps stopped every lifted call: `Registry::call` sent every bound
      target into the step executor, which knows only steps and services,
      so `__heap_init` stopped with MissingInstruction 0x012FE354. The run
      used a local one-line fallback; main now has the real fix.
    - Stop: untranslated 0x00E0FB9C, reached by a jump, so there is no
      return address (ecx 0x653343D0, esp 0x7EFFB348, eax 0xA8A). The
      record for 0x00E0FB80 (`this` in ecx, one stack argument, `ret 4`;
      it searches the array at [this+0x28], count [this+0x34], for
      arg & ~0xFFF) lifted only its 28-byte head. `jne 0x00E0FB9C` leaves
      for the chunk that Ghidra splits off as a function of its own.
      native-L28c's verification gaps list it as "stale body: 73
      instructions from 0x00E0FB9C not lifted". Cause 2 (lifter). Owner:
      claude-6 (L46's frame-tail records). Fixed in native-L46 (84
      instructions, the chunk local).
    - Its six direct callers are all virtual (vf10 0x00F94480, 0x00FA8FE0,
      0x010082D0, 0x0117D310; vf09 0x0112E4D0; 0x0117D1D0), so the static
      look-ahead, which follows vtables only from its known roots, did not
      reach it.
    - The look-ahead (`research/runtime/title_lookahead.py`; LIFT_DATA
      selects the data set) now also lists the "verified" functions whose
      record still has a verification gap. On native-L28c with two hops:
      30 of 82.
      - 16 stale bodies, all in the CRT: the two `memcpy` variants
        0x012EDE40/0x012EFEA0 (`__VEC_memcpy` tails, claude-6), plus
        `__initptd`, `___updatetmbcinfo`, `__write`, `___lock_fhandle`,
        `_fclose`, `__read`, `__close`, `_raise`,
        `__IsNonwritableInCurrentImage`, `___DestructExceptionObject`,
        `__getstream`, `__alloc_osfhnd`, `__sopen_helper` and 0x01302F75.
        These are 5 to 8 instructions each, mostly error paths.
      - 13 unproven jump tables (0x0044CAC0, 0x004593E0, 0x004298B0,
        0x004606A0, 0x004B72B0, 0x00789010, 0x00797BA0, 0x00BC1A00,
        0x00FA0130, 0x00441FB0, 0x0044D1F0, 0x004594A0, 0x0047A2A0).
      - One "tail call into its own inlined code" (0x00786CA0).
12. Scratch preview (not committed): main + task/L21-boot +
    task/L46-x87-state. Generated from the head's merged preview data,
    native-preview5 (59,572 records: main, claude-6's native-L46 and
    Codex's native-L28c). gen writes `patterns/verification_gaps.json`
    into LIFT_DATA, so it ran on a hard-linked copy. Runtime game tests
    46/46. The boot passes 0x00E0FB80 on main's executor, and more of
    `sBbsRpgMain`'s constructor.
    - Stop: untranslated 0x00DB8B90, called from just before 0x0044F67A
      (constructor 0x0044F500). Registers: eax = ecx = edi = 0x68000060,
      esp 0x7EFFD19C, ebp 0x7EFFD1A8.
    - 0x00DB8B90 is a 73-byte base constructor (vtable 0x0142A428) with
      20+ callers, and it has no fixture (`lift_novector.json`).
    - Codex's L28 Results give the cause. It pushes one argument for
      0x00DB8AC0. That function's normal exit reuses the argument slot
      and tail-jumps to LeaveCriticalSection through IAT 0x0139D164,
      which pops four bytes. Its failure path calls 0x004080A0 (which
      calls `__invoke_watson`, then INT3), so the recorder's static
      cleanup analysis returns zero.
    - Cause 2. Owners: claude-6 (noreturn evidence for 0x004080A0), then
      Codex (the recording).
