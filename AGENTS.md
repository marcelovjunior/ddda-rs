# ddda-rs: session instructions

Goal: a Rust/Bevy reimplementation of Dragon's Dogma: Dark Arisen (PC) that
reads the game data from the player's own installation, with two-player
co-op designed in from the start (see `docs/COOP_DESIGN.md`).

Read `docs/MILESTONES.md` first, then only the docs and source relevant to
the task. The research method is in `docs/METHODOLOGY.md`.

## Paths on the maintainer's machine

- Game install (read-only, never modify): `E:\SteamLibrary\steamapps\common\DDDA`
  (`nativePC` holds the data).
- Private research tree (never commit, never copy into this repo): `E:\ddda-re`
  - `bin\DDDA.exe.unpacked.exe`: analysis copy of the Steam exe
    (original SHA-256 `19facc26...77b8`, unpacked `a1831a43...b5`).
  - `ghidra\DDDA.gpr`: the canonical Ghidra 12.1.4 project.
  - `notes\`: private findings, surveys and dumps.
  - `tools\start-ghidra-mcp.ps1`: starts the GhidraMCP server on 127.0.0.1:8089.

## Working rules

- Derive behaviour and values from the game data, the exe (Ghidra) or
  recordings of the original game. Do not invent or tune by eye. Label
  unresolved guesses in code and docs.
- Decompiled logic may be translated into Rust only when it is marked with
  its source, e.g. `// Translated from 0x00a1b2c0 (DDDA.exe 2017-12-13 Steam)`,
  and has a regression test. Replace Ghidra's automatic names (`FUN_`,
  `DAT_`, `param_N`) with readable names. Raw exports, Ghidra databases,
  exe bytes, game files and recordings are never committed.
- Simulation lives in headless core crates with no rendering, audio or OS
  input. Bevy is a thin presentation and input layer. This split is what
  makes co-op (host-authoritative simulation) and testing possible.
- Every player-facing system must be written for N players from day one:
  no global "the player". See `docs/COOP_DESIGN.md`.
- Ghidra MCP: use read tools freely. Renames, retypes and comments go into
  the canonical project only when deliberate and evidence-backed. Never
  call delete, import or script endpoints.
- Distinguish implemented, tested and compared-against-the-original.
- Update `docs/MILESTONES.md` after each batch: evidence, blockers, one next
  action. Detailed findings go in topic docs.
- Never redistribute game files or modify the installation.

## Parallel work

Several agents (Claude subagents, Codex) work on this repo at once, led by
the coordinating Claude session (`head`). Before doing anything:

1. Read `docs/BOARD.md` and register on the shared board
   (`python scripts/board.py join <your-name> ...`). Claim the system and
   files you will edit, report status, and read your inbox, as it says.
2. Take only the brief the head assigned you (`docs/tasks/`). Work on
   branch `task/<id>` in your own worktree under `E:\ddda-rs-wt\`. Stay
   inside the brief's file scope. Commit on that branch only: never push,
   never touch `main`. Report in the brief's Results section and to `head`.

The system breakdown is `docs/ROADMAP.md`.
