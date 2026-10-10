# nonfield-play: `--play` on single-cell stages

- **Owner:** codex lane
- **Branch:** `task/nonfield-play`
- **Board area:** `play/nonfield`
- **Status:** open (small)

## Goal

codex-33's camera smoke: `ddview --play --stage 200 --spawn 2895,-78,-2368`
applies the field's origin and collision to Cassardis (a probe Z near
77498 and the Arisen falls), while `--opening` loads stage 200 correctly.
Make direct `--play` on non-field (single-cell) stages use the same stage
setup as the opening: stage coordinates, the stage's own collision, the
whole-stage models and layouts (`event::spawn_stage_models`, doors), the
stage's sky and lighting. One stage-setup path for both entry points.

## Scope

`crates/ddview/src/{main,play,event}.rs` stage setup, this brief's
Results.

## Done when

`--play --stage 200 --spawn ...` puts the Arisen on Cassardis ground with
the same models, doors, collision and camera as `--opening`, covered by a
test. Fill in Results and report to `head`.

## Results

(filled in by the agent)
