# L122: gameplay pass ownership map

Owner: codex-10. Branch: `task/L122-pass-map`, based on `task/fps-line`.
Assigned by head/user on 2026-10-13. Scope: `docs/FRAME_PASSES.md`, this
brief; narrow L122 milestone prepend agreed with codex-11 on board10998.

Map cap1 gameplay passes to engine systems, shader hashes, targets and
earlier resource producers. Identify safe incremental native candidates.
Do not duplicate L113's iterative simulation implementation/research.
No game runs, GPU work or shard builds are required for this document.

## Results

Delivered `docs/FRAME_PASSES.md`: representative frame7539 independently
decoded from the L111 analytical slice. **5,295 draws / 951 runs**, including
**864 SoftBody runs** (624 at64x8,240 at64x16), and all eight copy commands.
The complete sequence names each operation family, targets, prior sampled
resource writers and all shaders through **50 deduplicated shader sets**.

**402 shader blobs: 225 VS /177 PS**, all parsed through END with unique
12-digit SHA-256 prefixes; **334** byte-identical root-package matches.
Recomputed the full slice's108,502-command SHA-256: exact match to L111.
CTAB and consumer bindings establish sky-scattering, albedo/detail-normal/
normal mip chains, God Rays, color correction and DOF, correcting the
earlier tentative tone-map/bloom labels. Static issuer matrix cites DTI and
Ghidra addresses; native takeover priorities retain original producers.

Checkpoint **d3e3eced** contains provenance/static evidence. Its draft stage
split218/184 was corrected to225/177 by the complete sequence batch. No
runtime code changes, cargo tests, GPU/shard/game execution or FPS claim.
Documentation checks cover ordinal/draw totals, hashes, command digest,
UTF-8 and whitespace. Producer mapping is static plus shader evidence;
cap1 has no matching producer telemetry, and no original image comparison
was performed.

Private analysis scripts/results: `C:/ddda-targets/L122/`. No shader bytes,
capture payloads, package bytes or decompiler output belong in this commit.

Requested head-run correlation on board10916; existing instr1 is another
run, so its pointers/frame ordinals cannot be joined to cap1.
L111 lacks producer caller/class tags; the document specifies the additional
observer fields needed. Exact base-texture producers864/878/887/897, shadow
subclasses, and water/particle command ownership remain unresolved rather
than guessed. Bloom/ToneMap/Bokeh are not positively identified in7539.
SoftBody hashes/counts sent to L113 (board10965); no solver work duplicated.

Full sequence batch **7ac72d99** follows checkpoint **d3e3eced**.
Milestone editing initially collided with codex-11's claim; board10998
permitted the narrow independent L122 prepend, now included. No other
milestone sections changed. Branch is `task/L122-pass-map`; no pushes.

Milestone paragraph for head: L122 frame7539 pass map: 951 runs/5,295 draws,
864 tiny simulation passes,402 shader identities; sky/GodRays/DOF/LUT
classification corrects size-only guesses. No native/game/image/FPS claim.
Next: same-run producer-tagged capture to close dynamic ownership joins.
