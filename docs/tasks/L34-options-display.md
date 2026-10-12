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

(open)
