# Draw census: what a gameplay frame draws, and what could be merged, retained or cached

Task L193 (decouple2 agent), 2026-10-14. Code: `crates/ddruntime/src/census.rs`
(`DDRUNTIME_CENSUS=1`, `DDRUNTIME_CENSUS_OUT=<file>`; off by default; collects the native
sink's taken draws for 60 frames once a frame carries more than 2000 draws, then writes
a table and stops). Run: exe `C:/ddda-targets/d1/runs/d1cen`, pinned config
`E:/ddda-re/bench/config.ini` (windowed, CameraFov 15), benchmark save, fixed tick 60,
autoplay standing at the benchmark spot (the player does not walk; the camera is nearly
still). Average of 60 consecutive frames; raw table in `C:/ddda-targets/d1/runs/census1.md`.

## How the columns are defined

- **Pass** = render-target size (the sink's own grouping): main view = the largest target
  (1920x1080), shadow maps = 512x1536, soft-body sim = targets up to 64x16, everything else is
  post/other (half-res buffers, blur chains, LUTs).
- **Mesh** = hash of (vertex streams: buffer handle, offset, stride; index buffer; start, primitive
  count, base vertex, primitive type). **Material** = hash of (vertex shader, pixel shader,
  every texture resource of the draw, render-state list identity, declaration size).
  **Group** = mesh + material.
- **Instancing removes** = draws minus groups inside a pass (draws that share mesh AND material
  with another draw of the pass, contiguous or not); the non-skinned column leaves out draws
  whose vertex-constant bank has 100+ float registers (a bone palette, heuristic).
  **Contiguous merge** = draws minus runs of equal consecutive groups.
- **Static** = the draw's group (nth occurrence) had the same *per-object* vertex constants in the
  previous frame. Per-object registers are those that differ between the draws of one
  (vertex shader, viewport). This is a lower bound: the nth-occurrence identity breaks when the
  draw order or the culling set changes.

## Per pass class (per frame)

| class | draws | groups | instancing removes (all / non-skinned) | contiguous merge removes | material changes | % static (lower bound) | % skinned-like |
|---|---|---|---|---|---|---|---|
| main view 1920x1080 | 3431 | 2618 | 813 / 749 (24%) | 27 | 2043 | 13.2 | 10.1 |
| shadow maps 512x1536 | 518 | 400 | 119 / 119 (23%) | 11 | 211 | 0.4 | 5.8 |
| soft-body sim (64x8, 64x16, tiny) | 308 | 288 | 20 / 20 | 1 | 307 | 0.0 | 0.0 |
| post / other (960x540 140, 192x128 62, 128x128 17, ...) | 264 | 184 | 80 / 63 | 7 | 154 | 2.5 | 13.1 |
| **total** | **4521** | **3490** | **1032 (23%)** | 46 | | | |

Main view: 1097 distinct meshes, 910 distinct materials, 488 distinct textures, 30,967 texture
binds a frame (9 a draw). Shadow maps: 253 meshes, only 44 materials, 3,745 texture binds.
Soft-body: 308 draws over 11 targets, 4 vertex buffers: each is its own material (every draw binds
its own cloth state texture), nothing to merge.

## Findings

1. **Draws are mostly unique.** Only 24% of the main view's draws and 23% of the shadow draws share
   mesh and material with another draw. Contiguous merging (what the executor's own coalescing
   does) is worth almost nothing: 27 of 3,431. Instancing would remove at most about 1,000 of
   4,500 draws a frame, and only the non-skinned ones can be instanced without a palette path.
2. **There is much more mesh sharing than material sharing.** 3,431 main draws use 1,097 meshes
   (3.1 draws per mesh) but 910 materials, and the same mesh is drawn with many different
   materials (the top groups are one character-sized buffer at offset 12480 with 1-2 draws per
   material). So per-material state (textures, constants) is what varies; a material key that
   caches the texture set and the constant gather plan (3.8 draws per material in the main view,
   11.8 in the shadow pass) removes repeated decode work without changing the draw count.
3. **Static scene share is small, measured.** 13% of main-view draws (the big world meshes,
   96-98% static in the top groups) kept identical per-object constants since the last frame; the
   shadow pass shows 0.4%, which is certainly an artefact of the identity (shadow draws reorder
   with culling), not evidence that casters move. The static share cannot be trusted as a payoff
   estimate for a retained scene or a static shadow cache until the identity is made stable.
4. **Post/other and soft-body passes are small in count** (264 and 308 draws) but each one is
   its own material and state change; the 308 soft-body draws are about 0.9 ms of GPU
   (one 64x16 pass costs 0.7 ms, section 10.x) and are tick-bound anyway.
5. Seen vs taken: every counted draw is taken (4,521 seen = taken in the passes above); the
   remaining difference to the ~4.9k draws a frame in the stage probe is the D3D9-path draws
   (clears, copies, refused).

## Recommendation, ranked by measured payoff per effort

| rank | route | measured basis | payoff | effort |
|---|---|---|---|---|
| 1 | (1) nDraw-level material keys and a gather plan per material (the NDRAW_DIRECT S1-S3 caches) | 3.8 draws per material in the main view, 11.8 in shadows; the worker profile puts `binding_part`+`Mirror::textures`+`draw_call` glue at ~35% of worker time | up to a third of the sink (1.5-2 ms), also the cost of any replay | medium, verify mode exists |
| 2 | (5) parallel encoding of independent passes | shadow (518), soft-body (308), post (264) passes are separate target sets with only the D3D9 device state shared; the main view is 76% of the draws and cannot be split without state seeding | at most 25% of the sink moved off the critical thread | medium-high (constant-register history crosses passes) |
| 3 | (2) instancing | at most 23-24% of draws, none for skinned, same mesh+material only | fewer GPU draws (~1,000 of 4,500), small CPU gain on the decode side (decode is per record) | high (instance-data path, shader changes) |
| 4 | (6) static shadow caching | not measured reliably (identity artefact); the sun cascades move with the camera in any case; shadows are 518 draws, 0.4 ms GPU | ~0.4 ms GPU + 0.7 ms worker if valid | medium, needs a better static test first |
| 5 | (3) retained static scene / GPU-driven indirect | 13% static lower bound, 2,043 material changes in the main view | small until replay exists | very high |
| 6 | (4) bindless | 9 texture binds a draw but only 488 unique textures; the bind cost is in the decode (rank 1), not on the GPU | small | high (device features) |

Short version: the draw list is *wide, not repetitive*. The cheapest large win is making the
per-draw decode cheaper by keying on materials (rank 1, which is exactly NDRAW_DIRECT), not
reducing the draw count.

## Caveats

- One scene, standing still (benchmark spot), 60 frames. A moving camera lowers the static share.
- Material identity includes the texture handles of all 21 slots as bound, so a material that
  differs only by an unused slot counts as different (an upper bound on unique materials).
- Skinned-like is a register-count heuristic, not read from the shader.
