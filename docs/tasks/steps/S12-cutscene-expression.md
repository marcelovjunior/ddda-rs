# S12: Fix Quina's distorted facial expression

- **Branch:** `step/s12-expression` (after S04)
- **Files:** most likely `crates/ddview/src/character.rs` (animation,
  morph), `crates/mtlmt` (motion decoding), `crates/ddlook` (face morph)
- **Kind:** diagnose and fix one visible defect

## The defect

At ev10 frame 1010 Quina's face looks distorted (maintainer screenshot):
the expression doesn't look like the original's. S04 lists the face
joints whose animated pose moves far from rest.

## Leads

- Face joints may get the event's facial motion and the face-edit morph
  (ddlook::morph) in the wrong order, or twice.
- Face tracks may be in a separate motion bank or layer in the event
  (lip-sync / expression layer) that should be blended, not replaced.
- Unkeyed channels keep the rest value (character.rs comment says
  *assumed*); for face joints this may be wrong.
- docs/ANIMATION.md and docs/EVENTS.md describe what is known.

## Do

Find which joints are wrong and why, fix it in the shared path, and add a
regression.

## Done when

A frame-1010 capture shows a natural expression (`E:\ddda-re\notes\steps\s12\`),
tests pass, and the Result names the cause with its source. If the cause
needs a live comparison with the original's joints, describe what to read
and ask head.

## Result

**Ready for review.** Source-backed eye/ear constraints and joint dependency
ordering are implemented and tested. The frame-1010 capture shows aligned
eyes and the authored expression without the previous unsupported pose.

The shared character path omitted every active special face constraint:
`character.rs` filtered out `ddlook::face::special_kind` records. These
are implemented in `ddlook::face_constraint` and applied after motion and
body edits, before descendant joints are posed. No face value was tuned.

Sources (DDDA.exe 2017-12-13 Steam):

- `0x00812330` assigns type `0x10` to eyeballs 81/88; type `0xd`
  to lids 82..87/89..94, pivoted on 81/88; type `0xf` to companions
  226/227; type `0xe` to ear companions 228..233, pivoted on 221/225.
- `uCnsEdit::vf24`, `0x00860ac0`, dispatches `0x00869260` (d),
  `0x0086a100` (e), `0x0086aff0` (f), and `0x0086b2e0` (10).
  The headless helpers compose their correction directly into the world
  matrix, retaining the original missing-pivot, small-scale and singular
  inverse fallbacks.
- `0x00812820` clears active byte +0x61 before updating the edit;
  setters `0x0081e590`, `0x0081a7c0`, `0x0081a700` activate nondefault
  records. The viewer retains this gating: absent identity face records
  remain inactive.
- `0x01054540` / `0x01054610` rank parent and constraint dependencies;
  `0x01054690` sorts joint indices, used by `0x01054700`. The dependency
  comes from `uCnsEdit::vf21/vf22`, `0x0086d5a0` / `0x0086d5e0`, which
  expose pivot +0xac. Some lids precede their eye pivots in MOD order.
  The viewer now caches this original evaluation order per skeleton,
  so those lids see the current corrected pivot rather than a missing
  or previous-frame matrix.

Installed data and S04 evidence separate the cause from its leads. All
seven populated Quina event motion slots contain 273 tracks, with
bone-type 0 and weight 1; the main face channels are present. There are
no tracks for auxiliary IDs 237..252. Their S04 rotations match the
NPC's face edit exactly: -62.594772 or +63.178448 degrees. Those authored
rotations are retained. No alternate expression layer, codec change or
unkeyed-channel reset was needed for this fix.

Quina has **22 active special records**, all with zero constraint
rotation. Applying the original executable to the S04 pose changes eye
and lid positions by roughly **0.573..0.778 cm**, and ear companions by
**0.116..0.784 cm**. Reconstructing the resulting locals gives a largest
normalised cross-axis dot product of **1.02e-7** across those records:
there is no meaningful added shear for this pose. The viewer's general
TRS decomposition can still discard shear in other poses, as before.

Implementation commits: `1608200` (headless translation), `841e4fc`
(full original-dispatch regression), `26844c6` (viewer hookup), `845265f`
(pivot and repeated-frame viewer regression), `f2a04c5` (original
singular inverse fallback), `147e7bd` (API notes).
Final source commit `f7fdb37` adds the original dependency order and
regresses a lid placed before its pivot in MOD order, including repeated
motion-restore/morph cycles.

Prerequisite integration is distinct: merge `1d47527` carries reviewed
S10 branch `734cafb`, including bounds fix `ec01142`, into this task
branch. Head's then-current `main` had merged S11's documentation but
did not yet contain that source fix. Codex-1 explicitly authorized this
task-branch integration; main was not changed by this agent.

Validation:

- `cargo test --release -p ddlook --target-dir
  E:/ddda-rs-wt/item-use-review-target`: **36 passed** (27 unit, two
  appearance, seven install tests), zero failed. Original code was
  executed privately in an x86 emulator with synthetic rotated,
  nonuniform matrices and zero-scale matrices; only derived synthetic
  expectations are committed.
- Final S10-integrated viewer: **58 passed, two existing ignored**,
  zero failed, 8.50 s; release test build 19.11 s. The new ECS regression
  checks the original lid target with a later MOD pivot and three
  repeated motion-restore/morph cycles. A stale shared-target ddlook
  artifact was removed with a crate-only release clean before this run.
- `cargo build --release -p ddview --target-dir
  E:/ddda-rs-wt/m5-cassardis/target`: **passed**, 31.86 s. Existing
  `opening_world` visibility and unused `forget_actor` warnings only.
  Executable copied to the private S12 directory before releasing the
  shared build target.
- Hidden unattended event-22 capture at timeline frame 1010 / motion
  frame 100: **exit 0**, 28.43 s, with a 60 s watchdog. No material,
  group-colour, culling or depth override was enabled. Joint logging
  recorded 224 joints for each of three actors.
- All **22 Quina special joint world matrices** in the capture agree
  with the original executable's constraint prediction: maximum position
  error **0.00002953 cm**, maximum absolute basis-component error
  **2.54e-7**. This checks the actual installed edit and held motion pose,
  in addition to the synthetic regression fixtures.
- `git diff --check`: passed.

Private research is under `E:/ddda-re/notes/steps/s12/`: `emulate.py`,
`full-emulate.py`, `degenerate-emulate.py`, `actual-pose.py`, their JSON
outputs, and `look.txt`. No raw resources or executable exports are
committed.

Final image `frame1010.png`, joint dump `joints.txt`, full `stdout.log` /
`stderr.log`, `compare-candidate.py`, and `candidate-errors.json` are in
that same private directory. Image SHA-256:
`dfc1ab58dc42d704cd38e57399647e3b4114ea3f1b16f33c042fa89621b18a65`.
Both eyes receive their authored edits; the inherited S10 fix retains
the elder's complete ear and hair. GPU/build leases were released after
their processes completed.

Still open: no original live-game joint capture or matched original
frame-1010 screenshot comparison. The remaining general shear loss and
body/face edit order assumptions are unchanged. Suggested next step:
head reviews the private capture, then compares original actor matrices
at another eye/blink pose if a further facial mismatch is observed.
