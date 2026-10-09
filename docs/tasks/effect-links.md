# effect-links: Visual effect lists and motion-to-effect links

- **Owner:** codex lane
- **Branch:** `task/effect-links`
- **Board area:** `formats/effects`
- **Status:** open

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

(filled in by the agent)
