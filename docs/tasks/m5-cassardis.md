# m5-cassardis: the opening in Cassardis, playable

- **Owner:** claude-7
- **Branch:** `task/m5-cassardis`
- **Board area:** `story/play`
- **Status:** open

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

(filled in by the agent)
