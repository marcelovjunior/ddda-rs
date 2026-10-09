# oracle: an in-process probe for the running game

- **Owner:** claude lane
- **Branch:** `task/oracle`
- **Board area:** `live/game` (exclusive: the only agent that touches the running game)
- **Status:** open

## Goal

A tool that lets agents explore the original game unattended, while the
maintainer uses the PC, the way nv-rs uses `nv-probe` / `nv-inject` /
`nv-call` (`research/nv-oracle` in slaterain/nv-rs). The external route
does not work:
- DDDA.exe stops updating when its window loses focus;
- PrintWindow returns black for its D3D9 frames;
- DirectInput only reads the keyboard while focused.

So the probe must run inside the process.

## Requirements (in order of value)

1. **Injector:** a 32-bit DLL (Rust, target `i686-pc-windows-msvc`, its
   own workspace under `research/ddda-oracle/` so the main workspace
   stays 64-bit), loaded into the running `DDDA.exe` with
   `LoadLibraryW` through `CreateRemoteThread`. Plus an unload path. No
   files of the install are modified, ever.
2. **Command server** inside the DLL on `127.0.0.1` (TCP, line-based JSON),
   with a Python client `research/ddda-oracle/client.py`. Commands run on
   the game's main thread, from a hook on a per-frame function (find one:
   the main loop, or the D3D9 `Present` / `EndScene` through the device
   vtable).
3. **Keep running unfocused:** find the focus check (the window procedure
   handling `WM_ACTIVATE` / `WM_ACTIVATEAPP`, and the flag the main loop
   tests) and neutralise it while the probe is loaded. Restore it on
   unload.
4. **Frame capture:** copy the back buffer to a PNG on request
   (`GetBackBuffer` → `GetRenderTargetData` → lock), from the frame hook.
5. **Input:** inject pad or keyboard state from inside the process. Find
   where the game reads input (DirectInput8 `GetDeviceState`, XInput
   `XInputGetState`; both are imports) and overlay scripted state.
6. **Memory and calls:** read and write memory, resolve live objects by
   MtDTI class name (vtables in `E:\ddda-re\notes\dti-registry.json`),
   and call a game function at an address with given arguments on the main
   thread (`thiscall` / `cdecl` / `stdcall`).
7. **Recipes** built on these, documented with their addresses:
   - player position read and teleport;
   - current stage and area;
   - time of day;
   - open a menu;
   - talk to the nearest NPC;
   - list the party (Arisen, main pawn, hired pawns) with their objects.

## Safety (non-negotiable)

- **Before the first injection of a session**, copy
  `C:\Program Files (x86)\Steam\userdata\124651925\367500` to
  `E:\ddda-re\save-backups\<timestamp>\`. Never delete backups.
- **Never** write to the game install or make persistent patches.
  In-memory patches are undone on unload.
- If the game crashes, note what you did, do not retry blindly, and tell
  head on the board.
- Avoid actions that spend the player's resources or advance quests,
  unless a backup is fresh and the experiment needs it.
- **The maintainer may be at the PC.** Never steal window focus or send OS
  input. Everything goes through the probe.

## Evidence

- Exe facts: `docs/METHODOLOGY.md`. Imports: D3D9, D3DX9_43, DINPUT8,
  XINPUT1_3, WS2_32, WININET.
- Ghidra over HTTP on 127.0.0.1:8089 (read-only).
- Existing external helpers (read-only memory, Python):
  - `E:\ddda-re\live\mem.py`
  - `E:\ddda-re\live\ctl.py`: write access, window lookup, a capture that
    returns black.
  - Known fields: uPlayer +0x40 position; uCameraGame +0x30.. camera
    (docs/WORLD.md).

## Scope

- `research/ddda-oracle/**` (new, its own Cargo workspace; add
  `research/ddda-oracle/target/` to the ignore rules if needed)
- `docs/ORACLE.md` (new): how to inject, the commands, the recipes, the
  addresses found
- `.gitignore` (only to ignore the oracle's build output)

## Done when

- From a script, without touching the keyboard or the window:
  - inject into the running game;
  - keep it updating unfocused;
  - capture a frame to PNG that shows the game;
  - read the player's position, teleport the player 2 m, read it back;
  - unload cleanly with the game still running.
- ORACLE.md documents it. Fill in Results and report to `head`. Post `info`
  to `all` when the oracle is usable: other agents will ask you for live
  checks.

## Results

(filled in by the agent)
