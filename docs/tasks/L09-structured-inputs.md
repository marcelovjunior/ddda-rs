# L09: vectors for the functions random inputs cannot drive

Owner: Codex (same session), after R01. Branch `task/L09-structured-inputs`
from the head's `main`, worktree `E:\ddda-rs-wt\L09`.

## Why

36,793 of 37,728 non-platform functions lift, but only about 31,600 are
verified: roughly 3,200 have no usable vector (`patterns/lift_novector.json`
plus the per-task `unverified-*.json` lists). The typical reasons, all
rejections in `harness.run_sparse`:

- a loop whose count comes from random memory, over random pointers or a
  virtual call per element: "touches over 256 pages", "runaway", minutes
  per seed (the 20-second budget now cuts them off);
- a mocked allocator or callee returns a random pointer that is then used
  as an array base with a random length;
- execution reaches code outside the image through a random function
  pointer that is not a call (stray returns are already rejected).

Every case is still a sound test when it is *some* valid input; the inputs
only have to be plausible enough to finish.

## Task

1. Classify the no-vector functions by rejection reason (record a few
   seeds per function and keep the reasons).
2. Add input strategies to the recorder, each opt-in and recorded in the
   vector (like `zero_inputs`), for example: small integers instead of full
   32-bit random values for words that are used as counts or indices
   (decide per load, e.g. a seeded mix of small and large values); pointer
   fields that point into a fresh zeroed or small-valued object region;
   mocked returns that are small counts or valid pointers. Keep everything
   deterministic per seed. A vector is valid whatever the inputs were; the
   replay checks the translation, not the inputs.
3. Re-record the no-vector functions with the new strategies (time budget
   per function, worker limits as before), adopt what passes, and report
   per strategy how many functions it unlocked.

## Rules

As in L08 (`docs/tasks/L08-lifter-integer-sse.md`, "Rules"). Note the
recorder pitfalls fixed in main (docs/NATIVE_PIPELINE.md: the code-hook
size sentinel, the TCG buffer cap, worker recycling, the per-function time
budget).

## Results

(Codex fills this in.)
