# L35: the path from WinMain to the title screen's first frame (research map)

- **Owner:** claude-7
- **Branch:** `task/L35-title-path` (worktree `E:\ddda-rs-wt\L35`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md). Research only (Ghidra and
  the lifted sources); CPU-light.

## Why

claude-2 (L21) drives the boot stop by stop. Each stop is found only when
the run reaches it. A map of what the game does between WinMain and the
first Present of the title screen lets every owner prepare ahead of the
boot instead of after it.

## Task

1. **Trace the start-up sequence** from WinMain (start from FRAME.md
   section 1, docs/RUNTIME.md and L21's boot log), through `sApp`/`sMain`
   construction, the singletons (`sBbsRpgMain`'s constructor 0x0041B400),
   the device and window setup, and resource loading, up to the title
   scene's first frame and Present.
2. **For each step, list:**
   - the imports and COM methods it calls (D3D9, XAudio2, DirectInput,
     Steam, files, threads);
   - the files it opens (archive names, config.ini, shader packages);
   - the threads it starts;
   - the lifted functions it needs.
   Mark each item as:
   - *covered*: the runtime implements it, with a test;
   - *missing*: give the owner (claude-2 runtime, claude-5 D3D9, claude-4
     audio/network, claude-10 options/Steam, Codex SEH, claude-3/claude-6
     lifter);
   - *unknown*.
3. **Cross-check the lifter side:** every function on the path should be
   verified in the inventory (`scripts/inventory.py`). List any that are
   not.
4. **Write `docs/TITLE_PATH.md`:** the ordered map with addresses, the
   *missing* list grouped by owner, and the risks (long waits, thread
   handshakes, device-lost paths). Send each owner their part with
   SendMessage.

## Results

claude-7. The map is [TITLE_PATH.md](../TITLE_PATH.md). It is static:
Ghidra, the inventory's call graph, the lifted sources, and the lifter's
own `lift()` run read-only on each missing function.

- **The path.** WinMain → `sApp` → app setup (mutex, `config.ini`, the
  window, `new sBbsRpgMain`) → `sMain`'s base constructor (CPU detection,
  worker threads) → 49 game singletons → the main loop.
  - `aInitialize` shows the Capcom logo, waits for archive group 4, loads
    group 2, then switches to `aTitle` (`sArea`->vf+0x28).
  - `aTitle`'s first state creates `uGUITitle` and a `uMovieExt`
    (`movie\title\title_A/B.wmv`).
  - Files: `rom\Initialize.arc`, `rom\title.arc`, the `rom\gui` logo and
    title archives, the shader packages, `config.ini`, the save.
  - Threads: render, two resource loaders, workers, `MtThread`,
    0x0041FFE0. All entries are verified.
- **Coverage.** The likely-path closure has 3,594 functions; 3,276 are
  verified, and 101 others are on the path. None of the 101 is in the
  shared lifted set:
  - **80 lift with today's lifter.** They were not in the last lift run.
    The first are the window's IME setup 0x01106C80 and, in
    `sBbsRpgMain`'s constructor, the constructors of `sNpcManager`,
    `sObjCollision`, `sSave`, `sSetManager` and `sShader2`, the Steam
    callbacks 0x004083A0, and the sound engine 0x00DD5ED0. Also every
    frame's `sSoundExt::vf06` and the clock's 0x0130A5C0.
  - **21 are lifter rejections.** The first are CPUID/`pushfd` in
    `sMain`'s constructor (0x00DBD110, 0x00DBD140, 0x00DBD170). Then
    x87 returns, `fyl2x` 0x00E22520, `rcpss`, three jump tables, and the
    `ss` register on CRT fault paths.
- **Services.**
  - **IMM32 is on the path, unconditionally:** window creation calls
    0x01106C80 (`ImmGetDefaultIMEWnd`, `ImmGetContext`,
    `ImmSetCompositionWindow`, `ImmSetCandidateWindow`,
    `ImmReleaseContext`, `ImmSetOpenStatus`).
  - **D3DX `GetShaderConstantTableEx` and `GetShaderOutputSemantics` are
    on the path** at the first link of a shader pair (`sShader2` →
    0x00E12850).
  - Conditional: WS2_32 (only if the online protocol is asked to
    connect), AVI/VFW (capture), `CreateProcessA` (shutdown),
    `UnhandledExceptionFilter` (CRT faults), and
    `D3DXSaveTextureToFileInMemory` (capture).
  - The other 152 of the path's 174 imports are bound. WMVCore is
    covered by L18, XInput by ordinal.
- **Sent to the owners:**
  - claude-2: IMM32;
  - claude-5: D3DX;
  - claude-6: the rejections;
  - claude-3 and the head: the 80 functions that lift now;
  - Codex: the title movie;
  - claude-4: sound;
  - claude-10: Steam.
- **Not done:** no run. Not traced: the archive names behind groups 2
  and 4, whether the title starts the online protocol, `aTitle`'s `+0x38`,
  and the save-load branch.
