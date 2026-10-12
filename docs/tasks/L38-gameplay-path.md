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

claude-7. The map is [GAMEPLAY_PATH.md](../GAMEPLAY_PATH.md). It is
static: Ghidra, the call graph, scans of the lifted code, and the
lifter's `lift()` run read-only.

- **From the title to the areas.**
  - The title's state functions (`+0xF0`) lead to four area requests:
    `aGameMain` (0x005331E7), `aStage801` (character creation,
    0x00533277), and `aStage802` / `aStage804` (0x00533302 / 0x0053330B).
  - `aGameMain` runs "new game or load" (0x004FB7B0) once. That function
    creates `aStage%03d` (0x004FBD50): stage 380 for New Game.
  - The opening's route (380 → 800 → 801 → 200 → 100) is the documented
    one.
  - Not traced: which menu item reaches which call site, and how
    `aBbsRpg` is created (*inferred*: as `aGameMain`'s parent).
- **The `sa\DX9` loader** (TITLE_PATH's open item).
  - Every area's slot 5 waits in 0x00533750 → 0x00DA63F0 for
    `sa\DX9\<area name>.arc` (ready at once if the file is missing).
  - `sShader2`'s constructor loads `sa\DX9\root` and `sc\DX9\root`
    synchronously through 0x00E0E470, which is not lifted.
- **Services.** The gameplay closure (21,580 functions, 156 imports)
  needs no import beyond the title path's.
- **The lifter census of all 586 non-verified functions.**
  - 410 lift now: 99 on the title path, 108 more on the gameplay path,
    203 elsewhere.
  - 176 are rejected: x87 returns 84, SSE approximations 31, jump tables
    9, MXCSR 9, `rcr` 6, other x87 13, platform-dependent 4, other 20.
  - On the gameplay path: `cPlActJumpBegin::vf06` (the jump start),
    `uSimSoftBody::vf09`, `uWeaponEv::vf96`, `uEffect2D::vf09`, the arrow
    and lightning shells, `rsqrtps` in two object units, `rdtsc`,
    `stmxcsr`.
  - Sent to claude-3 (lift-now queue) and claude-6 (rejections).
- Also on this branch: TITLE_PATH 5.4 gets claude-10's save details.
