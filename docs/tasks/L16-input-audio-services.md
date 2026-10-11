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

### Scope and evidence

Implemented a per-session headless input profile and a clocked, silent XAudio2
2.7 graph in `input.rs` / `audio.rs`. No OS device or game installation is
modified. Input is disconnected by default. The host channel can attach a
system mouse/keyboard and independently connect four XInput ports. Audio uses
an explicitly modelled stereo 48 kHz null endpoint: this is a runtime policy,
not a claim about the original machine's speakers. Its bounded float-block
output channel currently carries silence. Decoding, audible mixing, filters,
and reverb DSP are **not implemented or compared against the original**; the
channel is the delivery boundary for a later renderer. Buffer timing and guest
callbacks do run with no output consumer.

Research used read-only Ghidra HTTP decompilation, the inventory opened read-only,
and Capstone on the analysis exe. Private exports remain under
`E:/ddda-re/notes/runtime-L16`. Ghidra's decompiler loses arguments at several
stdcall indirect calls; SDK prototypes and instruction-level stack setup take
precedence over those recovered argument counts. No lifter or vector data changed.

### API census

Addresses below are enclosing function entries unless a precise call instruction
is stated. All new exported APIs use 32-bit **stdcall**. COM has the object pointer
as the first stack argument; source/submix/master voices have **no IUnknown prefix**.
Thread labels describe static provenance, not an observed Windows thread ID:
**input** means the keyboard/mouse/pad update or construction caller; **sound**
means the sound construction/control/update caller. These paths can be invoked
on different guest threads. A live-game trace establishing each caller's OS
thread ID is **unconfirmed**. `0x00DD5ED0` registers logical sound job 4 through
`0x00DF2C00`, with callback `0x00DC94C0`; that registration alone does not prove
an OS thread assignment. The new audio processing thread is separate from
these callers, as XAudio2 requires.

| API | Evidence | Observable contract / thread |
|---|---|---|
| DirectInput8Create | import thunk `0x012E1B8A`; `0x00DF43E0`, `0x00DFA0C0`, `0x00E05120`; version `0x800`, IID `0x0144B12C` | IDirectInput8A, HRESULT and output object; no aggregation; input construction |
| SetWindowsHookExA / UnhookWindowsHookEx | calls `0x00DF452C`, `0x00DF3CF3` / `0x00DF45B6`, `0x00DF3CC4` | WH_KEYBOARD_LL=13; callback `0x00DF3C40`, invoked before async state update on its installing thread; hook handle/removal |
| CallNextHookEx / GetAsyncKeyState | `0x00DF3C8D` / `0x00DF405C`, `0x00DF3C5E` | Hook chain and suppression; high bit held, low bit pressed-since-query. `0x00DF3C40` can suppress Alt+Space; input/hook thread |
| GetKeyState | call `0x0110673A` | State when queued key messages are removed; lock toggle low bit; message consumer |
| GetKeyboardLayout / MapVirtualKeyA | `0x00DF42F0`; wrappers `0x0040B460`, `0x0040B750` | Explicit en-US headless layout; VK/scan/character mappings, extended scan codes; input callers |
| GetRawInputDeviceList / GetRawInputDeviceInfoA | `0x00DF5780`, calls `0x00DF5787`, `0x00DF5801` | No physical HID devices: count zero; invalid device handle fails, output left intact; pad enumeration |
| SystemParametersInfoA | `0x00DF453D`, `0x00DF3D2E`, `0x00DF3D07`, `0x00E05176`, `0x00E04EC4`, `0x00E04D72`, `0x00E05284`, `0x00DF0D11`, `0x00DF2F86`, `0x00DF1657`, `0x00DF0EFC`, `0x00DF0F3A` | Mouse acceleration, FILTERKEYS/TOGGLEKEYS/STICKYKEYS, mouse trails, screensaver active and show-IME-UI state. No host desktop changes; input/window callers |
| ImmGetVirtualKey | import thunk `0x012E1B30` | VK_PROCESSKEY with no active IME translation; message processing (exact dynamic caller unconfirmed) |
| XInputGetState (#2) | `0x00DF52B0`, `0x00DF6DD0`, `0x00DF9C90`, `0x00DF9D50`, `0x00DF9EE0`; thunk `0x012E1B96` | 16-byte state, packet number changes only with input; empty port returns 1167 without changing output; input |
| XInputSetState (#3) | above pad paths and `0x00DF7E20`; thunk `0x012E1B90` | Two u16 motors, four ports, rumble channel; disconnected 1167; input |
| XInputGetCapabilities (#4) / XInputEnable (#5) | `0x00DF54E0` / `0x00DF5BE0`; thunks `0x012E1B9C` / `0x012E1BA2` | 20-byte capabilities of declared virtual gamepad; disabled input is neutral, vibration stops and resumes retained state; input |
| CoCreateInstance | `0x00DCE117`, `0x00DCE664` | Engine and AudioReverb 2.7 classes, HRESULT/output/IUnknown/refcounts; sound construction |

The window procedure at `0x00DF1550` consumes WM_KEYDOWN/UP, WM_SYSKEYDOWN/UP,
WM_CHAR and mouse messages. Host characters are explicit events; no invented
keyboard-layout/IME translation is inserted by TranslateMessage. Mouse motion,
wheel and button changes post through `user32::post`. Host input must target the
appropriate guest window/session; it is not a global player singleton.

### DirectInput interfaces

| Interface slots (zero based) | Evidence | Contract and headless reachability |
|---|---|---|
| IDirectInput8A CreateDevice 3 | `0x00E05120` | GUID_SysMouse at `0x0144B1BC`; absent device DIERR_DEVICENOTREG, connected seam device gets its own acquired/format/refcount state |
| EnumDevices 4 | `0x00DF99E0` | Game-controller class 4, flags `0x101` then 1, callback `0x00DF58B0`; no legacy joysticks are exposed, so callback is not invoked for this class |
| Release 2 | `0x00DF4560`, mouse/pad destructors | COM lifetime; QueryInterface/AddRef supplied for returned objects |
| Device SetCooperativeLevel 13, SetDataFormat 11, Acquire 7, Poll 25, GetDeviceState 9 | `0x00E05120`, `0x00E04A90`, `0x00DF99E0`, `0x00DF9EE0` | Mouse state 20 bytes; joystick state `0x110` bytes on legacy-only path. Original mouse format `0x0144B114` uses DIDFT_ANYINSTANCE and optional Z/buttons. Per-device acquisition; relative deltas consumed by mouse state reads |
| Device EnumObjects 4 / SetProperty 6 | `0x00DF99E0`, callback `0x00DF5A10` | Legacy pad sets DIPROP_RANGE; not reached without a legacy joystick. Mouse/keyboard enumeration uses 316-byte DIDEVICEOBJECTINSTANCEA; unsupported property requests return DIERR_UNSUPPORTED |
| Device EnumEffects 19 / CreateEffect 18 / GetDeviceInfo 15 | `0x00DF99E0`, callbacks `0x00DF5B50`, `0x00DF5B70`; effect GUID at `0x0144B1CC` | Empty FF enumeration, no FF object for system mouse/keyboard; 580-byte device info |
| IDirectInputEffect SetParameters 6 / Start 7 / Release 2 | `0x00DF7E20` / `0x00DF99E0`, `0x00DF9EE0` / `0x00DF6DD0` | Legacy FF-only path; no effect object created by the declared headless/input seam profile. A future legacy-controller backend needs this interface |

Additional bound device contracts (tested as surface checks): GetCapabilities,
GetObjectInfo, Unacquire, SetEventNotification (signals a guest event), and
GetDeviceData (DIERR_NOTBUFFERED, zero records). Unused remaining slots retain
named unbound diagnostics. Buffered DirectInput input and legacy force feedback
are not advertised. The keyboard hook path is tested for the game's single
installing-thread usage; cross-thread chains with multiple installing threads
remain unsupported and must not be advertised as general Windows hook support.

### Audio interfaces and callbacks

CLSID `0x0142AC50` is `5a508685-a254-4fba-9b82-9a24b00306af`
(XAudio2 **2.7**, not 2.8/2.9). IID `0x0142AC40` is
`8bcf1f58-9fe7-4583-8ac6-e2adc465c8bb`. The second CLSID, `0x0142AC70`,
is `6a93130e-1d53-41d1-a9cf-e758800bb179`: **AudioReverb 2.7**, requested
as IUnknown at `0x0142AC60`, not a second engine or X3DAudio.

| Interface / slots | Evidence | Contract |
|---|---|---|
| Engine Initialize 5, GetDeviceCount 3, GetDeviceDetails 4, SetDebugConfiguration 15, CreateMasteringVoice 10 | `0x00DCE0E0` | 2.7 engine includes three methods removed in 2.8. Device details are 1,068 bytes; mastering defaults to declared endpoint format |
| Engine CreateSourceVoice 8 | `0x00DCE1F0`, `0x00DCE2E0`, `0x00DCE530`, `0x00DC7BA0` | Source format, callback, flags/max ratio, sends/effects. `0x00DCE1F0` creates 128 mono ADPCM voices at 48 kHz, 70-byte blocks / 128 decoded samples; PCM banks in `0x00DCE2E0` |
| Engine CreateSubmixVoice 9 | `0x00DCE600`, `0x00DD58D0`, `0x00DDB240` | Channel/rate, processing stage, send/effect chains; stage-order checks prevent cyclic submix sends |
| Engine Release 2 / reverb Release 2 | `0x00DCE7D0` | Release references and tear down engine worker/voices |
| Voice SetOutputVoices 1 | `0x00DC6680`, `0x00DC7A80`, `0x00DCE600`, `0x00DD58D0` | Explicit sends or default mastering send; routes must stay within engine |
| Voice SetOutputMatrix 16 | `0x00DC8E20`; calls `0x00DC7FAF`, `0x00DC8036`, `0x00DD14E2`, `0x00DD157D`, `0x00DD15C2`, `0x00DD161E`, `0x00DD1AFB`, `0x00DD1B37`, `0x00DD1B60` | Source-by-destination channel coefficients, operation set; game explicitly supplies 1-to-6 and other matrices |
| Voice SetEffectParameters 6 | `0x00DCE9B0`, `0x00DD5A80` | Effect zero, 52-byte reverb parameters; caller data copied. Guest effects receive IXAPOParameters::SetParameters via QueryInterface, outside runtime borrows |
| Source SetFrequencyRatio 26 | exact call `0x00DC6D8F` in `0x00DC6DC0` | Changes source clock rate, constrained by creation maximum |
| Source Start 19 / Discontinuity 23 | `0x00DC6CE0`, `0x00DC7610`, `0x00DC7720`; Start also `0x00DC77A0` | Start queued data; mark last buffer as stream end |
| Source Stop 20 / FlushSourceBuffers 22 | `0x00DC6C80`; Stop also `0x00DC6CB0`, `0x00DD17F0`, `0x00DDB860` | Stop freezes consumption; flush retains already-started current buffer when running; removed buffers receive OnBufferEnd without stream/loop callbacks |
| Source SubmitSourceBuffer 21 | `0x00DC77A0`, `0x00DC8060`, `0x00DDAD20`, exact call `0x00DD1CEA` | Copy descriptor, retain caller buffer lifetime until completion; validate play/loop ranges, consume decoded samples in real time |
| Source GetState 25 | `0x00DC77A0`, `0x00DC99A0`, `0x00DC9AE0`, `0x00DDB860` | **2.7 takes only this + output**, 16-byte state (context/count/u64 samples); not the 2.8 flags ABI |
| Voice DestroyVoice 18 | `0x00DCE530`, `0x00DCE840`, `0x00DCE8F0`, `0x00DC7BA0`, `0x00DDBD70` | No callbacks after destruction; release held effects |

These are the identified audio call forms in the inspected sound region
`0x00DC5xxx..0x00DDCxxx`; indirect calls outside that region have not been
proved absent. Engine callback registration was not found there. The engine
callback interface is nevertheless implemented/tested for start/end of each
processing pass. All engine slots 0..15 and source voice slots 0..28 are bound;
submix/master share slots 0..18. Additional getters, volume/channel controls,
filters, operation sets, ExitLoop and SetSourceSampleRate have contract tests.
Filter/effect controls retain state in the silent renderer; they do not claim
an audible DSP result.

The six callback objects installed by `0x00DD5ED0` use vtable `0x0142AC9C`:
pass-start `0x00C342F0`, pass-end and stream-end `0x00BFC7D0`, buffer-start
`0x00C342F0`, **buffer-end `0x00DCCD00`**, loop-end `0x00C342F0`, voice-error
`0x00B16AF0`. Raw instructions at `0x00DCCD00` add the completed ring-buffer
size (`context+0x718+4*index`) to `context+0x72C`, then advance
`context+0x728` modulo four. Its `ret 8` confirms this+context stdcall.
No game callback is translated or replaced. **Inventory gap:** `0x00DCCD00`
is absent from both the shared inventory and this worktree's generated resolver.
The head was notified (board #9092) to add it through the native workflow;
L16 makes no lifter or shared-data changes. Original streaming completion will
stop at that named/unbound address until it is lifted. The other vtable targets
are present. `services::call_guest` dispatches bound callbacks on a guest thread created by `threads::create_thread`; no runtime state
borrow survives the callback. Completion/flush/loop callbacks, EOS counter
reset and engine stop/release are modelled even when no host output is attached.

### Contract sources

- SDK `dinput.h`, `winuser.h`, `xinput.h`, `xaudio2.h`, `xapo.h` (Windows SDK
  10.0.26100.0): structure layouts, GUIDs, flags, HRESULTs and method prototypes.
- [XAudio2 version differences](https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-versions): use the 2.7 engine and voice layouts.
- [XAudio2 callbacks](https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-callbacks),
  [source buffer](https://learn.microsoft.com/en-us/windows/win32/api/xaudio2/ns-xaudio2-xaudio2_buffer),
  [voice state](https://learn.microsoft.com/en-us/windows/win32/api/xaudio2/ns-xaudio2-xaudio2_voice_state),
  [flush](https://learn.microsoft.com/en-us/windows/win32/api/xaudio2/nf-xaudio2-ixaudio2sourcevoice-flushsourcebuffers),
  [effect parameters](https://learn.microsoft.com/en-us/windows/win32/api/xaudio2/nf-xaudio2-ixaudio2voice-seteffectparameters).
- [DirectInput8Create](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee416756(v=vs.85)) (version-error cases also checked against the host Windows DLL),
  [XInputEnable](https://learn.microsoft.com/en-us/windows/win32/api/xinput/nf-xinput-xinputenable),
  [GetAsyncKeyState](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getasynckeystate),
  [SystemParametersInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfoa),
  [raw device enumeration](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdevicelist).
- [Wine's versioned xaudio2fx.idl](https://github.com/wine-mirror/wine/blob/master/include/xaudio2fx.idl)
  corroborates the **version 7** reverb CLSID read from `0x0142AC70`; modern
  Windows SDK headers contain newer CLSIDs and cannot identify this legacy class.

### Integration and verification

Small integration changes: `services.rs` adds the 17 API bindings; `session.rs`
installs the two interface families before mapping the token range; `user32.rs`
drains input at PeekMessage, records removed-message key state and accepts cursor
updates. `runtime.rs` owns per-session Input/Audio state; `lib.rs` exports modules.
No thread implementation, D3D9 code, lifted code or generated file was edited.

Validation completed:

- `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo test -p ddruntime`: **36 passed**
  (13 new input/audio contract tests + all 23 existing runtime tests); one private
  fixture ignored by default. Final monitored invocation 7.39 s, minimum free
  RAM 15.23 GiB. No new source warnings; existing generated-code warnings and
  the pre-existing unused-mut warning in `tests/runtime.rs` remain.
- With `DDDA_EXE` pointing at the player's install, `cargo test -p ddruntime
  --test input_audio original_engine_initialization -- --ignored`: **1 passed**.
  Lifted `0x00DCE0E0` returned success, stored a live engine and mastering voice,
  and read two endpoint channels from the real 2.7 device-details layout.
  This is original lifted control flow using modelled services, not a comparison
  against Windows XAudio2. The original `0x00DCCD00` callback cannot run until
  the head supplies its missing native entry.
- The first cold compile took 321.32 s and stayed above 9.69 GiB free RAM
  (its initial fixture run failed, then was corrected). Subsequent builds used
  cached shards. All builds used jobs=4/incremental=0. The monitor deferred
  behind foreign vector/native jobs and stopped only its own tree once when a
  competing native build appeared. SSD free space remained about 29 GiB.
- `rustfmt --check` on the three new Rust files and `git diff --check` pass.
  No generated files, game bytes, private exports or build artifacts committed;
  no push and no shared data writes.

Before L16: 0 input/audio API bindings and no
input/audio interface families in ddruntime. After: 17 API bindings, DirectInput
root/device and XAudio2 engine/voice/reverb families plus host channels. This is
service-contract testing, **not live-game input/audio comparison**. Open work:
audible DSP/backend, exact multi-thread hook chains, legacy DirectInput joysticks
and FF, and dynamic caller-thread tracing. Synthetic callbacks verify buffer completion in the headless profile; actual
game streaming additionally requires the missing native `0x00DCCD00` entry.
