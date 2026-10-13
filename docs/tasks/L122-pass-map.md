# L122: gameplay pass ownership map

Owner: codex-10. Branch: `task/L122-pass-map`, based on `task/fps-line`.
Assigned by head/user on 2026-10-13. Scope: `docs/FRAME_PASSES.md`, this
brief; milestone summary handed to head to avoid concurrent edits.

Map cap1 gameplay passes to engine systems, shader hashes, targets and
earlier resource producers. Identify safe incremental native candidates.
Do not duplicate L113's iterative simulation implementation/research.
No game runs, GPU work or shard builds are required for this document.

## Results

In progress: frame7539 decoded independently from the L111 analytical slice;
5,295 draw command offsets agree with L107's pass memberships, 951 runs.
402 unique shader blobs parsed through END; 334 byte-identical root-package
matches. CTAB separates sky-scattering, God Rays, color correction and DOF
from the earlier tentative tone-map/bloom labels. Producer mapping is static
plus shader evidence; cap1 has no matching producer telemetry.

Private analysis scripts/results: `C:/ddda-targets/L122/`. No shader bytes,
capture payloads, package bytes or decompiler output belong in this commit.

Requested head-run correlation on board10916; existing instr1 is another
run, so its pointers/frame ordinals cannot be joined to cap1.

Milestone paragraph for head: L122 frame7539 pass map: 951 runs/5,295 draws,
864 tiny simulation passes,402 shader identities; sky/GodRays/DOF/LUT
classification corrects size-only guesses. No native/game/image/FPS claim.
Next: same-run producer-tagged capture to close dynamic ownership joins.
