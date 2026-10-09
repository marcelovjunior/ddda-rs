# External sources

What the outside world already knows about DDDA, checked by head on
2026-10-09. These are leads only: every fact used in the code is traced in
the exe or measured on the data (AGENTS.md), and no third-party code is
copied (most of these projects have no licence).

## Projects

| Source | What it gives | Use |
|---|---|---|
| [Atvaark/DragonsDogma.Research](https://github.com/Atvaark/DragonsDogma.Research) (2017, no licence) | `types.tsv`: 4,406 type factories (caller, factory, name, size, vtable, reset); `packets.tsv`: the 34 packets of the game's own online protocol (line check, connect/disconnect, auth header/data/footer, **TUS** common/user area read/write) | names and sizes to check against our DTI dump; the online protocol map (see below) |
| [kubik-jaroslav/ddda-dinput8](https://github.com/kubik-jaroslav/ddda-dinput8) (2018, Nexus mod 96 "dinput8 hooks", no licence) | a d3d9/dinput8 hook mod: player stats, in-game clock, portcrystal positions, item editor, weapon sets, save backup, **server redirect** | runtime leads: the save-write hook, the clock words (hours/minutes as u32 next to a ms counter), portcrystal table (10 entries of 16 bytes), sDDCaProto::connect |
| [Lyall/DDDAFix](https://github.com/Lyall/DDDAFix) (MIT) | aspect ratio, FOV, HUD fixes | camera/FOV hook points, if ever needed |
| FluffyQuack's ARCtool and DDsavetool (not open source) | arc unpack/repack (needs file order for DD), TEX/XFS/LOT conversion; save <-> XML | the community's tools; our `mtarc`, `ddsave` already do the same in Rust |
| [mcondarelli/DDDAedit](https://github.com/mcondarelli/DDDAedit) | Python save editor (incomplete); credits Dugarr and FluffyQuack for the save format | cross-check for save fields |
| [sebastian-heinz/Arrowgene.DragonsDogmaOnline](https://github.com/sebastian-heinz/Arrowgene.DragonsDogmaOnline) | a server for Dragon's Dogma *Online* (a different game, MT Framework too) | protocol style only; not DDDA |
| Cielos' Cheat Engine table (v4.7, 2023; GamePressure) | the community's main CE table | structure leads for live probing |

## Findings to follow up

- **The game has its own Capcom online protocol.** The exe holds
  `sDDCaProto` and the host `dune.dragonsdogma.com` (strings 0x016130EC,
  0x016130F8); the dinput8 mod redirects `sDDCaProto::connect` (default
  port 12501). Atvaark's packet list gives its messages, including TUS
  (title user storage) user-area reads and writes. docs/PAWNS.md said the
  network is Steam only; that holds for pawn **sharing** as traced, but
  this second service is untraced: what it carries (the online Ur-Dragon's
  shared state is the obvious candidate; rift crystals, pawn ratings or
  notices are others). Brief: online-services.
- **Saves**: the community format (32-byte header, zlib, CRC-32, XML) is
  what `ddsave` already reads. No public tool documents the save *write*
  path or which game objects feed each XML node; that has to come from the
  exe (cSAVE_DATA_* reflection, the save-backup hook point).
