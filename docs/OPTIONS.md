# The game's options (config.ini)

Brief: [L34](tasks/L34-options-display.md). DDDA.exe 2017-12-13 Steam.
Sources: Ghidra and the lifted code, a config.ini the original game wrote
on the maintainer's machine, and research/runtime/options_census.py
(every option the exe registers).

## Where the file is

- The original keeps it at
  `%LOCALAPPDATA%\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini`, not under
  Documents. MtRegistry's constructor (0x00CFFF00) asks SHGetFolderPathW
  for CSIDL_LOCAL_APPDATA | CSIDL_FLAG_CREATE (0x801C), creates the folder
  if needed, and notes when the file is missing. The registry is the
  static object 0x018CFF48.
- The rebuild keeps it in the sandbox at
  `<sandbox>\csidl-0x001c\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini`
  (`ddruntime::profile::config_path`). It never writes the player's own
  file.
- To start from the player's options, import their file. Set
  `DDRUNTIME_IMPORT_CONFIG=player` (their own file) or
  `DDRUNTIME_IMPORT_CONFIG=<path>`, or call `profile::import_config`. The
  import reads the source, copies it byte for byte, and keeps a sandbox
  config.ini that already exists.

## The file format

- It is an INI file read and written with GetPrivateProfileStringW and
  WritePrivateProfileStringW. Code page 1252, CR LF line ends; the exact
  rules are in `ddruntime::profile`.
- Typed writers (all go through 0x00CFFD20):
  - text: 0x00CFFE20;
  - integers: `%d`, 0x00CFFE30;
  - floats: `%f`, so six decimals, e.g. `150.000000`, 0x00CFFEA0;
  - booleans: `ON` or `OFF`, 0x00CFFE80.
- Typed readers (each returns its default when the key is missing or
  empty):
  - text: 0x00D00120;
  - integers: atoi, 0x00D001E0;
  - floats: atof, 0x00D00440;
  - booleans: 0x00D002A0. `on` and `true` read as true, `off` and `false`
    as false, anything else as its number, case-insensitive.
- A new key goes after the last key of its section; a new section goes at
  the end. So the order of keys in a file shows the order the game first
  wrote them.

### Who writes what, and when

1. The input object writes `[KEYBOARDMOUSE]` (0x00408700, 0x004089F0,
   0x00408AF0).
2. `sBbsRpgMain`'s constructor (0x0041B400) runs these steps:
   - it reads MaxFPS (0x00DBE0F0, default 60) and sets it with no range
     check (0x00DBE120);
   - it reads every option item (0x00DF39E0);
   - it reads the PC options menu's state (0x00EDE0F0);
   - only on a first run, it writes the menu (0x00EDDB20). The byte it
     tests, 0x018D0158, is MtRegistry's "config.ini did not exist" flag:
     0x018CFF48 + 0x210, set by MtRegistry's constructor 0x00CFFF00 when
     GetFileAttributesW fails (found in L43). So a new config.ini starts
     with the menu's keys, in the menu's order.
   - It then sets the clock's base rate `mFps` to 30.
3. The options menu writes every key it shows when the player applies it:
   0x00EDDB20, called from 0x006A8AC0.
4. At start-up, after the main object exists (0x00DF3AC0), the game reads
   every option item from the file (0x00DF39E0, all sections), then writes
   every item back (0x00DF2CA0). The sections go in the order GAME,
   GRAPHICS, DISPLAY, ETC, DEVELOP, SOUNDS, CPU. Choice items are written
   by name, flag items as `A | B`, floats as `%f`, integers as `%d`, and
   booleans as `ON`/`OFF`. Every start therefore fills in any missing item
   key.
5. When the main window closes (WM_DESTROY in 0x00DF1550), the game writes
   `[Window]` MainX and MainY. It also writes SubX/SubY for the second
   display and ConsoleX/Y/W/H when a console window exists.

How an option item is read back (0x00DF3770):
- a choice must match one of its names, case-insensitive;
- an integer must be inside its range (0x00DF2100);
- a float must be inside its range (0x00DF2400);
- a boolean accepts `ON`, `OFF`, `TRUE` or a number. The original also
  reads `FALSE` as **on** (a quirk of 0x00DF3770).

A value that is not accepted leaves the option unchanged.

## The options

"Menu" means the in-game PC options menu (0x00EDE0F0 reads it, 0x00EDDB20
writes it). "Item" means an sApp option item: 16 are registered through
0x00DF1090. The types and ranges below are the exe's registrations.

### [GRAPHICS]

| Key | Values | Missing key | Applied to | Notes |
|---|---|---|---|---|
| MaxFPS | float; item range 30-999 | 60 | `sMain.mMaxFps` (+0x3C), the frame cap (FRAME.md 1a) | The menu offers 30 / 60 / 150. It maps the current cap: below 31 is 30, below 61 is 60, anything higher is 150. Applying the menu rewrites the key to one of those three. The start-up read (0x00DBE0F0) takes any value, and the item read ignores values outside 30-999, so a hand-edited value survives until the menu is applied. See "Frame rate" below. |
| HDR | NONE, DEFAULT, FLOAT (0-2) | the object's default | sRender +0xF4 | The menu shows DEFAULT and FLOAT. |
| AntiAlias | the device's multisample types (list built at run time, sRender +0x43CD70) | NONE | sRender +0xEC (0x00DA7D20 accepts only listed types) | MSAA. |
| AltAntiAlias | NONE, FXAA, FXAA3, FXAA3HQ | NONE | sRender +0xF0 | Post-process anti-aliasing. |
| ShadowQuality | LOW, MEDIUM, HIGH | LOW | sRender +0xE8 | |
| TextureDetail | HIGHEST, HIGH, MEDIUM, LOW, LOWEST | HIGH | texture streaming (item from 0x00FECDB9) | The menu shows LOW, MEDIUM, HIGH (and writes LOWEST for anything else). |
| TextureMipLimite | 1, 2, 4 ... 256 (0-8) | | texture mip limit (item) | Not in the menu. |
| TextureFiltering | INVALID, TRILINEAR, ANISO_X2, X4, X8, X16 | ANISO_X16 | | Menu only. Capped by the device's maximum anisotropy. |
| EffectVolume | LOW, MEDIUM, HIGH | | sEffect +0x14C | Effect amount (item and menu). |
| GrassQuality | LOW, MEDIUM, HIGH | HIGH | | Menu only. |
| ViewRange | NORMAL, FAR, FARTHEST | NORMAL | | Menu only. |
| DofFilter | ON, OFF | ON | | Menu only. Depth of field. |
| CameraFov | float, clamped to 0-15 | 0.0 | | Menu only. An addition to the field of view. |
| Brightness | float | 10.0 | | Menu (written also by 0x004ECA20). |
| SLI | ON, OFF | OFF | sRender +0x433E50 | |
| Stereo | ON, OFF | OFF | sRender +0x433E51 | 3D stereo. |

### [DISPLAY]

| Key | Values | Missing key | Applied to | Notes |
|---|---|---|---|---|
| Resolution | `WIDTHxHEIGHT` | 1280x720 | the window's client size (0x00DF1200) and so the back buffer | Clamped to 640x480 - 8192x8192. The menu lists the adapter's modes (EnumAdapterModes). The cursor size depends on the width (0x00DF31F0). |
| RefreshRate | e.g. `144.00Hz` (atof) | 0, which means the desktop's rate (GetAdapterDisplayMode, 0x00DB16B4) | FullScreen_RefreshRateInHz when full screen | The game looks up the mode with that size and `%.2fHz` rate (0x00DA9B50) and takes its size and rate. The menu writes the chosen mode's rate. |
| FullScreen | ON, OFF | OFF | sRender +0x23; Windowed = !FullScreen at CreateDevice, then 0x00DAE9C0 switches | Only while full screen is allowed (sRender +0x440E00). |
| VSYNC | ON, OFF | the object's current value | sRender +0x433E34 -> PresentationInterval: OFF = IMMEDIATE (0x80000000), ON = ONE (1), table 0x01429878 | |
| Flush | ON, OFF | | sRender +0x104 | Not in the menu. |

### [SOUNDS], [CPU], [Window]

| Key | Values | Notes |
|---|---|---|
| SOUNDS SeVoiceNum | integer 16-128 | Registered with a setter that does nothing (0x00BFC7D0): the value is written but has no effect. |
| CPU JobThread | integer, 1 to min(CPU count, 8) | Worker threads (sMain +0x9C). The setter 0x00DBD620 clamps. |
| CPU RenderingThread | ON, OFF | Separate render thread (sRender +0x26). |
| Window MainX, MainY | integer, default 32 | The window position, saved at close. |

`[KEYBOARDMOUSE]` holds mouse speed and inversion plus one key code per
action. The pad sections hold CameraBaseSpeed, InvertX/Y, Deadzone and
the button map per pad product. The input objects own these, outside L34.

There are no `RenderFps` or `FrameRate` keys: `FrameRate` is a DTI
property group of `sMain`, not a config.ini option.

## Display modes in the rebuild

- The adapter answers GetAdapterModeCount, EnumAdapterModes and
  GetAdapterDisplayMode from the host's own Direct3D 9: crate
  `hostdisplay`, asked once per session. `DDRUNTIME_DISPLAY=<file>`
  replays a recorded display, and `d3d9::set_display` sets one for tests.
- The old fixed 1920x1080 / 60 Hz list was not a recording. On the
  maintainer's machine Direct3D 9 reports a 2560x1440 / 240 Hz desktop
  with 161 X8R8G8B8 modes (research/runtime/windows/display_modes.py).
  That list is EnumDisplaySettingsW's 32-bit modes, each one once, in
  first-seen order.
- While the game's device is full screen, the current mode (what
  GetAdapterDisplayMode, GetDisplayMode and GetSystemMetrics report) is
  the full-screen mode.
- Full-screen present parameters must name a host mode: size, X8R8G8B8,
  and a listed refresh rate or 0. For 0, the runtime picks the desktop's
  rate when the mode has it, else the first listed rate (**runtime
  choice**: Direct3D 9 leaves the default rate to the driver).
- `d3d9dev::presentation()` gives a host window the game's choice: back
  buffer size, windowed or full screen, refresh rate, vsync interval.
  ddplay is to follow it (coordinated with L25).

## Frame rate, for the player

- The game's simulation step is in units of 1/30 s. Its frame cap is
  MaxFPS. The original's menu offers 30, 60 or 150.
- Higher caps work as data: set `MaxFPS` in config.ini, up to 999 through
  the item and higher through the start-up read. This is not a code
  change, but several systems are known to behave differently above 60
  ([FRAME_RATE.md](FRAME_RATE.md), L30):
  - enemy recognition becomes slower (4.1);
  - cloth drops to 80 % at 144 fps (4.2);
  - jumps are a few percent lower (4.3);
  - Frozen and Petrify braking scale with the frame rate (4.3);
  - effects that use noise on the frame counter flicker faster (4.5).
  - Capcom capped the PC release at 150 "to avoid potential game-breaking
    issues".
- VSYNC=ON presents at the display's refresh rate (D3DPRESENT_INTERVAL_ONE).
  The cap still applies on top.
- The opt-in fixes for a high-frame-rate mode are proposals in
  FRAME_RATE.md section 6. The rebuild stays 1:1 by default.

## Status

- **Implemented:**
  - the census;
  - host display modes;
  - the full-screen checks;
  - the config.ini import;
  - the path documentation.
- **Tested** (`crates/ddruntime/tests/options.rs`, `crates/hostdisplay`
  tests): written; they run in the next integration build (merge train).
  - In that test, the game's own writers produce config.ini as the
    original wrote it.
  - The game's readers read it back, including from an imported file.
- **Not compared live:** the start-up order, the first-run menu write, and
  the effect of each option on the image.
