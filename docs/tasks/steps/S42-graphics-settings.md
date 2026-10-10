# S42: Graphics settings like the original's options menu

- **Branch:** `step/s42-graphics-settings`
- **Files:** `crates/ddview` (a settings module and menu), config file
  handling; tests
- **Kind:** research + one feature
- **After:** S41's measurements help choose what each setting changes

## Context

The original PC release has graphics options (in its in-game options and
`config.ini`): the maintainer names depth of field, shadow quality and
texture detail; others likely include resolution/window mode, V-sync,
frame rate, anti-aliasing, ambient occlusion, etc. The viewer has no
settings: everything is fixed.

## Do

1. List the original's graphics options exactly: the `config.ini` keys
   and values of the installed game (read-only), the options GUI's
   entries and messages (the GUI class and layout), and what each one
   changes in the renderer (shadow map size, texture mip bias, DoF on/off,
   AA mode...), from the exe where it matters.
2. Implement the same settings in the viewer with equivalent effects in
   Bevy, persisted in our own config file (never write the game's
   `config.ini`), applied live where the engine allows.
3. A settings menu reachable from the pause menu, drawn like the
   original's options screen if its layout is available, else a plain
   labelled menu for now.
4. Tests for loading/saving settings and a capture per quality level.

## Done when

The viewer offers the original's graphics options with equivalent effects,
they persist between runs, and the Result maps each option to its
original key and to what our renderer changes.

## Results — source research checkpoint (2026-10-10)

Codex-73, `step/s42-graphics-settings`, based on committed main `f6ec992`.
**Initial source checkpoint was untested; actual acceptance is below. The
whole brief is incomplete and quality captures are pending.** Ghidra stayed
stopped under head's memory directive; the following addresses were checked
with private Capstone reads of the analysis executable and existing evidence.
Raw executable data, resources and disassembly remain outside the repo.

Original config read-only:
`%LOCALAPPDATA%/CAPCOM/DRAGONS DOGMA DARK ARISEN/config.ini` has 16 GRAPHICS
keys and five DISPLAY keys on this installation. GRAPHICS keys are
`AltAntiAlias`, `HDR`, `TextureDetail`, `ShadowQuality`, `MaxFPS`,
`GrassQuality`, `CameraFov`, `TextureFiltering`, `EffectVolume`, `Brightness`,
`ViewRange`, `DofFilter`, `AntiAlias`, `SLI`, `Stereo`, `TextureMipLimite`.
DISPLAY keys are `Resolution`, `RefreshRate`, `VSYNC`, `FullScreen`, `Flush`.
Window's MainX/MainY are a different section. The importer preserves
GRAPHICS/DISPLAY only.

`uGUIOption::vf58` **0x006A6160** loads the installed layouts
`id/DDN/menu/option/top_option` and `id/DDN/p_option/p_option`, and localized
`id/DDN/message/pause/pause_option/pause_option_<language>`. The English
message resource exposes display labels at IDs 139–151, Distance Scaling
233 and Depth of Field 237, with descriptions 152–164/234/238. These texts
and both layouts are loaded from the player's installation, not copied.
Top layout is authored at 1280×720 with 170 instances. Full widget state,
scrolling, native menu drawing and original comparison remain open.

| Option | Config key / original evidence | Renderer hookup status |
|---|---|---|
| Screen Resolution | DISPLAY.Resolution; EDDB20 uses runtime display-mode list | preserved, unsupported |
| Fullscreen | DISPLAY.FullScreen; EDDB20 ON/OFF | preserved, unsupported |
| Refresh Rate | DISPLAY.RefreshRate; EDDB20 runtime display modes | preserved, unsupported |
| V-Sync | DISPLAY.VSYNC; EDDB20 ON/OFF | implemented live present-mode policy; uncapped CLI wins |
| Framerate | GRAPHICS.MaxFPS; EDDC79–EDDCAE selects 30/60/150 (Variable) | preserved, pacing unsupported |
| Anti-Aliasing | GRAPHICS.AltAntiAlias; EDDB20 NONE/FXAA/FXAA3/FXAA3HQ | preserved; three native FXAA variants not equated with Bevy defaults |
| HDR | GRAPHICS.HDR; EDDB20 DEFAULT/FLOAT | preserved, native color-format mapping open |
| Shadow Quality | GRAPHICS.ShadowQuality; LOW/MEDIUM/HIGH | implemented live allocation; stable EDFB50→DA97D0(0), table14292BC:512/1024/2048 |
| Texture Detail | GRAPHICS.TextureDetail; 49ADC0 game override HIGH0/MEDIUM1/LOW2/other3 | preserved; resource-specific native mip selection still open |
| Texture Filtering | GRAPHICS.TextureFiltering; EDDD83–EDDE08 | implemented live world samplers TRILINEAR/ANISO_X2/4/8/16 |
| Grass Quality | GRAPHICS.GrassQuality; EDE324 reader LOW/MEDIUM/HIGH | preserved; density/renderer behavior open |
| Effect Volume | GRAPHICS.EffectVolume; EDDE0D writer LOW/MEDIUM/HIGH | preserved; particle admission behavior open |
| Camera FOV | GRAPHICS.CameraFov; EDE2D2–EDE2FF clamps 0..15, constant142AC80 | implemented mutable CameraConfig; paused edits applied on resume |
| Distance Scaling | GRAPHICS.ViewRange; EDDE65 NORMAL/FAR/FARTHEST | persisted native1/2/3; applied at startup; live reload unavailable |
| Depth of Field | GRAPHICS.DofFilter; EDDE9E ON/OFF, filter enable50ADC0 | implemented installed DdPost filter toggle in native depth-bearing lighting mode |
| Brightness | GRAPHICS.Brightness; EDDE46 writes float, EDE4F7 reader default10 | preserved; native transfer function not guessed |

`AntiAlias` is distinct from `AltAntiAlias` in the render registration
DB1FDB/DB2062. `SLI`, `Stereo`, `Flush` and `TextureMipLimite` are config-only
keys not established as widgets of the options screen. They remain preserved
without claims of implemented effects. No ambient-occlusion or motion-blur
option was invented from the brief's possible-options list.

Texture caution: sScene's base registration FECD40/table143A6A0 lists five
tokens HIGHEST/HIGH/MEDIUM/LOW/LOWEST, whereas sSceneExt's 49ADC0 game
constructor maps HIGH/MEDIUM/LOW to 0/1/2 and other tokens to 3. The options
reader EDE1ED maps those three gameplay grades back to its three choices.
49BBD0 supplies the game's grade to rTexture::vf10 E6B1C0, which also
uses resource flags (+50), header resident bias, a minimum-mip floor, and
per-resource overrides. A universal mip drop based only on TextureDetail
would omit those inputs; it has **not** been installed in the viewer.

`settings.rs` prepares a section-aware versioned INI store, graphics-only
read-only import, fixed project destination
`%LOCALAPPDATA%/ddda-rs/ddview.ini`, complete-file replacement and six typed
controls. Absent settings remain absent rather than choosing guessed native
defaults. Unsupported original keys survive a supported-setting edit.
Regression sources cover roundtrip/preservation/import isolation, duplicate
and malformed values, native allocation/anisotropy enums, and real filesystem
save/load with a rejected candidate preserving prior state. These were
unexecuted at the initial checkpoint; see actual acceptance below.

### Follow-up implementation

Head #6923 approved the first feature checkpoint; exact normal claims cover
main/pause/stage hooks. `35e8738` adds a temporary plain viewer menu reached
from pause, using runtime original labels; native GUI widget drawing remains
unported. Unsupported controls are visibly unavailable and do not change
settings. Esc returns to the still-paused menu. Hidden menu text does no
per-frame work; a failed resource load cannot open a missing menu.

V-Sync changes window present mode (explicit uncapped/perf CLI retains
priority). Shadow Quality sets Bevy directional shadow-map resolution;
caster culling, cascades and distance remain unchanged. Filtering applies
native anisotropy levels to world Loader images, preserving address modes,
compressed payload, mip chains, dimensions and formats. Live filter edits
re-read world textures into the same asset IDs and refresh material bindings
because render-only CPU payloads may have expired; HUD/fonts/movies are
excluded. DOF uses the installed stage filter and current lens planes,
retaining the original tone/vignette state. It requires the native material's
depth-bearing alpha output; fallback PBR alpha is never treated as depth.

Distance Scaling persists and applies at startup, **labelled as requiring
restart**. Static Loader::spawn_parts discards other LOD meshes; a visibility
toggle cannot recover them or change already streamed cells. Live range
reload remains open.

Independently reviewed CameraConfig `44b3f0a`/`203ad96` passed codex-66's
actual full suite (91 PASS, two existing ignored) and release (40.71 s,
exit 0), reported #6976/#6978. It is integrated here as `0088c67`/`bb132bc`.
S42 compares the last applied FOV offset instead of a transient Bevy Changed
tick, retaining edits made while paused until resume. Its new regression
runs the actual shared refresh helper across paused updates and resume.
Startup CLI FOV overrides survive other setting edits. Temporary viewer FOV
buttons step in whole degrees; only the native 0..15 range is sourced, its
authored slider/controller widget remains unported.

New regression sources also cover compressed-image sampler preservation
and pause entry/Esc return. The earlier accepted camera result alone was not
acceptance of S42; the following runs exercise the current settings source.

### Actual viewer test acceptance

Source `dff826c`, including own-branch reviewed-main merges `0427a36` and
`35e8431` (head's S54 main `5858ed0`). No main mutation occurred.

`cargo test --release -p ddview --target-dir
E:/ddda-rs-wt/m5-cassardis/target` completed with **actual exit 0** in
32.87 s: **97 passed, zero failed, two existing ignored, zero filtered**.
All seven settings regressions executed, including the installed native
option messages/layout/shadow-table test, real project-file replacement,
invalid-edit rollback, compressed mip/sampler preservation, and shared
DOF/presentation policy. The actual paused-FOV refresh helper regression,
multiple-follow-view camera regression and pause menu entry/Esc regression
also passed. The ignored tests require private camera samples or explicit
joint-remap installation admission. This is unit/install acceptance, not an
original screen comparison or a GPU interaction check.

The first compile exposed an ambiguous FOV parser error type, fixed in
`3840bca`. The next full run had 96 PASS/one FAIL/two ignored: the installed
resource test rejected the incorrect `rMessage` hash. `dff826c` corrects it
to `rGUIMessage`, matching the other installed message loaders. Both GUI
resources are read and parsed at runtime; the temporary menu copies only
labels and discards the unused raw layout buffers.

Brightness follow-up remains source research: `6B34D0` stores
`1+(Brightness-10)*1.6*0.05` in sRender+D8 and marks DC dirty.
`DA9CFE–DA9D26` supplies its reciprocal to `DA6D00`, which conditionally
builds a 256-entry 16-bit RGB ramp through `12F2F20`, with inputs stepping
by 1/256 and scale 65535, then calls device vtable+54. `DA87A0` also writes
the reciprocal to a GPU constant. The math helper and consuming shader are
not established, so Brightness stays unavailable; no guessed gamma or
linear brightness effect was added.

`cargo build --release -p ddview --target-dir
E:/ddda-rs-wt/m5-cassardis/target` completed with **actual exit 0** in
40.55 s. The existing Args/opening_world visibility and unused audio
method warnings remain. A private executable was copied and SHA-256
checked before the build lease was released:
`2708723855666EB5305FB7B3DCD8B066349E479FD55476FBCD48DBB63C337C6A`.
The executable stays outside the repository. No settings GPU capture or
original comparison has run yet.

Next: coordinated private quality captures after queued palette builds,
using the frozen pin and isolated project config. The whole S42
Done gate remains open for native menu drawing, unsupported options, live
range reload and original comparisons. Head updates MILESTONES; no shared
docs edited.

### Parent integrated captures and viewer-label correction

Head's integrated main `7ea18c9` contains the reviewed settings source plus
the current rescue and equipment fixes. Private executable SHA-256
`F4DD8720AF28FE1AFE76E7793B9D468B7A79182F3DF2F3F74EE7DCCC0EF85E64`
was checked before each case. The bounded MENU and LOW cases actually
exited zero (owned processes 30576 and 4884, 47.72 and 47.38 seconds).
Both PNGs were independently inspected: terrain had loaded; the plain
menu showed supported values, unavailable controls and the restart-only
range label. These are presentation checks, not live interaction,
performance or original-game comparisons. Each case used its own private
project INI, importing the original GRAPHICS/DISPLAY sections read-only.
Private evidence: `E:/ddda-re/notes/steps/S42/main-7ea18c9-menu-low`.
The original process 23408 remained running; both owned viewers exited
before the normal GPU lease was released.

The menu PNG exposed missing-glyph boxes for the viewer-owned em dash and
degree suffix. Under head's board #7309 follow-up, these three temporary
labels now use ASCII separators and `deg`; installed option messages are
unchanged. This is a small presentation correction, not a native font or
GUI interpreter implementation. Its rebuilt visual check is pending.
MEDIUM/HIGH shadow-resolution captures and the whole original comparison
remain open; the completed LOW capture alone does not close those gates.
