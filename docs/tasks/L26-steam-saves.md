# L26: Steam services and save data

- **Owner:** claude-10
- **Branch:** `task/L26-steam-saves` (worktree `E:\ddda-rs-wt\L26`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The game is the Steam build. `crates/ddruntime/src/steam.rs` answers the
Steam checks the boot reaches so far. A playable game needs more:

- the full set of Steam calls the game makes (user, stats and
  achievements, remote storage, friends, utils, apps, and the online pawn
  features where DDDA uses them);
- its save data and configuration, read and written the way the original
  does, but only inside the runtime's sandbox overlay.

The player's real saves and install are read-only.

## Task

1. **Census.** List every `steam_api.dll` import and every Steam interface
   method the game calls, with call sites from Ghidra: flat API and
   interface vtables. Then list how saves and config are stored: Steam
   Remote Storage or plain files, which paths, file names and sizes
   (`docs/SAVE_PAWNS.md` and the `ddsave` crate describe the format). Put
   it in Results.
2. **Implement** each call the game makes with offline Steam semantics:
   - documented return values from the Steamworks SDK headers and docs;
   - one local user with a stable SteamID;
   - stats and achievements stored in the sandbox;
   - callbacks and call results delivered through `SteamAPI_RunCallbacks`
     as Steam does;
   - online features answer "offline" as Steam does without a connection.
   Test each.
3. **Saves.**
   - Writes go to the sandbox overlay (see `files.rs`, including the "home"
     root L17A added). A new game creates its save there.
   - An option imports the player's existing save read-only, as a copy
     into the sandbox, so the rebuilt game can continue it.
   - Verify that a save the rebuilt game writes has the same format: the
     `ddsave` crate reads it.
4. Update the `docs/RUNTIME.md` rows.

## Results

(open)
