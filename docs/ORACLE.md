# The oracle: an in-process probe for the running game

`research/ddda-oracle` lets agents explore the original DDDA.exe without
touching the keyboard, the mouse or the window. A 32-bit DLL is loaded into
the running game. It hooks the game's per-frame call and D3D9 `Present`, and
serves line-based JSON commands on `127.0.0.1:47800`.

Only the agent that owns the board area `live/game` drives the game. Other
agents ask that agent for live checks.

## Status (2026-10-09)

| Capability | State | How it was checked |
| --- | --- | --- |
| Inject, unload | verified | Injected and unloaded with the game running. After unload, every patched byte and pointer was read back externally and matched the original. |
| Main-thread commands | verified | Frame hook ticks at the game's frame rate (about 60 to 70 per second while focused). |
| Frame capture to PNG | verified | 1920x1080 A8R8G8B8 back buffer, 18 ms grab. The images show the game, the HUD and the Pause Menu. |
| Memory read/write/scan | verified | Reads, main-thread writes, and a vtable scan of the whole heap in about 2 s. |
| Guarded calls | verified | Game getters return the right values. An access violation in game code (0x00435680 with a bad `this`) and in the probe's own copy (write to 0x10) both came back as errors, and the game kept running. |
| Teleport | verified | Wrote `uPlayer.mPos.x += 200` on the main thread. The new position held for more than 100 frames (y settled 2 cm onto the ground), and the before/after captures show the move. |
| Keyboard input | verified | Focused: Esc opens and closes the Pause Menu, and W held for 30 frames walks 1.6 m. Inactive (`sApp+0x2566 = 0`): W for 30 frames walks 2.1 m. |
| Running unfocused | verified | The window really lost focus (foreground false, `sApp+0x2566 = 0`): 205 frames in 3 s (68 fps), and the capture showed the pawns moving. A simulated deactivation (writing `0x2566 = 0` on the main thread, then 1 again) gave 57 fps. |
| Talk to NPC | implemented, unverified | No NPC was loaded near the player. |

## Use

Build once. This needs the `i686-pc-windows-msvc` target; the oracle has its
own Cargo workspace and does not touch the main one.

```
rustup target add i686-pc-windows-msvc
cd research/ddda-oracle
cargo build --release          # target/i686-pc-windows-msvc/release/{ddda_oracle.dll, ddda-inject.exe}
```

Then:

```
python client.py inject        # no-op if the server already answers
python client.py info
python client.py capture out.png
python recipes.py party
python recipes.py teleport 200 0 0
python client.py unload
```

The injector copies the DLL to `%TEMP%\ddda-oracle\ddda_oracle_<ms>.dll`
before loading it, so the build output is never locked and a rebuilt DLL can
be injected after an unload. The probe's log is
`%TEMP%\ddda-oracle\oracle.log`. A named mutex (`Local\ddda-oracle-<pid>`)
refuses a second instance.

Before the first injection of a session, back up the save
(`C:\Program Files (x86)\Steam\userdata\124651925\367500` to
`E:\ddda-re\save-backups\<timestamp>\`).

## Protocol

Send one JSON object per line and get one JSON object per line back. `id` is
echoed. Replies carry `"ok": true` or `"ok": false, "error": "..."`. Numbers
may be JSON integers or `"0x..."` strings. A float argument is written as
`{"f": 1.5}` and sent as its f32 bit pattern.

| cmd | arguments | reply |
| --- | --- | --- |
| `ping` | | `pong` |
| `info` | | frames, idles, presents, sapp, active (sApp+0x2566), bg (sApp+0x20), hwnd, foreground, device, patches, module |
| `read` | `addr`, `len` (at most 16 MiB) | `hex`. Uses ReadProcessMemory on the game itself, so a bad address gives an error, never a fault. |
| `write` | `addr`, `hex`, `main` (bool) | `main: false` writes now from the server thread (WriteProcessMemory). `main: true` writes on the main thread between frames, with fault guarding. |
| `call` | `addr`, `args` [..], `conv` (`cdecl`, `stdcall`, `thiscall`, `fastcall`), `this`, `float` (bool), `timeout_ms` | `eax`, `edx` (and `st0` if `float`). Runs on the main thread at the next tick, guarded. The stack is restored from a saved esp, so a wrong argument count or convention does not unbalance it. |
| `scan` | `values` [u32..], `limit`, `image` (bool) | `hits`: addresses holding each value. By default it skips the exe image and the probe itself. |
| `capture` | `path`, `timeout_ms` | `path`, `w`, `h`, `format`. The back buffer is grabbed in the next `Present`. |
| `wait` | `frames`, `timeout_ms` | Returns after that many game frames. |
| `bg` | `on` (bool) | Turns background running on or off. |
| `keys` | `down` [vk..] | Holds exactly these virtual keys until changed. |
| `tap` | `vk` (one or a list), `frames` | Presses for that many frames. |
| `release` | | Releases all scripted keys. |
| `input` | | Scripted key state. |
| `trace` | `fields` [[addr, len]..], `frames`, `script` [[frame, [vk..]]..] | `rows`: one hex string per game frame (the fields concatenated), sampled right after each sMain::vf06. Script entries set the held keys from that frame index on; keys are released at the end. This is the tool for per-frame logs of a scripted action. |
| `patches` | | Applied in-memory patches. |
| `unload` | | Undoes everything (on the main thread) and frees the DLL. |

`client.py` wraps these (`Oracle().read/u32/f32s/write/call/scan/find/capture/wait/bg/unload`).
`find(cls)` resolves an MtDTI class name to its vtable
(`E:\ddda-re\notes\dti-registry.json`) and scans for it. Heap tables contain
stray copies of vtable values (about seven per class), so a hit is checked
with a plausible field, or a singleton is reached through its static pointer
(below).

## Asking for a live check

Only the owner of `live/game` (claude-4 as of 2026-10-09) drives the game.
Post a `request` to the owner with:

- **What to read.** Give a global and a chain of offsets
  (`*(u32*)0x018fa4bc + 0xb876c`), or an MtDTI class and field
  (`uPlayer +0x2dd4`), with the type (u8/u32/s32/f32/vector3) and the count.
- **When.** Say whether it is a one-off read or per frame during an action.
  For per-frame logs, name the fields and the action: keys and their frame
  timing (`W held 40 frames, then Space`), or a place (cell plus local x/z,
  or "a ledge 1-2 m high"). Keys are camera-relative, so prefer a target
  ("toward the block at x, z") over a raw direction.
- **Calls.** Give the function address, the calling convention, `this`, the
  arguments, and whether it returns a float.
- **State changes.** Say whether the check changes game state (position,
  items, quests, time). The owner backs up the save first and avoids
  spending resources or advancing quests unless the request needs it.

Replies quote the values and, for traces, put the full table in
`E:\ddda-re\notes\oracle\` (private research tree, not committed).

Action ids seen at `uPlayer +0x2dd4`:

| Id | Action |
| --- | --- |
| 0 | idle |
| 1 | stop (seen once) |
| 2 | move |
| 7 | jump start |
| 9 | airborne |
| 13 | falling (after a teleport into the air) |
| 14 | landing |

*These names come from the observed sequences, not from the exe.*

### Finding a place: `ddda-ledges`

`target\i686-pc-windows-msvc\release\ddda-ledges.exe <nativePC> <A0> <B0> <x> <y> <z> [radius] [min] [max]`
lists vertical steps of `min..max` cm whose top is a walkable shelf. It
works from the merged stage collision of the 3x3 cells around the
floating-origin cell (A0, B0), and its output is in the same local frame as
`uPlayer.mPos`. `LEDGES_GRID=1` prints a height grid instead.

Placed scenery (rocks, buildings from layouts) is **not** in that mesh. A
"block" it reports can be part of a building, so check with a capture.

## How it works

### Frame tick (main thread)

The main loop `0x00df0d20` (sApp, global `0x018d0f50`) calls
`sApp+0x2558` (the sMain object, a live **sBbsRpgMain**):

- `vf06` (`0x0041dd60`, slot 6) every frame, at `0x00df0e54`;
- `vf11` (`0x0041dd20`, slot 11) every iteration while the window is
  inactive, followed by `Sleep(1)` (at `0x00df0dfe`);
- `vf09` / `vf10` on activation and deactivation.

The probe gives that one object a private copy of its vtable, with slots 6
and 11 pointing into the DLL. The hooks run queued jobs and then the
original. Installing and removing the hook is one aligned pointer write.
Commands therefore run on the game's main thread between frames, even while
the game is paused for lack of focus (through vf11).

### Focus and background running

Evidence from the wndproc `0x00df1550` and the readers of the flag:

- `sApp+0x2566` (u8) means "window active". `WM_ACTIVATE` and
  `WM_ACTIVATEAPP(1)` set it; `WM_ACTIVATEAPP(0)` clears it through
  `0x00df0ed0`.
- The main loop sleeps instead of running a frame when
  `sApp+0x20 == 0 && sApp+0x2566 == 0` (`0x00df0de9`). `sApp+0x20` is zeroed
  in sApp init (`0x00df2f30`) and never set by the game. It is a dormant
  "run in background" switch.
- Input readers check `sApp+0x2566`:
  - sKeyboard::vf06 (`0x00df412f`, GetAsyncKeyState / DirectInput);
  - sMouse::vf06 (`0x00e04f71`);
  - sQlocInput::vf06 (`0x00409f4b`, `0x0040a0be`, `0x0040a0ce`), which
    wipes the game buttons while inactive;
  - `0x0040a290`;
  - rumble (`0x0040cffc`, `0x0040d085`).

The probe sets `sApp+0x20 = 1` and leaves `0x2566` alone. The game then runs
while unfocused, but still ignores the maintainer's keyboard and mouse.

One routine is not gated. sMouse::vf06 calls `0x00e04d60` every frame
(its only caller is `0x00e04f66`). That routine calls
`ClipCursor(game window)` and changes the system-wide mouse acceleration
(`SPI_SETMOUSE`). Running frames in the background would trap the
maintainer's cursor. The probe retargets that call to a gate that runs the
original only while `sApp+0x2566` is set. Background mode refuses to switch
on without the gate.

Sound is not resumed: deactivation (`vf10`, sound through `0x00dc93c0`)
sets a suspend flag that only activation (`vf09`) clears, and background
mode does not trigger `vf09`. *Expected from the code; not yet observed.*

### Present and capture

The D3D9 device is at `sRender+0x98` (sRender global `0x018d08f4`; it is
used for `TestCooperativeLevel` in `0x00daf7c0`). The device's vtable is its
own heap copy. Slot 17 (`Present`) is replaced; the Steam overlay's inline
hook inside d3d9's `Present` keeps working because the original pointer is
called.

`Present` runs on the main thread. The capture path:

1. `GetBackBuffer`;
2. `StretchRect` to a plain render target, if the back buffer is
   multisampled;
3. `CreateOffscreenPlainSurface` (system memory);
4. `GetRenderTargetData`;
5. `LockRect`, converted to RGB.

PNG encoding happens on the server thread.

### Guarded calls

A naked x86 caller saves esp, pushes the arguments, sets ecx and edx, calls
the function, then restores esp. A vectored exception handler is active only
on the calling thread during a guarded call. It turns an access violation,
illegal instruction, divide error or similar into a jump to a landing pad
that restores the saved stack. The game can still be left inconsistent (a
lock held, a half-written object), so this is a seatbelt, not a licence.

### Keyboard input

sKeyboard::vf06 (`0x00df40e0`) clears a 256-bit VK bitmap at
`sKeyboard+0x120` and fills it through `0x00df3ef0`, but only while active.
It then derives the trigger and release edges. On the first input command
the probe (on the main thread):

- points the four sQlocInput reads of `sApp+0x2566` and the sKeyboard read
  (`0x00df412f`) at its own flag `INPUT_ON`. The patched instructions have
  the same length (`cmp byte [abs32], imm8` and `cmp byte [abs32], bl`).
  `INPUT_ON` = really active, or a script holds keys (plus 3 frames after
  release, so the game sees the release edge);
- retargets the call at `0x00df4140` to a reader that calls the real
  `0x00df3ef0` only while really active, then ORs in the scripted keys and
  folds L/R Shift, Ctrl and Alt into VK 0x10 to 0x12, as the game does.

sMouse is not redirected: the maintainer's mouse never reaches the game.
Pads are not filtered: a physical pad in use during a background script
would reach the game too.

### Unload

The `unload` job runs on the main thread. It:

1. releases scripted keys;
2. restores `sApp+0x20`;
3. reverts every code patch, newest first;
4. restores the `Present` slot and the sBbsRpgMain vtable pointer.

A reaper thread then:

1. waits until no `Present` hook is in flight;
2. stops the server threads;
3. removes the exception handler;
4. calls `FreeLibraryAndExitThread`.

If the main thread does not tick within 5 s, it is not inside any hooked
path, so the teardown runs from the server thread instead.

## Recipes (`research/ddda-oracle/recipes.py`)

The singletons were found live: heap-scan for the class vtable, then scan
the exe image for a pointer to the hit. Two of them, sGameSys and sUnit,
match the stores in their constructors, which confirms the method.

| Global | Class |
| --- | --- |
| `0x018d0f50` | sApp |
| `0x018d08f4` | sRender (sRenderExt) |
| `0x018d099c` | sArea (sAreaExt) |
| `0x018d09e0` | sUnit (sUnitExt); ctor store in `0x00db6060` |
| `0x018d0b6c` | sCamera (sCameraExt) |
| `0x018fa4bc` | sGameSys; ctor store in `0x00437be0` |
| `0x018fa4e8` | sPawnManager |
| `0x018fa4ec` | sPlayerManager |
| `0x018fa520` | sWeatherManager |
| `0x018fa528` | sWorldOffset |

**Player position and teleport.**
- The player is `sPlayerManager+0x99c` slot 0 (uPlayer, vtable `0x015e90d0`).
- Its position is `uCoord.mPos` at `+0x40` (vector3, cm, floating-origin frame).
- Teleport means writing `mPos` on the main thread (`write ... main=true`).
  The character snaps to the ground on the next frame. Checked for a 2 m
  move on flat ground only.
- Use `recipes.safe_teleport` (the rules below), never a raw `mPos` write.

**Incident, 2026-10-09: a teleport killed the Arisen.**

What happened:
1. The player stood in origin cell st100_63m52n at local
   (177.7, 32317.3, 1930.6).
2. `mPos` was written as (-1323, 32330, 3430). That is 15 m away and
   across the cell boundary (x < 0) into 63m51n, 20 to 75 cm above that
   cell's collision ground.
3. Within 60 frames:
   - the floating origin re-centred (x became 8720);
   - HP dropped to 0;
   - the action went to 30, then 38;
   - the death menu came up.
4. Writing HP back did not help. Head chose plain "Retry", which reloads
   the last save.

The cause is unproven: either the cross-cell move (floating-origin
re-centring) or a kill or out-of-bounds check.

Rules since then, set by head:
- Never teleport across a cell boundary.
- Teleport at most a few metres, onto ground found in the current origin
  frame.
- Watch HP and the action for 30 frames after every teleport, and report
  anomalies.

`recipes.safe_teleport(o, x, z)` enforces all three:
- it refuses targets outside local 0..10000 or more than 5 m away;
- it takes y from the stage collision (`ddda-ledges` with `LEDGES_AT=1`,
  plus 5 cm);
- it stops at the first HP drop or unexpected action.

Back up the save before any route that moves the player.

**Party.**
- `sPlayerManager+0x99c` holds 4 `uPlayerBase*`: the Arisen (uPlayer) and
  three pawns (uCmc, vtable `0x015e8468`).
- Each object points at its character record inside sGameSys through
  `+0x3dec`. In the record, `+0x700` is the level and `+0x70c` the name.
- Live result: Shai (Arisen, level 7), June (7), Eloyse (4), Ro (6).
- The Arisen's record is `sGameSys+0xa76d0`; the pawns follow at a stride
  of `0x1660`.
- *Slot 1 is presumably the main pawn: its level matches the Arisen's.
  Unverified.*

**Stage and area.**
- `sArea.mpArea[i]` at `+0x382c + 4*i` (getter `0x00412f50`) is the area
  stack. Live: [aBbsRpg, aGameMain, aStage100]. The class comes from the
  vtable.
- `sWorldOffset+0x50` (`mWorldAbsoluteOffset`) is minus the origin cell's
  position in the fixed layout frame (origin at cell 50m50n,
  `docs/WORLD.md`). So the origin cell is
  `st100_<50 - z/10000>m<50 - x/10000>n`.
- Checked live: offset (-20000, 0, -130000) gives st100_63m52n, and the
  loaded uStageSplitMdl at local (0, 0, 0) has the model
  `scr\st100\model\m60\st100_63m52n`.
- sWorldOffset also holds 22 and 23 at `+0x2c` and `+0x30` (`M`). Their
  meaning is unresolved: perhaps the cell index minus (30, 40).

**Time of day.** In sGameSys:

| Offset | Field | Getter |
| --- | --- | --- |
| `+0xb8768` | mGameTime | `0x004372a0` |
| `+0xb876c` | GameHour | `0x00435680` |
| `+0xb8770` | GameMinute | `0x004372b0` |

The setter `0x00437270` sets mGameTime and recomputes hour and minute.
Checked: the getters, called through the probe, return the field values.

**Open a menu.** `recipes.py menu` taps Esc for 4 frames, which toggles the
Pause Menu (Save / Quit, History, Quests, Map, Equipment, Status, Options,
Share). Checked with captures, opening and closing.

**Talk to the nearest NPC.**
- `recipes.py npcs` lists uNpc and uMultiNpc objects by distance.
- `recipes.py talk` taps E (Converse, the PC default shown in the HUD) when
  one is within 3 m.
- *Unverified*: no NPC was loaded at the test spot (outskirts, at night).

## Open problems

- Sound while running in the background: expected to stay suspended (see
  above), not checked by ear.
- Pad and mouse injection are not implemented. XInputGetState is imported at
  IAT `0x0139d40c` (ordinal 2) and DirectInput8Create at `0x0139d038`.
- The meaning of `sWorldOffset+0x2c` and `+0x30`.
- Which party slot is the main pawn.
- Talk recipe: verify next to an NPC.
