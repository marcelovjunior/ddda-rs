# From WinMain to the title screen's first frame

Brief: [L35](tasks/L35-title-path.md).

What the original does between WinMain and the first frames of the title
screen, and what the rebuild still lacks on that path:
- missing runtime services;
- functions not yet lifted and verified;
- conditions that decide whether a branch runs.

This is a static map (Ghidra, the inventory, the lifted sources); nothing
was run.

It starts where claude-2's boot (L21) stands. The CRT stops at
`__setmbcp` 0x012FB049 inside `__setargv`; claude-3's L28 lifts the CRT's
116 platform functions.

**Method.**
- **The call graph.** The inventory's `calls` table holds direct calls,
  including calls to imports (Ghidra's `EXTERNAL` ids) and to the DLL
  thunks (`thunk = 1`: D3D9, D3DX, DINPUT8, XInput, IMM32, PSAPI).
- **Virtual dispatch** (frames, areas, units) was added as roots:
  - every call listed in FRAME.md section 2;
  - `sBbsRpgMain`'s constructor;
  - the vtable slots of the three areas on the path;
  - the methods of the GUI classes they create.
- **This "likely path" closure** has 3,594 functions:

  | Status | Functions |
  |---|---|
  | verified | 3,276 |
  | CRT platform | 43 |
  | untouched | 62 |
  | documented | 26 |
  | translated | 13 |

  Plus 174 imports.
- **A broad over-approximation** adds every `s*` and `r*` class's methods:
  8,176 functions, 166 not verified.
- **Why each missing function is missing:** the lifter's own `lift()`
  was run on every non-verified function of the likely path (read-only,
  `research/native/lift.py`).
- **Imports** are compared with the runtime's binding tables on main
  (`crates/ddruntime/src`, by name and by ordinal; delay-loaded WMVCore
  through `movies.rs`).

The scripts and lists are private (`C:\ddda-targets\L35`).

## 1. The ordered map

### 1.1 WinMain (0x004079E0)

1. `LoadLibraryA("WMVCORE.DLL")` then `FreeLibrary`. If it fails:
   `MessageBoxA` and `_exit(0)`. Covered (L18 provides WMVCore).
2. `SteamAPI_Init`; on failure `MessageBoxA`, return 1. Then
   `SteamUtils()->vf+0x40(cSetInfoCoord::vf09)` (the warning hook).
   Covered (L26).
3. **`sApp::ctor`** (0x00DF3940):
   - `GetCurrentThreadId`, `GetTickCount`;
   - 0x00D0CB50 with the thread callback 0x00DF0C50;
   - **0x00DF2F30**: `CoInitialize`, and the file set-up (0x00DF1D00 →
     `MtFile` 0x00D0E450).
4. 0x00DF0B90, then the AI work heaps `0x00CF5E60(0x19, "AI_WORK")` and
   `0x00CF5E90(0x1A)`.
5. Allocators: 0x00740590, then 0x00740860.
   - `MtVirtualAllocator` reserves 512 MB and commits inside it.
   - `0x00740590` creates the heaps (about 244 MB).
   - claude-2 reworked `VirtualAlloc`/`VirtualFree` for this (L21
     2ea6e54, untested).
6. **App setup** `0x00DF3AC0` (1.2). On failure: `SteamAPI_Shutdown`,
   0x00DF1140, return 1.
7. **The main loop** `0x00DF0D20` (FRAME.md 1; `timeBeginPeriod`).
8. Shutdown: 0x00DF1140 (reaches `CreateProcessA` through
   0x01179A70/0x011797F0), then `SteamAPI_Shutdown`.

### 1.2 App setup (0x00DF3AC0)

1. `GetCurrentDirectoryA`, then 0x011799A0.
2. **Single instance.** `CreateMutexA(name)`; on `ERROR_ALREADY_EXISTS`
   (0xB7), `FindWindowW` + `ShowWindow(9)` on the running game and quit.
3. **Window class and config** `0x00DF31F0`:
   - `config.ini` under LocalAppData (claude-2's boot notes; `SHGetFolderPathW`
     0x00CFFF00, `GetPrivateProfileStringW` 0x00D00010 ← 0x00D00120 ← 0x00DF31F0);
   - DISPLAY/Resolution, default 1280×720.
4. **Window** `0x00DF1200`:
   - `CreateWindowExW` (the main window; a second window `"SubDisplay"`
     is created only on a condition, not traced);
   - then **`0x01106C80`, unconditionally**: the IME setup.
     `ImmGetDefaultIMEWnd`, `ImmGetContext`, `ImmSetCompositionWindow`,
     `ImmSetCandidateWindow`, `ImmReleaseContext`, and through 0x01106B30
     `ImmSetOpenStatus`;
   - then `ShowWindow(1)`, `UpdateWindow`;
   - `RegisterDeviceNotificationA` is in 0x00DF1550, which has no
     direct caller (*inferred*: the window-procedure side).
5. `LoadAcceleratorsA("XApp")`.
6. **`sMain = new sBbsRpgMain`** (through `sBbsRpgMain::DTI` vf+4), stored
   at `sApp+0x2558` (1.3).
7. 0x00DF39E0(0xFFFF), 0x00DF2CA0.

### 1.3 `sBbsRpgMain::ctor` (0x0041B400)

1. **`sMain`'s base constructor** 0x00DBE810 (direct callees from the
   inventory):
   - `mFps` 60, `mMaxFps` 3000, the `MaxFPS` option;
   - the worker threads `0x00DBDFE0` → `0x00DBDF10` (the job threads on
     `0x00DBDEF0` start later, from the job runner 0x00DBE560);
   - **CPU feature detection** `0x00DBD170`: CPUID (0x00DBD140) and a
     `pushfd` probe (0x00DBD110);
   - QPC (0x0130A5A0) and its frequency (**0x0130A5C0**);
   - if the CPU detection did not set the SSE2 flag (`+0x700E & 8`):
     **0x00D0CB00("ERR02 : Could not use SSE2 instructions.")**, a fatal
     error reporter that ends in `_exit(1)`. So the runtime's CPUID
     answer must report SSE2.
2. The engine singletons, through their creators. Among them:
   - `sRender` (0x00DB1A50 via 0x004889A0): the render thread
     `0x00DAC090` (where the device is created is not traced here);
   - `sResource` (0x00DBB690): the loader threads `0x00DBA9A0`,
     `0x00DB9F40`;
   - the sound engine 0x00DD5ED0 via 0x004AC8A0: XAudio2 through
     `CoCreateInstance` in 0x00DCE0E0 / 0x00DCE600.
3. **The 49 game singletons**, in source order:
   `sAICharacterInfo`, `sAICtrlManager`, `sAIMessageCtrl`,
   `sAISensorTarget`, `sAISituationBoard`, `sAIStudyCtrl`, `sAreaExt`,
   `sAreaHit`, `sCameraExt`, `sCollisionExt`, `sDDCaProto`, `sEffectExt`,
   `sEnemyManager`, `sEnvMapManager`, `sEventManager`, `sGUIExt`,
   `sGameSys`, `sGeneralPoint`, `sHumanEnemyManager`, `sIdCockpit`,
   `sIdMessage`, `sItemManager`, `sLockOnManager`, `sNpcIntelManager`,
   `sNpcManager`, `sObjCollision`, `sPawnManager`, `sPlayerManager`,
   `sQlocInput`, `sQuestManager`, `sRecognition`, `sSave`, `sScenario`,
   `sSceneExt`, `sScrOmManager`, `sSetManager`, `sShader2Ext`,
   `sShlManager`, `sSoftBody`, `sSoundExt`, `sSoundManager`,
   `sStorageExt`, `sToolExt`, `sVibrationExt`, `sWeatherManager`,
   `sWorkRate`, `sWorldOffset`, `cStartInfo`, and the first area
   **`aInitialize`**.
4. The Steam callbacks: 0x004083A0 → `SteamAPI_RegisterCallback`, and
   0x00DFA0C0 (an XInput pad function). The keyboard (`sKeyboard`'s
   constructor 0x00DF43E0: DirectInput and the low-level hook
   `SetWindowsHookExA`).
5. **Threads:** `CreateThread(0x0041FFE0)` running at once (0x0041B400 at
   +0x1665 of the decompile), then 0x0041FD70 (suspended workers on
   `0x0041FE60`).
6. `mFps = 30.0` at 0x0041CEC9 (FRAME_RATE.md 1).

### 1.4 The frame

`sBbsRpgMain::vf06` (0x0041DD60) in FRAME.md section 2's order. Each
frame:
- `sMain::vf06` (the clock; QPC through 0x0130A5A0, the frequency
  through 0x0130A5C0);
- the device check `0x00DAF7C0` (`TestCooperativeLevel`; the device-lost
  path sleeps);
- the singletons' updates, the unit passes;
- render 0x00DAF000, Present.

The areas run as units under `sAreaExt::vf06` (step 20).

### 1.5 `aInitialize` (vtable 0x0156CD80)

Slots 5-8: 0x004FC270, 0x004FC340, 0x004FC370, 0x004FC450. Slots 6-8 are
not functions in Ghidra but are verified supplementary entries.

1. **Slot 5, the logo.**
   - Allocates an archive request in `sArchiveManager` (`*0x018D9280`,
     *inferred* from its writer 0x004179C0 beside
     `sArchiveManager::vf06`) with 0x00418550, adds group 4
     (0x00418810), and starts it (0x00418610).
   - Creates **`uGUICapcomLogo`** (logos: `id\menu\logos\caplogo`,
     `capcom_ID`, `MTfw_ID`, `fwlogo`, `esrb`, `cerod`, `caution`).
   - **Waits** until that request's state is 10 (loaded), then
     `0x00438960`: a second request (group 2) and the GUIs
     `uGUIProgress`, `uGUINoticeBoard2`, `uGUIOption`.
2. **Slot 6** 0x004FC340 resets the frame (`sMain` vf 0x00DBD710), the
   sound (0x004AD4A0) and an `sSave` field.
3. **Slot 7** 0x004FC370 waits for its unit (+0x34) to reach state 1,
   then **switches to `aTitle`**: `sArea (*0x018D099C)->vf+0x28(aTitle::DTI
   0x0198D658)`.
4. **Slot 8** 0x004FC450: shader set-up (`sShader2`, 0x00E127D0); it
   releases the archive request.

**Files:** `rom\Initialize.arc` (468 KB), `rom\gui\GUIuGUICapcomLogo.arc`,
and the shader packages through `sShader2` (`sa\DX9\*.arc`).

### 1.6 `aTitle` (vtable 0x0156F7A8)

Slots 5-8: 0x00531290, 0x00531400, 0x005317B0, 0x00533360.

**Slot 6 (0x00531400)** sets the first state to 0x00531800:
- the save system: 0x004965A0, 0x0043C580;
- the sound requests `rSoundRequest`, `rSoundStreamRequest`;
- the time zone (0x00D25470 → CRT `__tzset`).

**The first state, 0x00531800:**
- creates **`uGUITitle`** on unit line 0x1A;
- calls 0x00531760, and an `*0x018FA518` call (vf+0x14);
- **unless `+0x38` is set** (*inferred*: set when returning from the
  game), creates **`uMovieExt`**, the title movie from the table at
  0x01875610: `movie\title\title_A` / `title_B` (`.wmv`).

**Files:**
- `rom\title.arc` (13.5 MB);
- `rom\gui\GUIuGUITitle.arc`;
- `movie\title\title_A.wmv`, `title_B.wmv`;
- the menu layouts `id\menu\title_2\...`, `id\DDN\menu\title_1\top_01`.

The first title frame is the first Present after `uGUITitle` and the
movie unit run.

### 1.7 Threads on the path

| Entry | Started by | Status |
|---|---|---|
| 0x00DAC090 render worker | `sRender` ctor 0x00DB1A50 | verified (L17 B/C) |
| 0x00DBA9A0, 0x00DB9F40 resource loaders | `sResource` ctor 0x00DBB690 | verified |
| 0x00DBDEF0, 0x00DBDF10 job workers | `sMain` 0x00DBDF30 / 0x00DBDFE0 | verified |
| 0x00CFE6B0 `MtThread` | 0x00CFE700 | verified |
| 0x0041FFE0 | `sBbsRpgMain` ctor (runs at once) | verified |
| 0x0041FE60 workers | 0x0041FD70 | verified |
| 0x011A4610 → 0x0041FFE0 | | verified |

Their closures add few gaps: `0x004080A0` (lifts now) and `0x00DAA4B0`
(render worker; lifts now); the rest are CRT error and logging paths.

## 2. What is missing, by owner

### 2.1 Lifter and recorder: functions not in the lifted set

None of the likely path's 101 non-verified functions is in the shared
lifted set (`crates/ddlift`, 58,989 functions). The runtime stops at the
first one it reaches.

**80 lift with today's lifter.** 35 lift cleanly; 45 call other unlifted
code (`ext`). They are absent only because they were not in the last
lift run's input. Lifting them and recording vectors is a pipeline job
(claude-3, or the head's next relift). In boot order:

1. **Window:** `0x01106C80` (IME setup; the **first** lifter stop after
   the CRT).
1b. **`sMain`'s base constructor:** 0x0130A5C0, beside the CPUID
   rejections below. (0x00D0CB00, also called there, is the fatal-error
   reporter: reached only when SSE2 is missing.)
2. **`sBbsRpgMain` ctor:**
   - 0x004083A0 (Steam callbacks) and 0x00DFA0C0;
   - 0x00419100 (archive manager);
   - the constructors of `sNpcManager` 0x00475400, `sObjCollision`
     0x00478600, `sSave` 0x004980F0, `sSetManager` 0x0049F990 (+
     0x004A2A40) and `sShader2` 0x00E121D0;
   - 0x00DA9EC0 (`sRender`);
   - the sound engine 0x00DD5ED0, 0x00DC7D00, 0x00DC8270, 0x00DCE2E0;
   - 0x00DFECF0, 0x00F9FBA0.
3. **Main loop and frame:**
   - render 0x00DAE760, 0x00DAA4B0;
   - **`sMain::vf06`'s 0x0130A5C0** (QPC frequency; every frame, and
     already in the constructor);
   - **`sSoundExt::vf06` 0x004AD440** (every frame);
   - `sGameSys` 0x004403A0, 0x0044E6F0;
   - input 0x00E04A90 (`sMouse`);
   - `sShader2` 0x00E0E470;
   - GPU particles 0x01128EE0, 0x01128F40, 0x01129030, 0x01129310,
     0x01129370, 0x01129460;
   - render setup 0x00DAFED0, 0x00FEE130, 0x01104470 (DepthStencilView
     ctor), 0x00DB8B90, 0x004080A0, 0x012E1A13, 0x012E1A60.
4. **Save system** (`sStorageExt`/`sSave`):
   - the `cSAVE_DATA_QUEST_SUB` ctor 0x00480240;
   - the `sSave::playerEditAndParam` ctor 0x00490E80 and the
     `sSave::playerGameData` ctor 0x004915A0;
   - 0x00480BA0, 0x0048AB50, 0x0048AC80, 0x0048B540, 0x0048F330,
     0x00491340, 0x004918D0, 0x00493AA0, 0x00495520, 0x00499830,
     0x00499880, 0x004A06B0, 0x004A3260.
5. **`aTitle`:** 0x004755E0, 0x00480950, 0x00494D80, 0x004AB440,
   0x0075BAD0.
6. **Others in frame systems:**
   - 0x0040DDC0, 0x00448500;
   - network 0x0046A150, 0x0046A2D0, 0x004709D0;
   - 0x00606130, 0x006066E0, 0x0081C330, 0x0081DB50;
   - 0x00C9B350, 0x00C9E0F0, 0x00D0CB00, 0x00D13E60, 0x00D29AF0,
     0x01106BB0, 0x011590E0;
   - CRT `_exit`, `__amsg_exit`, `__lseeki64`, `__ctrlfp`, `__write`.

**21 rejections** (claude-6, L22):

| Cause | Functions on the path |
|---|---|
| CPUID | **0x00DBD140, 0x00DBD170** (`sMain` ctor's CPU detection: the first rejection after the window) |
| `pushfd` | **0x00DBD110** (same) |
| x87 return | the number parsers 0x00CF91D0, 0x00CF9280, 0x00CF92E0, 0x00CF9360, 0x00CF93E0, 0x00CF9F70, 0x00CF9FD0, 0x00CFB650 (via 0x00D0B350 ← 0x00D0C0D0 ← 0x00D0C7E0 ← 0x00D0CA90); 0x004298B0; 0x00D473C0; CRT 0x012F05C0 |
| `fyl2x` | 0x00E22520 (`sBbsRpgMain` ctor via 0x0041B290 → 0x00E228F0) |
| `rcpss` | 0x012F1109 (CRT math, via 0x00DEB960) |
| jump tables | 0x004593E0 and 0x00B5A030 (`aTitle`'s save setup via 0x004965A0); CRT `memcpy` 0x012EFEA0 |
| `ss` register | CRT fault paths 0x012EDCAC, 0x012FF13C (only on a fault) |

### 2.2 Runtime services (claude-2)

| Import | On the path? | Where |
|---|---|---|
| IMM32 `ImmGetDefaultIMEWnd`, `ImmGetContext`, `ImmSetCompositionWindow`, `ImmSetCandidateWindow`, `ImmReleaseContext`, `ImmSetOpenStatus` | **yes, unconditional**, at window creation | 0x00DF1200 → 0x01106C80, 0x01106B30 |
| IMM32 `ImmGetCompositionStringW` and the rest | only with IME messages | 0x01106BB0 |
| `CreateProcessA` | shutdown only | 0x00DF1140 → 0x01179A70 → 0x011797F0 |
| `UnhandledExceptionFilter` | CRT fault reporting only | 0x012EDCAC, 0x012FF13C |
| WS2_32 (`socket`, `setsockopt`, `ioctlsocket`, `gethostbyname`, `WSAGetLastError`) | **conditional**: the online protocol (`sCaProto::vf06` 0x00CEEA80 → 0x00CEF210) stays idle until a connect request; whether the title requests one is *not traced* | 0x00DA2100, 0x00DA1BD0, 0x00D8B8F0 |
| AVIFil32, MSVFW32 | only while capturing video (`sCapture`) | 0x00F94890, 0x00F97440 |
| SSPI, clipboard, `wsprintfA`, `StringFromGUID2` | not reached on the likely path | |

Every other import the likely path calls (152 of 174) is bound.

### 2.3 D3D9 (claude-5)

- **`D3DXGetShaderConstantTableEx` and `D3DXGetShaderOutputSemantics`:
  on the path at the first draw.**
  - `sShader2::vf06` (every frame) → 0x00E12A20 (get or create the linked
    program of a shader pair) → 0x00E12850 → 0x00E10690.
  - That function reads the constant table of the vertex and the pixel
    shader (flags 0x20000), and the vertex shader's output semantics.
  - The data are in the bytecode (the CTAB comment block and the
    `dcl_` outputs), which `d3d9shader` already parses.
- `D3DXSaveTextureToFileInMemory`: only on a capture request (0x00DA9580,
  the state at `+0x440E10`/`+0x440E14`).
- The device itself: `Direct3DCreate9` and `D3DPERF_*` are bound; the
  device is created in `sRender`'s set-up.

### 2.4 Movies (Codex, L18)

The title screen's first state creates `uMovieExt`, playing
`movie\title\title_A.wmv` or `title_B.wmv`. So WMV playback (WMVCore
through `movies.rs`) runs before or at the title's first frame, not only
in cutscenes. `uMovie` uses `timeGetTime` (bound).

### 2.5 Steam, options (claude-10, L26)

- The Steam callback registration 0x004083A0 is in `sBbsRpgMain`'s
  constructor (not lifted; see 2.1).
- `config.ini` reading is 0x00D00010 (verified).

### 2.6 Audio (claude-4, L19)

- XAudio2 is created by the sound engine's set-up (0x00DD5ED0 →
  0x00DCE0E0 `CoCreateInstance`).
- 0x00DD5ED0 and `sSoundExt::vf06` 0x004AD440 are not lifted (2.1).
- `aTitle` issues `rSoundRequest` / `rSoundStreamRequest` at once (the
  title music).

## 3. Risks

- **Thread handshakes.**
  - The render thread (0x00DAC090) and the frame (0x00DAF7C0,
    0x00DAED10, 0x00DAECC0) wait on each other (`WaitForSingleObject`).
  - The loaders (0x00DBA9A0) feed `aInitialize`'s wait for archive
    state 10.
  - A missing completion (ReadFileEx / SleepEx alertable waits) hangs the
    logo screen rather than stopping.
- **The thread at 0x0041FFE0** starts running inside `sBbsRpgMain`'s
  constructor (flags 0), concurrently with the rest of the start-up.
- **Device lost:** 0x00DAF7C0's `TestCooperativeLevel` path sleeps
  instead of rendering. If the backend reports lost, the boot idles
  silently.
- **Save data:** `sStorageExt` loads the save during start-up. A missing
  or unreadable save takes the "no save" branch (not traced).
- **The online protocol** may request a connection from the title (2.2);
  if so, WS2_32 is needed.
- **CPU detection:** without SSE2 in the CPUID answer the game stops
  with "ERR02 : Could not use SSE2 instructions." (0x00DBE810).
  - the CPUID/`pushfd` answers must agree with the registry's recorded
    CPU (L22's rule);
  - they decide code paths (SSE use) for the rest of the run.
- The **movie** must deliver frames to `uMovieExt` in time, or the title
  waits for it (*not traced*).

## 4. Not done

- **Static only.** The order inside each constructor is the source
  order; branch conditions are marked where traced. The likely-path
  closure over-approximates within those roots (it follows every direct
  call, taken or not).
- **Not traced:**
  - which archive names groups 2 and 4 resolve to;
  - whether the title starts the online protocol;
  - `+0x38` of `aTitle`;
  - the save-load branch.
- The broad closure's other 65 non-verified functions (outside the
  likely path) were not given lifter reasons.
