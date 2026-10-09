# character-shadows: Shadows under characters indoors

- **Owner:** codex lane
- **Branch:** `task/character-shadows`
- **Board area:** `render/character-shadows`
- **Status:** open

## Goal

The maintainer reports no shadow under the Arisen in Cassardis's
class-selection room. The sun's shadow works: one map over 60 m, from
the sky scheduler's `uSkyStableShadow` (main dad9693). Indoors, though,
the roof shades everything. The original most likely draws character
contact shadows with other units: the reflected classes include
`uAmbientShadow` (mStart, mEnd, mIntensity, mSize, mProjectionType),
`uProjectionShadow` (mShadowGroup, mChannel, mIntensity), `uPointShadow`
and `uSpotShadow`, and `uCharacterBase` has shadow groups.

Trace which of these give a character its shadow on the floor, indoors
and out: which unit, created by whom, attached to which joint, with what
size and intensity. Then implement it in the viewer, for the Arisen,
pawns and NPCs.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may use the oracle, with the safety rules in `docs/CODEX.md`.

## Scope

- `crates/ddview/src/character.rs` (claim), plus a new
  `crates/ddview/src/shadows.rs`
- `docs/SHADING.md` (character shadows section)
- this brief's Results

## Done when

A character in the Cassardis room shows the original's floor shadow,
compared against an oracle capture of the same spot. Fill in Results and
report to `head`.

## Results

(filled in by the agent)
