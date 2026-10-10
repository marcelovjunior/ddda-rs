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
**Not complete, not compiled/tested, no quality capture yet.** Ghidra stayed
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
| V-Sync | DISPLAY.VSYNC; EDDB20 ON/OFF | typed, live present-mode hookup pending |
| Framerate | GRAPHICS.MaxFPS; EDDC79–EDDCAE selects 30/60/150 (Variable) | preserved, pacing unsupported |
| Anti-Aliasing | GRAPHICS.AltAntiAlias; EDDB20 NONE/FXAA/FXAA3/FXAA3HQ | preserved; three native FXAA variants not equated with Bevy defaults |
| HDR | GRAPHICS.HDR; EDDB20 DEFAULT/FLOAT | preserved, native color-format mapping open |
| Shadow Quality | GRAPHICS.ShadowQuality; LOW/MEDIUM/HIGH | typed; stable allocation EDFB50→DA97D0(0), table14292BC:512/1024/2048; live hookup pending |
| Texture Detail | GRAPHICS.TextureDetail; 49ADC0 game override HIGH0/MEDIUM1/LOW2/other3 | preserved; resource-specific native mip selection still open |
| Texture Filtering | GRAPHICS.TextureFiltering; EDDD83–EDDE08 | typed TRILINEAR/ANISO_X2/4/8/16; live sampler hookup pending |
| Grass Quality | GRAPHICS.GrassQuality; EDE324 reader LOW/MEDIUM/HIGH | preserved; density/renderer behavior open |
| Effect Volume | GRAPHICS.EffectVolume; EDDE0D writer LOW/MEDIUM/HIGH | preserved; particle admission behavior open |
| Camera FOV | GRAPHICS.CameraFov; EDE2D2–EDE2FF clamps 0..15, constant142AC80 | typed; mutable CameraConfig source handoff pending independent acceptance |
| Distance Scaling | GRAPHICS.ViewRange; EDDE65 NORMAL/FAR/FARTHEST | typed native1/2/3; loader/living-LOD hookup pending |
| Depth of Field | GRAPHICS.DofFilter; EDDE9E ON/OFF, filter enable50ADC0 | typed; existing DdPost toggle hookup pending |
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
save/load with a rejected candidate preserving prior state. **Unexecuted.**

Next: get exact shared main/pause/event/stage hooks agreed and claimed,
integrate the independently accepted CameraConfig checkpoint, implement the
first sourced live controls with native menu admission/layout, then run the
full viewer suite/release under the normal M5 lease and capture each quality
level. Head folds accepted evidence into MILESTONES; no shared docs edited.
