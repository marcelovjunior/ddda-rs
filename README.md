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

Hold the right mouse button to turn the camera. F2 toggles the town/field
walk.

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

The Arisen fights with the kit of the save's primary weapon: a staff plays
the Mage's, anything else the Fighter's sword. `--kit fighter|mage`
overrides it.

The bars at the top left are a viewer overlay, not the game's GUI: HP and
stamina, the pawn's HP, and the nearest goblin's HP.

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
