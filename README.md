# ddda-rs

A Rust reimplementation of **Dragon's Dogma: Dark Arisen** (PC, 2017
Steam build), aiming at a 1:1 game with two-player LAN co-op. Its
movement, rules, data and look are derived from the original game:
- its data files, read from the player's own install;
- its executable, read in Ghidra;
- recordings of the running original.

Nothing is invented or tuned by eye.

This repository contains no game data. You need your own copy of the game.

## Status

Early. The world renders (stage cells, scenery, water, blended terrain
layers), and two local players can walk, run, sprint, jump and climb
ledges in it. Most of the game's systems are researched and have tested
readers for their data. The playable features follow the milestone routes.

- [docs/MILESTONES.md](docs/MILESTONES.md): the plan, M0 to M10, each
  with a gate checked against the original, plus the current status.
- [docs/ROADMAP.md](docs/ROADMAP.md): every system, its status, docs and
  owner.
- [docs/tasks/](docs/tasks/README.md): work items (briefs).

## Quick start

```
cargo build --release -p ddview
target/release/ddview <path to DDDA>/nativePC --play
```

| | Player 1 | Player 2 |
|---|---|---|
| Move | WASD | arrow keys |
| Sprint | Left Shift | Right Shift |
| Walk | Left Ctrl | Right Ctrl |
| Jump | Space | Enter |

Move the mouse to turn the camera (right turns right; up looks up).
The cursor is grabbed and hidden during play. Esc opens a viewer pause
menu with Resume and Quit; Esc again resumes. Losing window focus also
opens the menu. F2 toggles the town/field walk.
`--paused` starts at the viewer menu.

## How to play

```
target/release/ddview <path to DDDA>/nativePC --play --save <path to DDDA.sav> [--goblin]
```

This starts where the save stands:
- at its stage, cell and position;
- lit by its game clock and weather (`--clock HH:MM` and `--weather f|e|r`
  override them);
- with the save's Arisen and main pawn, dressed and shaped from the edit
  data, carrying their sheathed weapons.

`--goblin` puts a goblin 6 m ahead; repeat it for more. The pawn follows
you and fights it.

| | Player 1 |
|---|---|
| Light attack | J or left mouse button |
| Heavy attack | K or middle mouse button |
| Skills | 1-6 (P2: Numpad 4-9), the save's skill slots; implemented: Broad Cut, Tusk Toss, Ingle, Levin |
| Use item | U (P2: Numpad 0), the first useful restorative in the save's bag |

The Arisen fights with the kit of the save's primary weapon: a staff plays
the Mage's, anything else the Fighter's sword. `--kit fighter|mage`
overrides it.

The HUD is the game's own, built from its GUI data: the health and stamina
bars, the pawn's bar and the action prompts. Not yet shown: pawn names and
icons, hired pawns, status icons and the minimap.

Offline, the viewer menu pauses simulation and animation. In a network
session it stops only your controls: networking and the shared world
continue. Release movement keys before resuming.

Manual movement, jump or attack takes over `--demo`.

`--demo` plays by itself: with goblins, the Arisen runs at the nearest one
and attacks.

**Co-op on a LAN:**
- One machine hosts: `ddview <nativePC> --host 47810 --save A.sav --goblin`.
- The other joins: `ddview <nativePC> --join <host address> --save B.sav`.
- The guest plays its own save's Arisen, or a stand-in without `--save`.
  Its world must be the host's place: the same save spot, or the same
  `--stage` / `--cell`.
- Each side sees the other's Arisen as a stand-in.
- Goblins and HP are not sent to the guest yet.

`target/release/ddtool` (run it without arguments for usage) inspects the
install: archives, textures, models, XFS objects, text, parameters, AI
graphs, sound, effects and more.

### Cassardis opening slice

`ddview <nativePC> --play --opening` starts at Newly Arisen after a
headless prologue fast-forward. Walk to the weapon table, press E, and
pick 1 Fighter or 2 Mage in the labelled vocation stub. Exit the room to
play ev10, speak to Quina with E (E advances lines; digits choose), then
walk out toward the village. The slice stops when q0002 boots.

`--demo --shots <temporary-directory>` runs the scripted route, captures
its steps, checks the endpoint flags and exits. Blocked paths are logged;
the demo never teleports through geometry. Do not treat the demo as an
original-game comparison.

Unattended test modes (`--unattended`, `--demo`, `--shot`, or `--shots`) keep running on
focus loss, leave the cursor free, and rotate the camera only while the
right mouse button is held. Normal play retains grabbed direct mouse look
and focus-loss pause. Esc remains available in both modes.

For this slice both machines use `--opening`: the host adds `--host 47810`,
the guest adds `--join <host-ip>:47810`. The guest sees host-owned NPCs,
players and shared cutscenes; dialogue/menu controls currently belong to
the host viewer. Esc pauses solo; network transport and shared events
continue while a local menu is open. The original game install is only
read. NPC layout gating and several schedule actions remain incomplete;
this is not the complete opening campaign.

## Layout

| Crate | What it is |
|---|---|
| `mtarc`, `ddvfs` | ARC archives; a path + type index over the whole install |
| `mttex`, `mtmod`, `mtmrl`, `mtshader` | textures, models, materials, shader package |
| `mtxfs`, `mtlot`, `mtlmt`, `mtsbc` | serialised objects, layouts, animations (with blending), collision |
| `mtgmd`, `mtgui`, `mtsound`, `mteffect`, `mtcamera`, `mtweather`, `mtfsm`, `mtprp` | text, GUI, sound, effects, cameras, weather, AI graphs, enemy params |
| `ddparam`, `ddenemy`, `dditem`, `ddpawn`, `ddsave`, `ddnpc`, `ddmap`, `ddquest` | typed game data: player, enemies, items and shops, pawns and the Rift, saves, NPCs, map and travel, quests and events |
| `ddsim` | headless simulation for N players (movement, jumps, ledges) |
| `ddworld` | world queries: ground, walls, rays, scenery collision |
| `ddview` | Bevy viewer and `--play` mode |
| `ddtool` | command-line inspection |

The `research/` folder holds the tooling used to study the original:
- `dti/`: the MtDTI class registry, plus reflected field names, types and
  offsets for 1,272 classes;
- `ghidra/`: Ghidra scripts;
- `ddda-oracle/`: an in-process probe for the running game (its own
  32-bit workspace; see [docs/ORACLE.md](docs/ORACLE.md)).

## How the work is done

- [docs/METHODOLOGY.md](docs/METHODOLOGY.md): decompilation-informed
  reimplementation, checked against the original.
- [AGENTS.md](AGENTS.md): the rules for every contributor, human or agent.
- [docs/BOARD.md](docs/BOARD.md): several agents work in parallel,
  coordinated on a shared board by a lead.
- Every rule carries its source address; every format reader is tested
  against the whole install.

## Legal

This project reads the game's files from the user's own installation at
run time. It does not include, redistribute or modify game files.
Addresses and behaviour descriptions refer to the 2017-12-13 Steam build.
Third-party notices are in [third_party/](third_party/).


Unattended viewer tests must use `--unattended`; `--demo` and `--shot`
also enable it automatically. These runs never grab or lock the cursor,
keep running after losing focus, and rotate the camera only while the
right mouse button is held. Normal play keeps direct grabbed mouse look
and focus-loss pause. Esc and the pause menu remain available in either
mode. Use `--unattended` for automated runs without a demo or screenshot.
