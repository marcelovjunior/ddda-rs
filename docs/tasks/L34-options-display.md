# L34: the game's options end to end (config.ini, display modes, frame rate)

- **Owner:** claude-10
- **Branch:** `task/L34-options` (worktree `E:\ddda-rs-wt\L34`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The player wants control over frame rate and graphics. The original keeps
its options in config.ini, read through `profile.rs` (L17A; MtRegistry
0x00CFFF00, path `<Documents>\CAPCOM\...`). The options are registered on
`sApp` (for example `MaxFPS` 30-1000 on `sMain`, 0x00DBE810; "AntiAlias"
on `sRender+0xEC`; resolution, window mode, vsync/PresentInterval,
"RenderFps", "FrameRate"). The rebuild must read and write those options
in the sandbox exactly as the original does, and answer the display
queries they depend on from the host.

## Task

1. **Census** (Results, with addresses): every option the game registers
   (name, type, range, default, setter, where it is applied), and the
   config.ini format the game writes (keys, sections, value formatting,
   from the writer code).
2. **Display queries:**
   - EnumAdapterModes, GetAdapterModeCount and GetAdapterDisplayMode
     answer the host's real monitor modes. The recorded profile has one
     machine's modes; decide with evidence whether to record at start or
     keep the recording.
   - Windowed vs full-screen present parameters;
   - the window size;
   - ddplay's window following the game's chosen resolution and mode.
3. **Round trip:** a test that the game's option writer produces
   config.ini, the reader reads it back identically, and an imported
   player config.ini (read-only copy, like L26's save import) is honoured.
4. **Document** each option and its effect for the player in
   `docs/OPTIONS.md`, with the frame-rate caveats from `docs/FRAME.md`
   section 1a and L30 (claude-7's audit).
5. Merge-train rule.

## Results

Branch `task/L34-options` (claude-10). The player-facing description of
every option is [docs/OPTIONS.md](../OPTIONS.md). The two research
scripts are `research/runtime/options_census.py` and
`research/runtime/windows/display_modes.py`.

### 1. Census

**Option items.** sApp keeps up to 256 items: a pointer array at +0x154
and the count at +0x554. All of them are made by 0x00DF1090 (0x44 bytes:
+4 section mask, +6 type, +8 the property descriptor, +0x30 a valid-values
function, +0x34 an enabled predicate, +0x38/+0x3C the range, +0x40 the
choice list). There are four registrars:

| Registrar | Type | Arguments popped |
|---|---|---|
| 0x00DF2B80 | 1, a choice | 0x3C |
| 0x00DF2BC0 | 3, f32 range | 0x3C |
| 0x00DF2C00 | 4, s32 range | 0x3C |
| 0x00DF2C40 | 5, bool | 0x34 |

Type 2 (flags) exists in the reader and writer but has no registrar.
The descriptor is:
- the name;
- the MtDTI type word (bit 23: accessors);
- the object;
- the **getter** (descriptor word 3);
- the **setter** (word 5).

FRAME.md calls 0x00F5BB60 MaxFPS's setter. It is the getter
(`fld [ecx+0x3c]`); the setter is 0x00DBE120. The census script emulates
every registrar call site with unicorn. That gives 16 items:

| Item | Section | Type | Range or choices | Getter / setter | Field | Site |
|---|---|---|---|---|---|---|
| JobThread | CPU | s32 | 1 to sMain+0x7010 (the CPU count); the setter caps it at 8 | 0x00DBD0B0 / 0x00DBD620 | sMain +0x9C | 0x00DBEAA5 |
| MaxFPS | GRAPHICS | f32 | 30.0-**999.0** (0x4479C000; FRAME.md says 1000) | 0x00F5BB60 / 0x00DBE120 | sMain +0x3C | 0x00DBEB43 |
| RenderingThread | CPU | bool | (direct field) | | sRender +0x26 | 0x00DB1F4C |
| HDR | GRAPHICS | choice | NONE 0, DEFAULT 1, FLOAT 2 (0x01429C58) | 0x00DA69D0 / 0x00DA8240 | sRender +0xF4 | 0x00DB1FD6 |
| AntiAlias | GRAPHICS | choice | built at run time (sRender +0x43CD70) | 0x00DA7D10 / 0x00DA7D20 | sRender +0xEC | 0x00DB205D |
| AltAntiAlias | GRAPHICS | choice | NONE, FXAA, FXAA3, FXAA3HQ (0x01429C20) | 0x00DA69E0 / 0x00DA69F0 | sRender +0xF0 | 0x00DB20E2 |
| SLI | GRAPHICS | bool | | 0x00DA6A80 / 0x00DA8310 | sRender +0x433E50 | 0x00DB2166 |
| Stereo | GRAPHICS | bool | | 0x00DA6A90 / 0x00DA82F0 | sRender +0x433E51 | 0x00DB21E5 |
| ShadowQuality | GRAPHICS | choice | LOW, MEDIUM, HIGH (0x01429BF0) | 0x00DA69A0 / 0x00DA6990 | sRender +0xE8 | 0x00DB226E |
| FullScreen | DISPLAY | bool | enabled only while sRender +0x440E00 (0x00DA6AC0) | 0x00DA69C0 / 0x00DAE9C0 (switches through Reset) | sRender +0x23 | 0x00DB22F1 |
| VSYNC | DISPLAY | bool | | 0x00DA98E0 / 0x00DA98F0 | sRender +0x433E34, the index into the present-interval table 0x01429878 = {IMMEDIATE, ONE, ONE, ONE} | 0x00DB2370 |
| Flush | DISPLAY | bool | | 0x00DA6AA0 / 0x00DA6AB0 | sRender +0x104 | 0x00DB23EF |
| SeVoiceNum | SOUNDS | s32 | 16-128 | 0x00DC94C0 / 0x00BFC7D0 (`ret 4`: no effect) | | 0x00DD64F7 |
| EffectVolume | GRAPHICS | choice | LOW, MEDIUM, HIGH (0x01438868) | 0x00EDD360 / 0x00FB5170 | effect object +0x14C | 0x00FB6463 |
| TextureDetail | GRAPHICS | choice | HIGHEST, HIGH, MEDIUM, LOW, LOWEST (0x0143A6A0) | (direct field) | +0x80 | 0x00FECDB9 |
| TextureMipLimite | GRAPHICS | choice | 1 ... 256 (0x0143A650) | (direct field) | +0x84 | 0x00FECE3C |

**Keys outside the items.**
- The PC options menu (writer 0x00EDDB20, reader 0x00EDE0F0) also owns
  GrassQuality, CameraFov (clamped 0-15), TextureFiltering (capped by the
  device's anisotropy), Brightness, ViewRange and DofFilter. It writes
  MaxFPS only as 30, 60 or 150.
- Start-up reads these directly:
  - DISPLAY Resolution (`1280x720`, clamped 640x480 - 8192), RefreshRate
    (0 means the desktop's), FullScreen and VSYNC (0x00DF1200, 0x00DB1480);
  - Window MainX/MainY (default 32);
  - GRAPHICS MaxFPS (0x00DBE0F0: default 60, set with no range check).
- The window procedure writes Window MainX/Y (also SubX/Y and Console*) at
  WM_DESTROY.
- The input objects own `[KEYBOARDMOUSE]` and the pad sections (listed in
  OPTIONS.md).
- No `RenderFps` or `FrameRate` key exists. `FrameRate` is an sMain DTI
  property group.

**Format** (writer code):
- the sections are the table at 0x0142C068: GAME 1, GRAPHICS 2, DISPLAY 8,
  ETC 0x10, DEVELOP 0x20, SOUNDS 4, CPU 0x40;
- values: text, `%d`, `%f`, `ON`/`OFF`; choices by name, flags as `A | B`;
- every write goes through WritePrivateProfileStringW on MtRegistry
  0x018CFF48;
- the file is `%LOCALAPPDATA%\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini`
  (SHGetFolderPathW 0x801C), **not Documents**: `profile.rs` and
  `files.rs` said Documents and now say Local AppData.

**Start-up order** (0x0041B400, then 0x00DF3AC0):
1. MaxFPS is read.
2. The items are read.
3. The menu state is read.
4. The menu is written, only on a first run. The byte tested,
   0x018D0158, is MtRegistry +0x210 (0x018CFF48 + 0x210): the
   constructor 0x00CFFF00 sets it when config.ini does not exist
   (resolved in L43).
5. `mFps` = 30.
6. The items are read again (mask 0xFFFF).
7. Every item is written back.

A config.ini the original wrote on the maintainer's machine has exactly
the key order this sequence predicts, with the menu's keys first.

### 2. Display queries

- **Decision: ask the host at session start; do not keep the old list.**
  The fixed list (`d3d9::MODES`, 1920x1080 at 60 Hz) was not a recording
  of any machine. This host's Direct3D 9 reports a 2560x1440 / 240 Hz
  desktop and 161 X8R8G8B8 modes.
- New light crate `hostdisplay` asks the host's own Direct3D 9 (adapter 0)
  for its desktop and modes. On this host, D3D9's list equals
  EnumDisplaySettingsW's 32-bit modes, each (w, h, Hz) once, in
  first-seen order, and its desktop equals ENUM_CURRENT_SETTINGS
  (display_modes.py: "True" for both).
- `d3d9::display()` takes, in order: `set_display` (tests), a
  `DDRUNTIME_DISPLAY` record (replays), or the host. It drives
  GetAdapterModeCount, EnumAdapterModes, GetAdapterDisplayMode,
  IDirect3DDevice9::GetDisplayMode and GetSystemMetrics' screen size.
- While the game's device is full screen, the current mode is the
  full-screen mode.
- Present parameters:
  - Windowed: completed as before (client size, display format).
  - Full screen: the size and X8R8G8B8 must name a host mode, and the
    refresh rate must be a listed rate or 0. For 0, the runtime takes the
    desktop's rate if the mode has it, else the first listed rate
    (**runtime choice**, labelled in code).
- Window size: the game sizes its window from DISPLAY Resolution through
  AdjustWindowRect (0x00DF1200). The back buffer is the client area.
- ddplay: `d3d9dev::presentation()` gives the game's back-buffer size,
  windowed/full screen, refresh rate and vsync interval. It must be read
  on the guest thread. ddplay follows it on branch `task/L34b-window`
  (from train/1, as the head asked, b4f221b):
  - `present::Sink::following` reads it at each Present and reports
    changes;
  - windowed: the window takes the back buffer's size;
  - full screen: the monitor's video mode with that size and refresh rate
    (exclusive), borderless when there is none;
  - VSYNC=OFF (IMMEDIATE) or ON sets the surface's present mode, unless
    `--no-vsync` overrides it.

### 3. Round trip

`crates/ddruntime/tests/options.rs` runs the CRT start-up and the static
initializers, which build MtRegistry with the sandbox path. It then calls
the game's **own lifted code**:
- the menu writer 0x00EDDB20, on a menu object;
- the item writer 0x00DF2CA0, on an sApp with 16 items made like the
  registrars make them (the exe's names and choice lists, direct fields);
- the window-position writer.

The test then checks four things:
- The file equals what the original wrote, byte for byte (the
  maintainer's config.ini without `[KEYBOARDMOUSE]`).
- The item reader 0x00DF39E0, the MaxFPS reader and the typed readers
  read every value back.
- An imported config.ini is honoured, and its source is untouched.
- When this machine has the player's own config.ini, every key of it
  reads back through the game's string reader.

`hostdisplay` has a text-form test and a host test (the desktop is among
the modes, each mode once).

### 4. Docs

- docs/OPTIONS.md: each option, its values and effect, the frame-rate
  caveats from FRAME.md 1a and FRAME_RATE.md (L30), and the display
  rules.
- Proposed RUNTIME.md row (codex claims RUNTIME.md; this goes to head):
  "Display: the host's own Direct3D 9 modes and desktop (crates/hostdisplay),
  current mode follows a full-screen device; options and config.ini:
  docs/OPTIONS.md | `d3d9.rs`, `profile.rs` | implemented; tests written
  (merge train)".

### Verification and what is left

- **Verified on the branch:**
  - `cargo test -p hostdisplay`: 2 passed, including the host query on
    this machine;
  - `cargo check -p ddruntime --no-default-features --tests` (no lifted
    program, L36) passes with the L34 changes; tests/options.rs also
    type-checks with its `game` gate lifted for the check;
  - `cargo check -p ddplay` (task/L34b-window, against ddruntime without
    the lifted program) passes;
  - the census (unicorn emulation, cross-checked against Ghidra's
    decompilation for every site);
  - the display probe (ran on this host);
  - the expected config.ini text against the original's real file;
  - `cargo metadata` resolves the workspace with the new crate
    (Cargo.lock updated).
- **Unverified, waiting for the merge train:**
  - the round-trip test has not run (it needs the lifted program);
  - ddplay's follow-up has not run live.
  - The test's start-up path (the CRT steps plus `__cinit` via
    `Session::call`) is the same one `Session::start` uses. The test
    assumes the MtString allocator needs nothing that WinMain sets up.
- **Test edits in other agents' files:**
  - `tests/runtime.rs:295` (codex agreed, #9319) now expects the host's
    desktop;
  - `tests/d3d9_device.rs:133` (claude-5 agreed, #9330) now Resets full
    screen at the host's desktop mode (419fd70).
- **Not done:**
  - per-option effects compared live with the original.
