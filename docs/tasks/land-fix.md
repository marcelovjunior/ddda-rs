# land-fix: a body sinking under higher ground

- **Owner:** claude-1
- **Branch:** `task/land-fix`
- **Board area:** `player/movement`
- **Status:** open

## Goal

claude-10 (#1149, #1152) saw a guest Arisen in a two-window run sink
below the ground and fall forever (y to -181,000). The host's own sim did
it from plain demo commands. The ground exists along the path (about
41,000 high, nothing below it), and the body ended at 38,393, under it.

Likely a landing or step case where a jump or step meets higher ground.
Reproduce it headlessly from the coordinates in #1149, fix it with the
traced sweep rules, and add a regression test. Also: never let a body
fall more than a set distance below any ground without being found again
(and say what the original does: does it respawn or warp?).

## Scope

- `crates/ddsim/**`, `crates/ddworld/**`
- `docs/MOVEMENT.md`

## Done when

- A regression test reproduces it and passes.
- The rule is documented.
- Fill in Results and report.

## Results

(filled in by the agent)
