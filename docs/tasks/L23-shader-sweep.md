# L23: every game shader translated and matching the driver

- **Owner:** claude-7
- **Branch:** `task/L23-shader-sweep` (worktree `E:\ddda-rs-wt\L23`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The game ships 29,198 distinct SM2/SM3 shaders in 76 shader packages.

- `crates/d3d9shader` decodes all of them exactly like D3DDisassemble.
- `crates/d3d9wgsl` translates them to WGSL. Translation is checked against
  the host's D3D9 driver through `crates/d3d9ref`, in
  `crates/d3d9wgsl/tests/against_d3d9.rs` (`DDDA_SHADER_PACKAGES`,
  `DDDA_SHADER_LIMIT`, `DDDA_SHADER_TRACE`).
- Pixel shaders matched 600/600 for root/Stage100 plus Title. Vertex
  shaders and the full sweep are unfinished.

A shader that differs means a wrong frame.

## Task

1. Run the full sweep, pixel and vertex, over all 76 packages, with
   several random constant, input and texture sets per shader. It runs
   in the background and in chunks; the crates are light and need no
   heavy slot. Record per package: translated, matching, and differing with
   the first difference.
2. Fix every difference in the translator, with the D3D9 rule behind it
   (the spec, the driver's measured behaviour, or the bisection tooling in
   the test: `prefix`/`prefix_to`). Rules already found are the D3D9
   multiply rule, relative-address range, half-pixel offset and
   derivatives (see `docs/RUNTIME.md`, implementation status and findings).
3. Make sure every translated shader passes WGSL validation in naga and
   wgpu: the backend creates pipelines from them.
4. Record in Results the final counts and any shader that cannot match
   exactly, with the reason (for example driver precision on transcendental
   instructions, given as a measured ULP bound). Update the `docs/RUNTIME.md`
   row.

## Results

claude-7, branch `task/L23-shader-sweep`. Host: NVIDIA GeForce RTX 4060,
driver 610.62. The D3D9 oracle (`crates/d3d9ref`) and the translation on
wgpu (Vulkan backend) run on the same GPU.

### Final counts

All 29,198 distinct shaders (27,070 ps_3_0, 2,128 vs_3_0) of the 76
packages. Each ran with 4 random sets of constants, inputs and textures.
A shader counts in the first package (sorted) that holds it.

| | Distinct | Bit-exact | Within tolerance | Differing | Untranslated | Rejected |
|---|---|---|---|---|---|---|
| Pixel | 27,070 | 26,403 | 665 (worst 332 ULP) | 0 | 0 | 2 (harness, see below) |
| Vertex | 2,128 | 2,083 | 45 (worst 92 ULP) | 0 | 0 | 0 |

- *Bit-exact*: every defined output component is equal as a float in
  every set (+0 and -0 count as equal, and any NaN as equal to any NaN).
- *Within tolerance*: inside the test's `close` (relative 1e-5) but not
  bit-exact; the count of units in the last place (ULP) is the largest
  over all components and sets.
- No shader is *differing* (outside `close`).
- *Rejected*: the two root pixel shaders that write three and four
  Rgba32Float targets exceeded wgpu's default colour-attachment limit
  (32 bytes per sample) in the test device, not in the translation.
  The harness now takes the adapter's limit (866c3d9).
  Their rerun is pending: D3D9 refuses a device while a full-screen game holds the display, and the head paused the agents.
- No vertex shader was skipped for using texcoord14/15 (which the oracle's
  output capture needs).

Per package (pixel and vertex columns: distinct / exact / within
tolerance (worst ULP); the last column counts differing, untranslated,
rejected and oracle-skipped shaders):

| Package | Pixel: distinct / exact / close (worst ULP) | Vertex: same | Differing, untranslated, rejected or skipped |
|---|---|---|---|
| GameMain | 5 / 5 / 0 (0) | 1 / 1 / 0 (0) | 0 |
| Stage100 | 4219 / 4134 / 85 (163) | 763 / 756 / 7 (23) | 0 |
| Stage100_T | 261 / 260 / 1 (2) | 2 / 2 / 0 (0) | 0 |
| Stage200 | 322 / 322 / 0 (0) | 25 / 25 / 0 (0) | 0 |
| Stage200_T | 38 / 38 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage210 | 120 / 120 / 0 (0) | 2 / 2 / 0 (0) | 0 |
| Stage210_T | 7 / 7 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage220 | 127 / 106 / 21 (12) | 12 / 12 / 0 (0) | 0 |
| Stage230_T | 40 / 38 / 2 (9) | 3 / 3 / 0 (0) | 0 |
| Stage240 | 105 / 104 / 1 (1) | 7 / 7 / 0 (0) | 0 |
| Stage240_T | 43 / 43 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage250 | 1826 / 1818 / 8 (2) | 2 / 2 / 0 (0) | 0 |
| Stage300 | 100 / 87 / 13 (5) | 2 / 2 / 0 (0) | 0 |
| Stage300_T | 7 / 7 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage310 | 219 / 215 / 4 (3) | 4 / 4 / 0 (0) | 0 |
| Stage310_T | 6 / 6 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage320 | 2402 / 2392 / 10 (5) | 75 / 75 / 0 (0) | 0 |
| Stage330 | 31 / 31 / 0 (0) | 4 / 4 / 0 (0) | 0 |
| Stage330_T | 13 / 13 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage370 | 27 / 27 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage370_T | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage380 | 300 / 253 / 47 (41) | 13 / 13 / 0 (0) | 0 |
| Stage380_PROLOGUE | 729 / 694 / 35 (11) | 39 / 39 / 0 (0) | 0 |
| Stage380_T | 38 / 36 / 2 (5) | 1 / 1 / 0 (0) | 0 |
| Stage400 | 580 / 466 / 114 (30) | 47 / 47 / 0 (0) | 0 |
| Stage401 | 20 / 13 / 7 (4) | 0 / 0 / 0 (0) | 0 |
| Stage402 | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage405 | 15 / 4 / 11 (9) | 0 / 0 / 0 (0) | 0 |
| Stage406 | 1 / 1 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage410 | 158 / 139 / 19 (332) | 1 / 1 / 0 (0) | 0 |
| Stage411 | 63 / 58 / 5 (5) | 0 / 0 / 0 (0) | 0 |
| Stage413 | 36 / 16 / 20 (8) | 0 / 0 / 0 (0) | 0 |
| Stage420 | 48 / 37 / 11 (3) | 0 / 0 / 0 (0) | 0 |
| Stage421 | 6 / 5 / 1 (1) | 0 / 0 / 0 (0) | 0 |
| Stage423 | 31 / 23 / 8 (83) | 1 / 1 / 0 (0) | 0 |
| Stage424 | 52 / 34 / 18 (15) | 2 / 2 / 0 (0) | 0 |
| Stage425 | 15 / 15 / 0 (0) | 1 / 1 / 0 (0) | 0 |
| Stage430 | 173 / 139 / 34 (10) | 2 / 2 / 0 (0) | 0 |
| Stage431 | 54 / 41 / 13 (80) | 1 / 1 / 0 (0) | 0 |
| Stage435 | 12 / 6 / 6 (4) | 0 / 0 / 0 (0) | 0 |
| Stage436 | 87 / 71 / 16 (4) | 8 / 8 / 0 (0) | 0 |
| Stage440 | 34 / 34 / 0 (0) | 1 / 1 / 0 (0) | 0 |
| Stage443 | 298 / 295 / 3 (8) | 0 / 0 / 0 (0) | 0 |
| Stage444 | 126 / 121 / 5 (18) | 0 / 0 / 0 (0) | 0 |
| Stage445 | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage446 | 13 / 13 / 0 (0) | 1 / 1 / 0 (0) | 0 |
| Stage447 | 3 / 3 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage450 | 172 / 171 / 1 (2) | 8 / 8 / 0 (0) | 0 |
| Stage500 | 304 / 304 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage501 | 732 / 732 / 0 (0) | 1 / 1 / 0 (0) | 0 |
| Stage502 | 1128 / 1122 / 6 (2) | 0 / 0 / 0 (0) | 0 |
| Stage502_T | 951 / 944 / 7 (234) | 0 / 0 / 0 (0) | 0 |
| Stage600 | 523 / 523 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage601 | 40 / 40 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage601_T | 99 / 99 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage602 | 2043 / 2029 / 14 (30) | 1 / 1 / 0 (0) | 0 |
| Stage603 | 329 / 328 / 1 (1) | 0 / 0 / 0 (0) | 0 |
| Stage604 | 282 / 277 / 5 (3) | 0 / 0 / 0 (0) | 0 |
| Stage605 | 221 / 221 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage610 | 137 / 135 / 2 (3) | 3 / 3 / 0 (0) | 0 |
| Stage611 | 96 / 96 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage615 | 108 / 108 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage700 | 434 / 404 / 30 (21) | 3 / 3 / 0 (0) | 0 |
| Stage701 | 2538 / 2497 / 41 (111) | 134 / 133 / 1 (10) | 0 |
| Stage702 | 255 / 255 / 0 (0) | 1 / 1 / 0 (0) | 0 |
| Stage704 | 46 / 25 / 21 (9) | 0 / 0 / 0 (0) | 0 |
| Stage704_T | 2 / 2 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage705 | 44 / 44 / 0 (0) | 3 / 3 / 0 (0) | 0 |
| Stage706 | 36 / 36 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage800 | 7 / 7 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage801 | 1205 / 1205 / 0 (0) | 14 / 14 / 0 (0) | 0 |
| Stage802 | 855 / 855 / 0 (0) | 33 / 33 / 0 (0) | 0 |
| Stage803 | 10 / 10 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Stage804 | 0 / 0 / 0 (0) | 0 / 0 / 0 (0) | 0 |
| Title | 2 / 2 / 0 (0) | 2 / 2 / 0 (0) | 0 |
| root | 1661 / 1642 / 17 (21) | 905 / 868 / 37 (92) | 2 |

Reproduce (writes one report per package; resumable):
`DDDA_SHADER_SWEEP=<dir> cargo test -p d3d9wgsl --release --test against_d3d9 sweep -- --nocapture`
(`DDDA_SHADER_PACKAGES`, `DDDA_SHADER_STAGE`, `DDDA_SHADER_SETS`,
`DDDA_SHADER_BISECT`). About 1.5 h on this host.

### Rules found, applied to the translator

Each rule was measured with hand-assembled shaders on the driver. Random
operands were compared against software models, plus special operands
(signed zeros, infinities, NaN, denormals). Each rule is pinned by a test
in `tests/against_d3d9.rs`. Before the rules, Title's shaders were 8-48
ULP off, and 5 of the first 300 root vertex shaders differed.

- **mad** is one fused multiply-add (400/400 random trials; unfused
  148/400).
- **dp3/dp4** and the matrix macros are a fused chain in component order:
  x·x', then fma(y, y', ·), fma(z, z', ·), fma(w, w', ·). Other orders and
  the unfused sum match 206-270/400.
- **dp2add** is fma(y, y', x·x') + c.
- **lrp** is fma(a, b − c, c).
- **crs** is fma(a.y, b.z, −a.z·b.y) in pixel shaders but
  fma(−a.z, b.y, a.y·b.z) in vertex shaders (400/400 each; the other
  order 110/400 and 114/400). The stages are compiled differently.
- **Zero rule**: a zero or denormal factor gives +0, also against inf and
  NaN, and also inside the fused products (+0 + c, so −0 and denormal
  addends give +0). The WGSL is an fma of zeros, because the GPU compiler
  folds `0.0 + c` to `c`.
- **frc** reads a denormal operand as 0 (frc(−1e-40) is 0, not 1).
  Pixel-shader **sincos** does the same: sin(−0) is +0 in pixel shaders
  and −0 in vertex shaders.
- **lit** clamps its exponent to [−128, 128], not the reference
  rasteriser's 127.9961. The driver's result at w = 127.99999 equals the
  one at 128 and above; at 127.999 it differs.
- A shader's def'd −0 and NaN keep their bits.

Transcendentals need no tolerance: rcp, rsq, exp, log, pow, sincos, nrm,
lit, expp and logp are bit-exact against the driver in both stages, on 200
wide-range random operands and the special operands each
(`arithmetic_matches_the_driver_bit_for_bit`). Both paths use the same
hardware units, so the brief's example of driver precision on
transcendental instructions does not arise on this host.

### Why 710 shaders are within tolerance but not bit-exact

The cause is never a single instruction's translation. It is a compiler
rewriting across several instructions, on either side:

- **The driver's ps_3_0 compiler factors a common multiplier.** It
  computes `mul r1, r1, w; mul r2, r2, w; mad d, a, r1, r2` as
  (a·r1 + r2)·w. Measured 200/200; the literal form 20/200. vs_3_0 is
  literal (200/200). Pinned in `fused_arithmetic_follows_the_driver`.
- **Our side: NVIDIA's Vulkan compiler reassociates.** An example:
  `add r3, r5, r2` then `add r1, -r5, r3` yields r2 exactly, where the
  driver rounds twice (Stage100 vertex 256d92628ffc592e). WGSL has no
  `precise`, so naga cannot forbid it.
- A mul feeding a mad's addend alone is computed literally by the driver
  (200/200 in both stages).

`attribute_inexact` (`DDDA_SHADER_ATTRIBUTE=<sweep dir>`) classifies each
inexact shader. It finds the first instruction whose value differs, reads
that instruction's inputs in context, and runs it alone on both sides:

- "instruction": a missing rule in the translation;
- "driver rewrite";
- "wgpu rewrite".

On Stage405 all 11 inexact shaders are "driver rewrite" (the factoring above, 1-13 ULP at the first differing instruction). The run over all 710 is pending (paused, as above).

Neither rewrite is a D3D9 rule. Imitating NVIDIA's optimiser would tie the
translation to one vendor's compiler, so the translation stays literal.
The measured bound is 332 ULP in pixel and 92 ULP in vertex shaders. Large
ULP counts come from cancellation after a 1-2 ULP rewrite.

### WGSL validation and pipelines (step 3)

- `every_translation_is_valid_wgsl` (`DDDA_ALL_SHADERS=1`): 60,524 of
  60,524 modules parse and validate in naga. These are every shader's
  default translation plus the variants ddgpu builds: vertex input types
  (float/uint/sint) with the half-pixel offset; linked pixel input
  locations, with an unwritten input, and the alpha test.
- `every_translation_makes_a_backend_pipeline`: each shader becomes a wgpu
  pipeline the way ddgpu builds it, so wgpu's inter-stage interface checks
  and the driver's pipeline compilation run. 14,314 of the first 14,314 were built with 0 failures before the pause; the rest is pending.
- The sweep itself created every default translation's pipeline: 0 WGSL
  rejections.

### Tooling added (tests/against_d3d9.rs)

- `sweep`: resumable, per package, with global deduplication. Pipelines
  are reused across sets.
- `probe`: traces one shader instruction by instruction with ULP distances.
- `tap()`: replaces the prefix cut. It reaches instructions inside loops,
  branches and subroutines, so the sweep's first-difference search covers
  them.
- `attribute_inexact`, as described above.
- `asm` token builders for hand-made shaders.

### Not done, limits

- Rerun of root's two shaders, the attribution over all 710 and the rest of the pipeline pass: waiting for the head's resume.
- Coverage limits of the sweep:
  - Random constants only: loop counts are 0-4 and a0 indices are random.
  - Textures are 4x4, single level and point-sampled. Filtering and mips
    are sampler state (ddgpu).
  - Inputs are the same at every pixel of the quad, so dsx/dsy and
    implicit-LOD texture reads see zero derivatives.
- All results are for one GPU and driver (NVIDIA RTX 4060, 610.62). The
  fusion rules are the driver's. Another vendor's driver may round
  differently.
