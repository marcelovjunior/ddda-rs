# Step tasks

Small, pre-digested tasks for the Codex lane (from 2026-10-09 night it runs
a cheaper model). Each step is **one change or one diagnostic**, with the
files, facts and done check written out. Head (Claude) does the analysis
between steps and writes the next ones.

## Rules for every step

- Work on the branch named in the step, in your own worktree. Claim the
  listed files on the board first (`scripts/board.py claim`).
- Change only the files listed. If the step needs anything else, stop and
  post a `question` to `head`; don't widen the scope yourself.
- Don't guess. If the code or the exe doesn't show what the step says, stop
  and report what you found instead.
- Run the tests the step names (`cargo test -p <crate>`), and for ddview
  changes `cargo build --release -p ddview`.
- Finish by appending a short **Result** section to the step file: what you
  did, the commit hash, the test output line, and anything surprising.
  Then post `ready <branch> <crates>` to `head` and stop.
- Never commit game data, screenshots or anything from E:\ddda-re. Put
  captures under `E:\ddda-re\notes\steps\<step>\` and name them in the
  Result.
- Commit as `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  with no AI co-author trailer. Never push.
- Every GUI run gets `--unattended` and a timeout (60 s unless the step
  says otherwise).

## Order

Take the lowest-numbered open step whose "after" steps are done. Steps
without an "after" can run in parallel, one agent each.

| Step | What | After |
|---|---|---|
| S01 | cutscene: log every character part (diagnostic) | - |
| S02 | cutscene: colour each mesh group (diagnostic capture) | - |
| S03 | cutscene: force the eyebrows visible (diagnostic capture) | - |
| S04 | cutscene: dump Quina's face joints at frame 1010 (diagnostic) | - |
| S05 | cyclops: translate base action enter 0x0097FD30 | - |
| S06 | cyclops: translate the placement initializer 0x00978820 | S05 |
| S07 | ambient: ground the creatures' root motion on collision | - |
| S08 | opening movie: find the original's skip input (research) | - |
| S09 | NPCs: keep a layout set per stage in NpcAdapter | - |

The cutscene command used by S01-S04:

```
target\release\ddview.exe E:\SteamLibrary\steamapps\common\DDDA\nativePC --event 22 --start-frame 1010 --hold --unattended --shot <out.png> --shot-delay 15
```
