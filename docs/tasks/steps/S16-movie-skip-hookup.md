# S16: Movie skip input and the hookup after the prologue

- **Branch:** `step/s16-movie` (after S08)
- **Files:** `crates/ddview/src/movie.rs`, `intro.rs`, `opening.rs`
- **Kind:** one feature

## Context

docs/tasks/opening-movie.md: movies decode and play (`--movie st100ev00`);
the intro plays it before the editor. Open: the original's skip input (S08
finds it), subtitles outside the picture, and the hookup to q0000's event
st100 ev00 in `--opening` (after the prologue, stage 800).

## Do

Use S08's result for the skip input (keyboard and pad), and play the
movie at the point of the opening sequence where the original plays it,
driven by the event/quest data rather than a fixed order. If subtitles are
separate data, note where they are and leave them for a later step.

## Done when

An `--opening` run plays the movie at the original's point, the skip input
works as traced, ddview tests pass, and the Result lists the trigger.
