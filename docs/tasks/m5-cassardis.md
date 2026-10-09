# m5-cassardis: the opening in Cassardis, playable

- **Owner:** codex-1 (implementation: codex-18; takeover of claude-7 WIP)
- **Branch:** `task/m5-cassardis`
- **Board area:** `story/play`
- **Status:** review

## Goal

The first playable M5 slice: `ddview <nativePC> --play --opening`
starts at q0001 "Newly Arisen", as after the prologue.
1. The st200 ev10 cutscene plays with the event player.
2. Control returns at the party placement.
3. The player walks around the stage with the NPCs on their schedules
   (`ddnpcrt`).
4. Talking to Quina or the elder picks the right conversation
   (`Host::talk`) and shows its lines as subtitles.
5. Walking out triggers what the original's scripts and trigger volumes
   do next: the q0001 → q0002 boot, with the SceNo ladder as in
   QUESTS.md.

Use `ddhost`: story, NPCs and the interaction lock. It must work in co-op
too: the guest sees the cutscene and the NPCs.

The vocation menu at the weapon table (stage mode 0x19) can be a
labelled stub: pick Fighter or Mage with a key.

## Scope

- `crates/ddview/src/play.rs`, `event.rs`, `net.rs`; `main.rs` flags
- `crates/ddhost/**`, `crates/ddstory/**`: additive; coordinate
- `docs/QUESTS.md`
- README's "How to play"

## Done when

- A scripted `--demo` run goes cutscene → control → talk to Quina → walk
  out → q0002 boot, with the flag and SceNo checks.
- Screenshots of each step are described.
- Fill in Results and report.

## Results

Continued the preserved WIP `052a20a` in its original task worktree; merged
main and viewer-controls. Implementation commits so far: `09c63d9`,
`41c9fe4` (task branch only, no push).

- The opening host retains event `PlaceParty` through fast-forward: ev05
  selects installed start row 2, rather than stale area-entry row 0.
  A regression verifies that request; ev10 still selects row 12.
- Solo and LAN share the same `Host<InstallData, OpeningWorld>`. Protocol
  3 mirrors NPC positions and typed event starts; joining during an event
  receives its original start frame. The host gates inputs until its own
  playback ends. Loopback verifies the NPC mirror, event, late join and
  movement gate.
- Guest cutscene captures at event frames 600-1500 confirm actors,
  subtitles and the stage render after loading. The initial guest capture
  was premature; later frames were inspected. The elder's clipping/skin
  defects and camera accuracy are being handled under cutscene-visuals.
- The demo no longer teleports when blocked. A demo-only collision-sweep
  A* planner uses the same body capsule/ground, with synthetic wall and
  installed Cassardis door regressions. This is a test-driver choice,
  not a claim to have translated the original navigation algorithm.
- Unattended policy requested in board #1422: `--unattended`, `--demo`,
  `--shot` and `--shots` never grab the cursor or pause on focus loss;
  mouse look needs the right button. Ordinary play retains direct grabbed
  mouse look and focus pause. Esc remains usable; ECS tests verify both.
- Build/check passed in release. Initial ddhost/ddnet suite: 36 tests;
  after main integration the focused Cassardis/loopback tests and all 15
  viewer tests pass, including complete projection restoration after
  multiple event frames and installed village/door navigation regressions.
- A bounded two-process LAN run (`--unattended`, host `--demo --shots`,
  guest `--shots`) completed with host exit 0 on 2026-10-09. It walked
  through ev10, control, Quina dialogue, the village and ev15 without
  a demo teleport. The village detour followed 197 collision-tested
  waypoints. The only placements were installed event `PlaceParty` requests.
  Endpoint: SceNo 1400, GSF 372/381 true, q0001 words
  `[0x18000000, 0, 3]`, q0002 booted. Guest cleanup was harness termination
  after host success. Both viewers restored gameplay projection
  FOV 1.2217305 radians, near 0.96, far 20000 after events.

Private screenshot review (`%TEMP%/ddda-m5-lan4`, never committed):

| Capture | Observed |
|---|---|
| 1-newly-arisen | Bedroom, host and undressed remote body, stage NPC |
| 2-vocation-menu | Weapon table and labelled Fighter/Mage stub |
| 3-cutscene-ev10 / guest row22 frame1200 | Event actors and subtitle; camera checkpoint restores elder face, ear hole remains |
| 4-control | Host at doorway with gameplay camera and SceNo1200 |
| 5-talk-quina | Quina beside doorway with NPC2 dialogue subtitle |
| 6-cutscene-ev15 | Frame300 roof/sky shot; framing not compared with original |
| 7-q0002 | Village gameplay view and endpoint flags/banner |
| guest-control | Guest player and NPC visible after ev10; logs contain two player bodies and 31 NPC snapshots |

Checkpoint `432ad60` includes projection restoration and capture fixes;
`c565960` merges current main and its isolated unattended-controls fix,
retaining the opening-specific `--shots` guard. Next action: head review,
then guest dialogue/menu and remaining cutscene visuals as separate work.

Limits: the prologue remains a ScriptedHost fast-forward; vocation is a
labelled stub; guest dialogue/menu controls and remote appearance dressing
remain incomplete. NPC layout presence is over-inclusive and unsupported
schedule variables park NPCs. NPC schedules continue during events; the
global cutscene freeze policy is deferred to head (board #1478/#1482).
Viewer HUD labels overlap at the top; remote appearance remains undressed.
This slice stops at q0002 boot; the next QCT
judgment type 18 is not implemented. No original-game control or writes
were used; no new comparison to an original save or original video was
performed. Screenshots/logs live only in OS TEMP, never in Git.
