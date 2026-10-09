# effect-links: Visual effect lists and motion-to-effect links

- **Owner:** codex lane
- **Branch:** `task/effect-links`
- **Board area:** `formats/effects`
- **Status:** review

## Goal

Decode the provider motion-to-effect links and the EFL resource references first.

## Evidence

Start from `docs/SURVEY.md`, section "2. Visual effects": the resource classes,
counts, sample structures, exe loader addresses and reference coverage
are there. The tools are listed in `docs/tasks/README.md`.

## Scope

- `crates/mteffect/**`
- `docs/EFFECTS.md`
- `crates/ddtool/src/<name>.rs`, if you add a CLI module (shared hookups as in `docs/CODEX.md`)
- this brief's Results

## Done when

All provider and list instances pass bounds checks, and one attack's chain is resolved from motion to effect resources. Particle simulation is out of scope. Distinguish traced from guessed, and fill in Results. Then report
to `head`.

## Results

Implemented `mteffect`: EPV v0 two-level elements, motion/event links; EFL
0x20110318 header/unit offsets and executable-dispatched resource references.
Counts, paths, offsets, local targets and EOF are bounded. Particle bodies are
opaque and no rendering/simulation is implemented. Detailed evidence and limits
are in [EFFECTS.md](../EFFECTS.md).

Verified all **7,576 provider copies / 656 paths** and **58,307 list copies /
4,603 paths**. All **144,146 motion + 78 event links** target existing elements.
Provider references: **208,497 occurrences / 3,354 unique class/path targets**.
List references: **1,451,397 occurrences / 403 targets**, across **828,769 units**.
All resource targets exist, including the source-backed render-target texture
fallback. Six synthetic tests and one corpus/attack integration test pass.

Resolved named attack `cPlActAssassinAttack` (**0x00b28d00**) -> motion **0x723**
-> bank-7 custom `m0005_at_19`, slot 35 (**91 frames**) ->
`effect/epv/pl/p_pl_dag`, index 19 / element 1 ->
`effect/efl/sk/dag/l_c_dag020_02` (**28 units / 51 references**) -> textures,
animations and model. Provider loader **0x00cb6cf0**; EFL loaders/traversal
**0x01147790**, **0x01147400**, **0x01146b30**, **0x01146bf0**, **0x01146e80**.

Unresolved: runtime provider flags/window evaluation, opaque particle/joint/life
bodies, attachment and LOD, archive override precedence, exact localized skill
name. No running-original visual comparison. Next action: trace the provider
window evaluator and one particle kind before rendering. No raw game data or
private exports are committed; no writes to the game/process were made.
