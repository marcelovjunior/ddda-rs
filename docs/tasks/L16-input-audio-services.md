# L16: input and audio services in the runtime

Owner: Codex (same session). Branch `task/L16-input-audio` from the head's
`main` (after L15 is merged, or from current `main` if L15 is still in
review: this task touches different files), worktree `E:\ddda-rs-wt\L16`.

## Why

`crates/ddruntime` runs the translated game on a modelled Windows
(docs/RUNTIME.md, "Implementation status"). Graphics is the head's area
(`d3d9*.rs`, `crates/d3d9shader`, `crates/d3d9wgsl`, `crates/d3d9ref`); the
other device families the game needs before it can be played are input and
audio, and neither exists yet. When the boot reaches them, every call to an
unmodelled API or interface method stops with its name: this task makes
those calls answer as the documented APIs do, headless first (no host
devices), with a clear seam for the later host backends.

## What the game uses (head's Ghidra notes, verify each)

- **Keyboard/mouse**: `sKeyboard` ctor 0x00DF43E0: `DirectInput8Create`
  (version 0x800, IID at 0x0144B12C), `SetWindowsHookExA(WH_KEYBOARD_LL,
  0x00DF3C40)`; also `GetAsyncKeyState`, `GetKeyState`, `MapVirtualKeyA`,
  `GetKeyboardLayout`, raw-input device queries (`GetRawInputDeviceList`,
  `GetRawInputDeviceInfoA`); the window procedure 0x00DF1550 handles
  WM_KEYDOWN/WM_CHAR/WM_SYSKEYDOWN; DirectInput callers 0x00DFA0C0,
  0x00E05120.
- **Pads**: XInput 1.3 by ordinal (`XINPUT1_3.dll` #2..#5: XInputGetState,
  XInputSetState, XInputGetCapabilities, XInputEnable), called from
  0x00DF52B0, 0x00DF6DD0, 0x00DF9C90, 0x00DF9D50, 0x00DF9EE0.
- **Audio**: XAudio2 2.7 created with `CoCreateInstance` in 0x00DCE0E0
  (CLSID at 0x0142AC50, IID at 0x0142AC40; Initialize at +0x14,
  CreateMasteringVoice at +0x28) and 0x00DCE600 (CLSID 0x0142AC70, IID
  0x0142AC60: identify it, e.g. the debug engine or X3DAudio/XAPO). Find
  every interface and method the game calls (source/submix voices,
  buffers, callbacks: IXAudio2VoiceCallback/IXAudio2EngineCallback are
  guest objects the runtime must call back through the dispatcher on the
  right thread).

## Task

1. Census (write it into this brief's Results): every input/audio API and
   COM method the game calls, with call sites and the documented contract
   (Microsoft Learn / the DirectX SDK headers' documentation), and which
   game thread calls it.
2. Implement them in new runtime modules (`crates/ddruntime/src/input.rs`,
   `audio.rs`), following the existing conventions: `services.rs` (service
   ABI: stdcall/cdecl/thiscall exactly as the original), `interfaces.rs`
   (COM objects: vtables of named tokens, methods bound as services,
   others stop with their name), `threads.rs` (guest threads, the guest
   lock, callbacks via `services::call_guest`), `user32.rs` (window
   messages: `user32::post` is how input messages enter). Headless
   behaviour: no device connected / silence, but every documented result
   code, out-parameter and callback the game can observe must follow the
   contract (e.g. XInputGetState on an empty port returns
   ERROR_DEVICE_NOT_CONNECTED; an XAudio2 source voice consumes its
   submitted buffers in real time and calls OnBufferEnd).
3. A host seam: a small trait or channel through which a later window
   layer can inject keyboard, mouse and pad state and receive mixed audio,
   with a test that drives it.
4. Tests in `crates/ddruntime/tests/runtime.rs` (or a new test file) for
   every service: argument handling, results, out-parameters, callbacks.

Do not touch the head's files (`d3d9.rs`, `d3d9dev.rs`, the `d3d9*`
crates); small additions to `services.rs`/`user32.rs`/`session.rs` are fine
and must be listed in Results. Never invent behaviour: where the contract is
unclear, cite what you found and choose the documented default.

## Rules

As in L12 (`docs/tasks/L12-remaining-lift-rejections.md`, "Rules"): astra
medium, no subagents, CARGO_BUILD_JOBS=4, never two heavy jobs at once, no
pushes, commit on your branch only, results below with what is tested.

## Results

(to be filled by the owner)
