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

The distance-filter construction gate has an initialization exception:
controller byte +0x4d starts at 0 (`0x00c5bc92`) and is set to 1 at the
end of each valid controller update (`0x00c5cb76`, `0x00c5ca60`).
Archive loaders `0x00c5e210`/`0x00c5e740` and model/collision/layout
construction `0x00c5f070`/`0x00c5f680`/`0x00c60510` bypass the additional
distance predicate unless this byte is exactly 1. This makes the first
valid update broader than later construction. Retention has its own
predicate checks; do not apply the construction bypass to it.

### Additional distance filter

`0x00507510` takes the party's first actor's position at +0xe0, falling
back to +0x40 when the former is the zero vector, and subtracts the world
absolute offset. It calls `0x005075d0`, also called directly by loaders.
Its hidden EDI argument selects model/collision/layout fields: the
decompiler's displayed signature omits it. Call-site assembly sets EDI to
0 at `0x00c5f189`/`0x00c5f374`, 1 at `0x00c5f7f7`/`0x00c5fa04`, and 2 at
`0x00c6067e`/`0x00c608d0`, for retention/construction respectively.

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
The angular term is `abs(sin(2*acos(t))) * (sqrt(2)-1)*cell_width_x`;
`t` is a clamped Z component
of the normalized difference between normalized player and cell-centre
directions in the fixed stage frame. The helpers are acos at `0x012f161f`
and sin at `0x012f0906`: their assembly uses the pi/2-minus-input small
argument path and 0/pi endpoints (`0x012f199c..0x012f19fb`, split constants
at `0x0144ea60`/`0x0144ea80`), and sine's small-input identity and
sin/cos reconstruction table (`0x012f0a06`, `0x0144b410`). Both normalize
only when magnitude >=2^-23. Exact library approximation/rounding at
threshold boundaries remains unverified. The final comparison is
strict: equality rejects (`0x0050790d..0x00507925`).

An apparent night alternative in the decompilation must not be "fixed":
the assembly at `0x005076f8..0x00507702` requires unsigned hour >=21 and
<=3 together, so the alternate coefficient array is unreachable on this
path. No claim is made about other night-specific systems.

## Unload delays and hysteresis

After the initialization bypass, the inspected model and collision paths
use the same spatial interval and distance predicate for entry and retention; no second larger unload radius
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

### Authored group survey

`ddtool stream-survey <nativePC>` reads all installed GPL resources and
aggregates the second area list only for groups whose scalar 38 is nonzero.
`0x00cc4690` loads this scalar into +0x70; `0x00cc5670` tests that field
and the +0x78/+0x84 area count/list. The linked-group fields are
+0x130/+0x134. A linked group can therefore use another group's gate.
The installed corpus contains 199 lists and 3925 groups:

| Resource suffix | Lists | Groups | Spatial enabled | Linked | Enabled but empty |
| --- | ---: | ---: | ---: | ---: | ---: |
| dlc01 | 2 | 6 | 0 | 0 | 0 |
| e | 52 | 1169 | 837 | 14 | 4 |
| n | 40 | 606 | 0 | 0 | 0 |
| p | 52 | 1734 | 0 | 0 | 0 |
| t | 53 | 410 | 1 | 0 | 1 |

Enabled groups contain 1525 cylinder shapes, radii 50..44525.98 units.
These are authored region sizes, not universal character spawn distances.
Suffixes are reported literally, without inferring runtime classes.

### Enemy reduced-update distances

`0x00853dc0`, called by `uEnemy::vf08` (`0x00aa94d0`), classifies
3D distance from actor reference +0xe0 to the first player reference.
With near/far fields +0x2984/+0x2988, distance strictly below near selects
mode 0; near through strictly below far selects mode 1; far or greater
selects mode 2. A parent actor can supply its mode instead. On stage 100,
the availability check through `0x005072e0` / `0x00c5f630` requires the
matching **collision** cell in state 3; unavailable selects mode 3.
The classifier itself has no distinct enter/exit radii. Modes 2/3 skip the
full enemy/model update in `0x00aa94d0`; this is not proof of deletion.

`uEnemy::vf05` (`0x00aa8830`) reads near from the float table at
`0x014f9768 + 4 * actor[0x2d]`, multiplied by the quality factor 1/2/3.
A nonnegative override at +0x2994 replaces it; far is near +20000.
For example, subtype 5 (goblin) has base near 4000, hence near/far
4000/24000 at quality 1 without an override. The table must be read from
the player's installed exe, not copied as bulk data into the engine.
The default character constructor (`0x0084a100`) supplies 10000/30000
before class initialization and sets the override to -1.
`cSetInfoCoord::vf08` (`0x0079f230`) casts the created object to
`uCharacterBase`, then copies placement +0x64 into actor +0x2994 (and
+0x68 into +0x2998). This resolves the common placement override path.

### Scenery model update distance

`uOmSetBase::vf08` (`0x00c55380`) has a separate stage-100 gate: when
its readiness/update flags permit and object flags +0x3c4 lack 0x200,
it compares squared 3D player distance to squared radius +0x430.
Distance greater than the radius clears +0x448 bit 1 and skips the normal
`uSetModel::vf08` path. Equality remains eligible. This is model-update
suppression, not a traced object destructor or universal scenery rule.
Other readiness, flag and indoor visibility tests also intervene.

`uOmSetBase::vf30` (`0x00c556c0`) initializes +0x430 after transform work:
if model field +0x16c exceeds `sGameSys+0xbe460`, choose the large-object
radius +0xbe45c, otherwise choose +0xbe458. The exact geometric meaning
of +0x16c is not established here; call it the size selector, not height.
`0x0044d770` initializes these values from stage category and multiplies
the radii by the same quality factor 1/2/3 (the size cutoff is unscaled).

| Stage category | Small radius | Large radius | Size cutoff |
| --- | ---: | ---: | ---: |
| 100, or 200..250 except 220 | 2500 | 100000 | 300 |
| 220 | 2000 | 10000 | 300 |
| 601..609 | 5000 | 100000 | 300 |
| Other | 3000 | 100000 | 300 |

These are small embedded policy constants sourced to `0x0044d770`, not
an asset table. The table describes initialization; the inspected distance
consumer `0x00c55380` applies its radial test only on stage 100. Other
scenery subclasses and indoor policies must be checked before generalizing.
A passive parameter read at 2026-10-09 15:37:57 UTC confirmed stage-100
values 2500/100000/300. No scenery distance transition was induced.

### Managed NPC distances and budgets

`cSetInfoCoord.mDrawDistance` is at +0x64, initialized to -1 by
`0x0079ec40`. Its NPC consumer is now traced: `cSetInfoNpc::vf08`
(`0x007a32f0`) copies it into `uNpcIntel+0x4fc`. The Intel constructor
`0x00bbbfa0` sets near +0x4f4 to 5000; initialization `0x00bbc4e0`
uses the override if nonnegative and sets far +0x4f8 to near +20000.
Thus the default initialized pair is **5000/25000**.

`0x00bbece0` computes distance +0x4ec from Intel position to player or
camera reference depending on its control flags, with availability,
state and special-NPC exceptions. `sNpcIntelManager::vf10` (`0x00471d80`)
selects `0x00472ad0` or `0x00473180` using game flags and `0x00421ce0`.
These policies assign Intel mode +0x4f0 using the near/far distances,
priority, and manager budgets +0x16dc/+0x16e0. They are not a single
radial despawn rule. The first policy also uses six-plane sphere inclusion
(`0x00dc16e0`), ray occlusion and previous mode; an inactive candidate
outside the visibility test must be below 500 to enter one queue, while
already-active candidates follow another queue. Queue sorting
`0x00472910` uses ascending distance. The alternative policy prioritizes
visible candidates inside near, then other candidates inside near, and
can assign mode 1 inside far subject to the secondary budget.
The full queue priority/exception logic is not translated here.

`uNpc::vf08` (`0x00baf4f0`) reads this Intel mode through actor+0x5854;
modes greater than 1 skip its full update. Without Intel it uses the
character mode. These are update gates, distinct from layout construction
and eventual destruction. Other scenery subclasses and the full NPC lifecycle exception set
remain open; the 3-by-3 layout
window is not an enemy/NPC spawn guarantee.

The manager budget fields are reflected as `mDrawMax` (+0x16dc) and
`mMoveMax` (+0x16e0), with `mManagementMode` at +0x16e4
(`0x00471a70`). Initialization `0x00471c10` sets draw budget to 12/24/36
for quality 1/2/3, move budget to 0, and management mode to 0.

### NPC physical actor lifecycle

The mode feeds construction/removal, not just update suppression.
`uNpcIntel::vf08` calls `0x00bbea90`. With no actor at Intel+0xf8,
`0x00bc0eb0` normally rejects mode 2; NPC 990 and the selected game-system
Intel bypass that check, and stage-100/story flags add other exclusions.
The accepted path requests the NPC archives, waits on +0xfc resource
state, checks `0x00bbef40`, and acquires a shared construction-budget slot
before calling `0x00bbe070`. That function obtains the physical actor,
links actor+0x5854 back to Intel, transfers its persistent parameters and
near distance, sets position, and attaches the Intel behavior state.
Thus distance eligibility alone is not immediate physical construction.

With an existing actor, `0x00bbea90` checks mode 2. Actor state +0x5824
values 1/2 and attached work checked by `0x00bade80` can defer removal;
otherwise `0x00bbe670` detaches its Intel/behavior links, requests removal
through the character manager, clears Intel+0xf8, and resumes Intel's
lightweight behavior/navigation state. `0x00472320` is a distinct path
that queues removal of the Intel itself, marks +0x4e9 and requests any
physical actor's removal. It is reached for group invalidation and some
availability conditions, not simply every crossing of near/far.

A passive registered-list aggregate at 2026-10-09 15:40:03 UTC found
3 Intel entries in mode 0 with physical actors and 27 in mode 2 without
actors. This agrees with the traced lifecycle but is a sequential snapshot,
not an observed disappearance or reappearance transition. The complete
story/attachment exceptions and construction-rate budget are not ported.

A second passive observation at 2026-10-09 15:33:38 UTC found 30 Intel
entries registered with the manager. The first four registered entries
all had near/far/override = 5000/25000/-1 and modes 0,0,0,2;
manager budgets were 12 and 0. Vtable-only enemy scanning produced
unvalidated matches and provides **no enemy threshold observation**.
These sequential reads were non-atomic and used only read/scan operations;
no game control, state writes, or boundary movement was performed.

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
it was left untouched. Later passive reads found a small position change,
so these sequential reads are not claimed to be an atomic frozen snapshot.
The probe was already installed. Raw bytes and
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
(48,57), all status 3. A private Python evaluation of the traced formula
at the recorded position selected **exactly the eight collision and six
layout cells**, with no extra or missing cells. A later passive read found
the multiplier setting =1, hour=4, and actor+0xe0 equal to +0x40.

The same calculation selected 13 model candidates, four more than the
resident set. Two additional cells (N47/M57 and N47/M58) have no installed
cell archive; the other two (N48/M56 and N49/M56) do. A passive follow-up
of the controller's main archive list (+0x108 count/+0x114 pointer) found
eight entries matching the collision set. `0x00c5dda0` returns false when a candidate lacks a matching entry in that
archive list, irrespective of whether its archive exists on disk. Thus all
four extra model candidates fail the contemporaneous construction dependency
gate. The ninth resident model can pass model retention after its archive
entry leaves the narrower archive set: model retention does not call
`0x00c5dda0`. Combined with the first-update bypass, this gives a traced
mechanism compatible with the observed nine models versus eight archives.
Its actual construction history was not recorded, so this is an explanation
of permitted residency, not a reproduced transition sequence. No boundary crossing, fade duration or
spawn transition was tested. The numerical check used host double precision,
not a bit-exact emulator of the original SSE library.

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
the angular expression as a plain circle.

Next: compare a selected threshold boundary under the current live-session
safety protocol, including physical actor residency and archive dependencies.
Other scenery subclasses, NPC construction-rate limits and full policy
exceptions remain to be traced. The terrain pattern has a compatible
traced explanation, but its actual construction history was not recorded.
The brief's transition-comparison gate remains open.
