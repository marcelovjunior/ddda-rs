# Rules for every agent (head-run parallel work)

Up to ten agents (Codex and several headless Claude Code sessions) share
one machine (32 GB RAM, 16 threads, E: and C: SSDs) under the head, a
Claude Code session that writes the briefs, reviews and merges branches.
These rules hold for every brief in `docs/tasks/` that points here.

## Quality

- Read `AGENTS.md` first. The project's bar applies: 1:1 with the original
  exe, every claim backed by evidence (Ghidra, the exe's bytes, Microsoft's
  documentation, or measurements on the host's real Windows/D3D9/XAudio2),
  no guesses presented as facts, no special cases by address without a
  general rule, and lifted game code is never replaced by hand-written
  behaviour.
- Never weaken a check, a recorder guard or a test to make something pass.
- Record what you could not do and why in your brief's Results.

## Machine

- **Builds.** Every cargo command sets `CARGO_INCREMENTAL=0` and
  `CARGO_TARGET_DIR=C:/ddda-targets/<your task>`, because E: is short on
  disk. Light crates (`d3d9*`, `ddgpu`, `ddaudio`, `ddexe` and the like)
  use `CARGO_BUILD_JOBS=4`.
- **Heavy jobs.** Anything that builds `ddnative`/`ddlift`, which includes
  `ddruntime` and `ddplay`, plus vector recording, gen and the native gate,
  runs inside a machine-wide slot (two slots exist):
  `E:/ddda-re/tools/pyenv/Scripts/python.exe E:/ddda-re/tools/heavy.py <your-name> -- <command...>`,
  (the slot caps `CARGO_BUILD_JOBS` at 3). The slot waits, runs the command, releases
  itself and passes the exit code through. `heavy.py status` shows the
  holders. Batch your edits so heavy builds are rare.
- **Code the lifted crates call.** Never `#[inline(always)]` in
  `ddnative-base` (or a readable crate) on anything the generated code
  calls: debug builds (opt-level 0) copy it into each of millions of call
  sites. One such accessor made the shard crates four times larger and the
  runtime examples too large to link (over 2 GB). Use `#[inline]`; release
  builds inline it anyway.
- **Runtime work without the game (brief L36).** `ddruntime`'s `game`
  feature (on by default) is what pulls `ddnative`. Runtime work outside
  the game entry (services, threads, files, user32, D3D9, input, audio,
  network, Steam, movies) builds and tests with
  `cargo test -p ddruntime --no-default-features`. That is a light build:
  no heavy slot, `CARGO_BUILD_JOBS=4`. Without the feature a session still
  loads the exe and binds every service; a call into game code stops as
  untranslated. Tests and examples that run lifted code carry
  `#[cfg(feature = "game")]` or `required-features = ["game"]`, and run
  in the slot as before.
- **Batched verification (the merge train), since 2026-10-11 23:00.** Do
  not run the full `ddnative` build or required-vector replay for each
  step. Builds are the bottleneck: each worktree recompiles every ddlift
  shard. Instead:
  - On your branch, check only what is cheap: light crates' tests,
    `cargo check` where it is enough, Python unit tests, and recording on
    your own data set (lifter agents).
  - Commit, and tell the head when a coherent batch is ready: the commits,
    the private data to adopt, and what is unverified.
  - The head merges several ready branches (about five) into one
    integration build and runs one gen + `native_gate.sh` + runtime tests
    for all of them. Green: they land on main together. Red: the head
    bisects by branch and sends the failing one back with the log.
  - main stays verified. Branches may be ahead of verification, and that
    is expected.
- **Memory guard.** Below 1.5 GB of free RAM, a guard kills the largest
  cargo/rustc/python process. A build that dies without an error message
  died for that reason: rerun it in the slot.
- **Long jobs.** Never sit inside one tool call for more than a few
  minutes; that keeps your prompt cache warm. Run long jobs in the
  background with output to a file and poll the file.
- **Python.** Research Python with capstone and unicorn is
  `E:/ddda-re/tools/pyenv/Scripts/python.exe`.
- **Ghidra.** Ghidra is read-only over HTTP:
  `curl "http://127.0.0.1:8089/decompile_function?address=0x..."`, plus
  `/disassemble_function?address=` and `/get_xrefs_to?address=`. It is
  shared, so keep requests sequential.
- **Game files.** The game install
  (`E:\SteamLibrary\steamapps\common\DDDA`) is read-only, and so are the
  player's saves. Runtime writes go to the sandbox overlay only.

## Git

- Work only in your worktree and on your branch. Commit each verified step
  as `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  with **no AI co-author trailer**.
- Never push.
- Never commit game data, anything from `E:\ddda-re`, or decompiler
  output.

## Working with others

- Do not spawn subagents.
- The head is the Claude session named `claude-a8` in `ListAgents`.
  - When blocked on a decision only the head can make, or when you need a
    file another agent owns, send it a short message (SendMessage) and
    carry on with another part of your brief.
  - Do not end your run to wait.
- Other agents appear in `ListAgents` too. Coordinate directly when your
  work touches theirs.
- **Finishing.** Fill in your brief's Results section and update the
  `docs/RUNTIME.md`/`docs/INVENTORY.md` rows you changed. Then end your run
  with a short summary for the head: commits, what was verified and how,
  and what is left.

## Game runs and builds (2026-10-12)

- Only the head runs the game (ddplay, winmain, any exe that opens the game
  window). Agents that need FPS or behaviour numbers post a request on the
  board (the exe path, what to compare against, which phases); the head
  runs it with every build paused (E:/ddda-re/tools/pause.py) and posts the
  numbers back.
- Every build that compiles the ddlift shards goes through heavy.py with
  HEAVY_BIG=1. heavy.py runs one such build at a time, and none while a
  run is paused for.
