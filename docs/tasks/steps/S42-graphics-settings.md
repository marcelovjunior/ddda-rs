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
