# skill-vfx: Effects for attacks, skills and hits

- **Owner:** codex lane
- **Branch:** `task/skill-vfx`
- **Board area:** `render/vfx`
- **Status:** open

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Play the game's effects (`mteffect`, EFFECTS.md) from the motion events and shell parameters: Ingle's fireball, Levin, Broad Cut, hit sparks and blood, goblin attacks. Follow the original's request path (which call spawns which EFL, attached to which joint).

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

**First step (head, 2026-10-09).** `mteffect` reads only the resource
links: the generator, particle and life bodies are kept opaque
(EFFECTS.md). Decode those first: emission, lifetime, size, colour and
velocity curves, billboard and strip kinds, traced from 0x01146b30,
0x01146bf0 and 0x01146e80 and their consumers. Then render the simplest
kind (textured billboards) for Ingle's shell, checked against a capture.

## Scope

- `crates/ddview/src/effects.rs`
- `crates/mteffect/**`
- `docs/EFFECTS.md`
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

The four implemented skills and basic hits show their original effects, compared against an original capture of each. Fill in Results and report to `head`.

## Results

(filled in by the agent)
