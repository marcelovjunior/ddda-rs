# Streaming rules: stage 100

Research against the 2017-12-13 Steam executable. Addresses identify that
build. This is a **partial trace, with a passive live snapshot**, not an
implemented streaming system or a movement/boundary acceptance test.
No original-game input, calls, memory writes or injection was performed.

## Three different decisions

Archive residency, construction of world objects, and drawing those objects
are separate decisions. A square of candidate cells does not imply that all
its cells are resident. A loaded layout does not imply every enemy is active.
There is no evidence here for one universal "spawn radius".

`uStageSplitCtrl::vf08` (`0x00c5ca60`) updates archives, coarse terrain,
cell models, collision, layouts, far models, landmarks, effects and water.
The controller lives at `aStage+0x640`; the current stage is
`[sArea+0x3834]`, with `sArea` at global `0x018d099c`.

## Grid and candidate windows

`aStage100::DTI_newInstance` (`0x0051b7f0`, store at `0x0051b866`) assigns
the grid descriptor at `0x0153023c` to `aStage+0x830`. The descriptor has
X/Z stage origins at +0/+4, counts at +8/+12, cell widths at +16/+20,
and filename N/M offsets at +24/+28. Stage 100 has a 40 by 33 grid,
10000-unit square cells, stage origin (-200000, -100000), and filename
offsets (30, 40). These are executable **data**, to be read from the
installed exe, not duplicated as a runtime table in code.

`0x00507440` computes inclusive, grid-clipped intervals around the current
zero-based grid index: `max(0, index - width/2)` through
`min(count-1, index + width/2)`, using signed integer division. Filename
offsets are then added. The base stage constructor `0x004fdf60` initializes:

| Consumer | Stage fields X/Z | Default widths | Candidate cells away from edges | Main consumer |
|---|---|---|---|---|
| Terrain models | +0x7f0/+0x7f4 | 5/5 | 5 by 5 | `0x00c5f070` |
| Collision | +0x7f8/+0x7fc | 5/5 | 5 by 5 | `0x00c5f680` |
| Layouts | +0x800/+0x804 | 3/3 | 3 by 3 | `0x00c60510` |

The defaults are also observed live below. This does not prove that every
stage or runtime override retains them. Water/sea (`0x00c631a0`), effects
(`0x00c623a0`) and the far-model path (`0x00c613d0`) use the layout window.
Archive paths `0x00c5e210` and `0x00c5e740` use the collision and layout
windows respectively. `0x00c5dda0` requires the matching archive entry to
have completed its loading state before its dependent cell is constructed.

### Additional distance filter

`0x00507510` takes the party's first actor's position at +0xe0, falling
back to +0x40 when the former is the zero vector, and subtracts the world
absolute offset. It calls `0x005075d0`, also called directly by loaders.
Its hidden EDI argument selects model/collision/layout fields: the
decompiler's displayed signature omits it. A port must recover caller
registers, not copy that signature.

The predicate compares horizontal distance to the cell centre against a
base term plus an angular correction. The base term is
`quality_multiplier * stage_width_x / divisor * cell_width_x`.
The three divisors are 2.5, 3.3 and 2.4 (reads at `0x0050769e`,
`0x005076ba`, `0x005076c8`, constants `0x017f41e8`, `0x0161c428`,
`0x0161d050`). The multiplier is 3 when `[global 0x018d1d20]+0x38 == 3`,
2 when it equals 2, otherwise 1 (`0x0050771c..0x00507744`). The user-facing
meaning of that setting is not established here.

For the default widths and multiplier 1, the base terms are 20000,
approximately 15151.5, and 12500 units. **These are not the final radii.**
The angular term multiplies `(sqrt(2)-1)*cell_width_x` by the absolute
output of `0x012f0906(2 * 0x012f161f(t))`; `t` is a clamped Z component
of the normalized difference between normalized player and cell-centre
directions in the fixed stage frame. The exact library-function identities
and numerical edge behavior remain unverified. The final comparison is
strict: equality rejects (`0x0050790d..0x00507925`).

An apparent night alternative in the decompilation must not be "fixed":
the assembly at `0x005076f8..0x00507702` requires unsigned hour >=21 and
<=3 together, so the alternate coefficient array is unreachable on this
path. No claim is made about other night-specific systems.

## Unload delays and hysteresis

The inspected model and collision paths use the same spatial interval and
distance predicate for entry and retention; no second larger unload radius
was found in those paths. They do have asynchronous lifetime state.

* `0x00c5f680`: collision statuses 1/2/4/5 run transition helpers. Status 3
  is eligible for deletion when outside the candidate window or the
  additional predicate fails. Resource readiness can delay reaching 3.
* `0x00c5f070`: a terrain model leaving eligibility gets +0x38d set. The
  wrapper remains until model +0x38e reports completion.
* `uStageSplitMdl::vf08` (`0x00c63d30`) state 3 subtracts 0.1 from its
  alpha-like field +0x158 per update, clamps below zero, then marks +0x38e.
  State 1 adds 0.1 while fading in. This is a per-update value, not a
  measured seconds duration. A flagged state-10 path completes without
  that fade. No reversal-on-reentry was established.
* Layouts in `0x00c60510` transition 3 -> 7, call `0x0075f190`, then
  subtract `sMain.mDeltaTime * 0.2` from +0x58 until <=0, entering state 8.
  Removal additionally checks the cell-change argument and `0x00c62330`.
  The initial timer value and complete state-8 dependency are not resolved,
  so this is not evidence for a fixed five-second unload delay.

## Far terrain and landmarks

`0x00c5d7a0` manages six regional coarse-model entries, choosing between
two variants using a byte table at `0x014e8d00` indexed by grid M*40+N.
It waits for the replacement's readiness before retiring its counterpart.
`0x00c613d0` also uses region-selection data at `0x014e8740` and
`0x014e8c68`. These data-dependent choices are not concentric fixed rings.
The tables belong in an installed-exe reader if implemented.

`0x00c61bd0` iterates 15 landmark records of stride 0x40 at `0x01873550`.
Record +8 is a distance; +0x10/+0x14/+0x18 is its position; later words
provide scenario conditions. It converts the record position into the
floating frame and compares X/Z distance to the first actor. Distance >=
the record threshold selects mode 0, smaller distance mode 1. The observed
file-backed table has six distinct thresholds, spanning 20000..43000
units. This is an aggregate survey, not a copied table. The paired model
readiness/retirement flags prevent dropping the old variant immediately.
No distinct enter/exit threshold was found in this function.

## Scenery, enemies and NPCs

These classes must not inherit the terrain-model radius by assumption.
The traced common group gate `0x00cc4f20` combines story, hour and flag
conditions with `0x00cc5670`: a group without a spatial condition passes
that spatial test; otherwise it asks authored shape objects at +0x38 via
their virtual +0x20 point-containment predicate. A linked group can supply
the spatial condition. See [SPAWNS.md](SPAWNS.md) for the other conditions.

`cLayoutSetCharaBase::vf06` (`0x0075f860`) and `0x0075f9c0` construct
eligible missing entries through virtual +0x3c; enemy/NPC implementations
are `0x00760a10` and `0x00762000`. The object implementation is
`0x00762d30`. None of this establishes a universal per-class metric spawn
distance: layout residency and authored group regions intervene first.

`cSetInfoCoord.mDrawDistance` is reflected at +0x64, initialized to -1
by `0x0079ec40`, and serialized by its virtual methods. Its name is not
proof that it controls construction or unloading. **Its downstream drawing
consumer, individual actor distance sleep/despawn rules, and complete
class-specific exceptions remain open.** Do not present the 3-by-3 layout
candidate window as an enemy/NPC spawn guarantee.

## Floating origin

`sWorldOffset` is global `0x018fa528`. `0x004c66b0` establishes the meaning
of the previously unexplained +0x2c/+0x30 fields: zero-based grid N(X)/M(Z),
not the filename indices. For descriptor origin `(ox, oz)` and cell size
`(sx, sz)`, it recovers fixed-stage coordinates from the current local
position plus `(ox + old_n*sx, oz + old_m*sz)` when enabled and initialized.
It then uses truncation toward zero of `(fixed_x-ox)/sx` and
`(fixed_z-oz)/sz` (`CVTTSS2SI`, `0x004c6744..0x004c676c`). This is not a
floor operation for negative inputs.

On first initialization the delta is `(-ox-new_n*sx, 0, -oz-new_m*sz)`.
On a change it is `(-(new_n-old_n)*sx, 0, -(new_m-old_m)*sz)`.
The delta goes to +0x40 and accumulates into +0x50, the absolute offset.
Unchanged indices return false: no extra distance threshold or spatial
hysteresis appears in this function. It does not clamp the indices.

`sWorldOffset::vf06` (`0x004c64e0`) controls when this runs: it requires
the stage grid, split controller and actor; initialization state 1 waits
for `0x00c63850`, then enables the offset. Active state 2 checks the actor
position and, on change, dispatches through sMain virtual +0x20, updates
split collision through `0x00c5cb90`, and invokes `0x00c60ca0`. The complete
fan-out to every cached transform is not ported or exhaustively traced.

## Passive observation

Read-only `research/ddda-oracle/client.py` `info`/`read` operations under
head's board authorization #1292, 2026-10-09 15:12:05 UTC, PID 8300,
frame counter 4091325. Head reported the game paused at a tutorial popup;
it was left untouched. The probe was already installed. Raw bytes and
addresses stay in private temporary files, not the repository.

Stage 100 reported widths `(5,5,5,5,3,3)`, descriptor values as above,
origin indices N=19/M=18, and absolute offset `(10000,0,-80000)`.
This is filename cell `st100_58m49n`. Actor local position was approximately
`(3156.9133,39957.5977,2547.5808)`. Reconstructing fixed X/Z gives
`(-6843.0867,82547.5808)`, and the traced formula returns indices 19/18.

The terrain list contained all nine combinations N=48..50, M=57..59;
positions were `(N-49)*10000, 0, (M-58)*10000`. All wrappers had status 2.
The collision list had eight of those cells (missing N=50/M=59), status 3.
The layout list had six: (48,58), (49,57), (49,58), (49,59), (50,58),
(48,57), all status 3. These are observed resident sets, **not predictions
of a fully reconstructed distance predicate**. Paused-state history can
matter. No boundary crossing, fade duration or spawn transition was tested.

The older snapshot in [ORACLE.md](ORACLE.md) also agrees: indices 22/23
plus filename offsets 30/40 give 52n/63m; absolute offset is
(-20000,0,-130000). This resolves those fields without changing that doc.

## Proposed core design and remaining gate

Keep authoritative positions in a fixed stage frame in `ddworld`. Compute
resource interest independently for every player and union those interests
for host simulation/collision residency. Preserve each group's persistent
state when a cell unloads. Render interests and floating origins belong to
each client/view in `ddview`; rebasing a view must not move authoritative
entities or network coordinates. This is a proposed co-op design, not a
claim about original multiplayer behavior.

Use distinct desired/resident/retiring states and retain dependencies until
replacement resources are ready. Separate the measured original policy
from client quality settings. Read grid/region/landmark tables from the
player's installed exe and layouts from their archives. Do not implement
the unresolved angular expression with a guessed circle.

Next: identify and independently evaluate `0x012f161f`/`0x012f0906`, recover
all call-site kind arguments, and compare predicted residency to a stable
passive snapshot. Then trace `mDrawDistance` to its consumers and record
ordinary boundary crossings when the maintainer supplies them. The brief's
full per-class distance and transition-comparison gate remains open.
