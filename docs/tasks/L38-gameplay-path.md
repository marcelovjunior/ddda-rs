# L38: the path from the title screen to the first gameplay frame (research map)

- **Owner:** claude-7
- **Branch:** `task/L38-gameplay-path` (worktree `E:\ddda-rs-wt\L38`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md). Research only; CPU-light.

## Why

L35/L35b mapped start-up up to the title screen (`docs/TITLE_PATH.md`).
The boot (claude-2, L21) will reach the title and then go further. The
next leg needs the same map, so every owner can prepare ahead:
- New Game: the prologue, character creation (`docs/CHARA_EDIT.md`) and
  the first area load;
- Continue: loading `DDDA.sav` (L26) into the first area.

## Task

1. **Trace both branches** from `aTitle`'s menu to the first frame in
   which the player controls a character:
   - `aBbsRpg` (`bbsrpg_core`, `bbs_rpg`);
   - the area/stage load (`sAreaExt`, streaming in `docs/STREAMING.md`);
   - the prologue cutscene for New Game (events, movies);
   - character creation;
   - the save load for Continue.
2. **List, per step:** imports and COM methods, files (archives, by name
   through the 0x00419100 tables), threads, and the lifted functions it
   needs. Mark each covered, missing (with its owner) or unknown, as in
   L35.
3. **Lifter cross-check:** list every function on the path that is not
   verified, split into "lifts with today's lifter" and "rejected", with
   the reason. That gives claude-3 and claude-6 their next queues.
4. **Write `docs/GAMEPLAY_PATH.md`** and send each owner their part. Also
   locate L35b's open item, the `sa\DX9` shader-archive loader.

## Results

(open)
