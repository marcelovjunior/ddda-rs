# Step tasks

Scoped tasks for the Codex lane (from 2026-10-09 night it runs a cheaper
model). Each step is one problem of moderate size, with the files, the
facts already known and a done check written out. The agent researches and
decides how; head (Claude) reviews, merges and writes the next steps from
the Results.

## Rules for every step

- Work on the branch named in the step, in your own worktree. Claim the
  files you change on the board first (`scripts/board.py claim`).
- The listed files are where the work should land. Small related edits
  elsewhere are fine when the step needs them; say why in the Result. If
  the step turns out to need a bigger change than described, stop and post
  a `question` to `head` with what you found.
- Research is welcome: Ghidra (headless on 127.0.0.1:8089), the docs, the
  installed data, live reads with the oracle (safety rules in
  docs/CODEX.md). Use your judgement on the approach; the "Do" lists are
  the suggested path, not a script.
- Trace in the exe and label any guess as *assumed* or *viewer choice*
  (AGENTS.md). Never invent behaviour to make a test pass.
- Run the tests for the crates you touch (`cargo test -p <crate>`), and for
  ddview changes `cargo build --release -p ddview`.
- Finish by appending a **Result** section to the step file: what you
  found and did, the commit hash, the test lines, what is still open, and
  your suggestion for the next step. Then post `ready <branch> <crates>`
  to `head` and pick the next step.
- Never commit game data, screenshots or anything from E:\ddda-re. Put
  captures under `E:\ddda-re\notes\steps\<step>\` and name them in the
  Result.
- Commit as `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  with no AI co-author trailer. Never push.
- Every GUI run gets `--unattended` and a timeout (60 s unless the step
  says otherwise).


## GPU checks (maintainer, 2026-10-10)

The viewer GPU run is the scarcest resource; no agent may sit idle waiting
for it, and one batch of steps should need it at most once.
- Every step is accepted on **headless** tests first (installed host/sim
  tests, ddview unit tests). Write the step so its "Done when" is mostly
  headless.
- GUI checks start from the nearest route checkpoint (`--opening-from`,
  S46), are bounded (target under 3 minutes, hard watchdog), and capture
  only what the step needs (`--shots`, one PNG, one log grep).
- Do not launch your own GUI run while another agent holds `live/ddview`.
  Instead post a **capture request** on the board (binary pin, exact command,
  what to look for) and keep working; the GPU holder or head batches pending
  requests into one run.
- The full Cassardis-to-road route is the integration test: head runs it at
  most a few times a day, not per step.

## Order

Take the lowest-numbered open step whose "after" steps are done. Steps
without an "after" can run in parallel, one agent each.

| Step | What | After |
|---|---|---|
| S01 | cutscene: log every character part (diagnostic) | - |
| S02 | cutscene: colour each mesh group (diagnostic capture) | - |
| S03 | cutscene: force the eyebrows visible (diagnostic capture) | - |
| S04 | cutscene: dump Quina's face joints at frame 1010 (diagnostic) | - |
| S10 | cutscene: fix the elder's ear hole and hair | S01, S02 |
| S11 | cutscene: fix the eyebrows | S01, S03 |
| S12 | cutscene: fix Quina's distorted expression | S04 |
| S05 | cyclops: translate base action enter 0x0097FD30 | - |
| S06 | cyclops: translate the placement initializer 0x00978820 | S05 |
| S07 | ambient: ground the creatures' root motion on collision | - |
| S08 | opening movie: find the original's skip input (research) | - |
| S09 | NPCs: keep a layout set per stage in NpcAdapter | - |
| S13 | NPCs: admit field NPCs from the loaded layout cells | S09 |
| S14 | cyclops: think driver and target selection | S06 |
| S15 | shadows: the dark patches on Cassardis walls | - |
| S16 | opening movie: skip input and the hookup after the prologue | S08 |
| S17 | cyclops: navigation provider (world sets 1002/1003) | S14 |
| S18 | cyclops: WalkTrace approach, runnable actor, test fight | S17 |
| S19 | opening movie and event subtitles | S16 |
| S20 | the naked NPC in the class-selection room | - |
| S21 | enemy HP bars above their heads | - |
| S22 | identify the roadside goblin encounter (q0002) in the data | - |
| S23 | **the opening stalls at SceNo 1400 after ev15** (tutorial blocker) | - |
| S24 | map the beach dragon fight (research; S25+ follow from it) | - |
| S18 | (ON HOLD: cyclops belongs to the Encampment, after the playtest) | |
| S25 | the bow 1: bow mode and the action's phases | - |
| S26 | the bow 2: arrows that hit | S25 |
| S27 | the bow 3: in the viewer | S26 |
| S28 | the new-game inventory in the opening (quick use) | - |
| S29 | the beach dragon as a host actor, with its entrance | S24 |
| S30 | the dragon lands (PressAttack) and breathes fire (BreathWide) | S29 |
| S31 | ev00 -> dragon fight -> ev01 -> ev05 in the opening | S30 |
| S32 | NPC authored motions (Reynard's rescue script) | S23 |
| S33 | LAN: replicate every enemy kind (the dragon for guests) | S29 |
| S34 | Reynard as a combat body (HP, goblins target him, his gauge) | S32 (viewer part) |
| S35 | the real vocation menu at the weapon table (uGUIJobChange) | - |
| S36 | pawn commands Go!/Wait!/Help!/Come! | - |
| S37 | experience from kills, level-ups in the fight, level-up notice | - |
| S38 | the item menu (pause menu: Items) | S28 |
| S39 | NPC quest talk: Reynard's thanks, reward and departure line | S32 |
| S40 | event cameras: the roadside close-up (camera 12) and the others | - |
| S41 | frame rate: profile, aim for 180 fps on the field | - |
| S42 | graphics settings like the original's options | S41 (helps) |
| S43 | the level-up notice (uGUICharacterLevelUp) | S37 core |
| S44 | LAN: quest cameras and NPC combat state for guests | S40, S34 |
| S45 | **MultiNpc placements on the road (route blocker)** | - |
| S46 | route checkpoints: `--opening-from` a saved milestone (head) | - |
| S47 | **goblins look right: equipment/armour by placement** (maintainer report) | - |

Priority now (goal: the tutorial through the road to the Encampment, the dragon attack included; the prologue is for later; the Encampment and its cyclops wait for the maintainer's playtest): S23 first, then S29-S31 (the dragon attack, from S24's map), S25-S27 (the Strider's bow), S28 (inventory). Done: S21 (head), S12, S19, S23, S25, S26, S28, S29, S33; S32 source merged (route gate open); S37 core (head). S18 (cyclops) is ON HOLD.

The cutscene command used by S01-S04:

```
target\release\ddview.exe E:\SteamLibrary\steamapps\common\DDDA\nativePC --event 22 --start-frame 1010 --hold --unattended --shot <out.png> --shot-delay 15
```
