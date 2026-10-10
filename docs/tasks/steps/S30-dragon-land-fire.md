# S30: The dragon lands (PressAttack) and breathes fire (BreathWide)

- **Branch:** `step/s30-dragon-land-fire` (after S29)
- **Files:** `crates/ddenemyrt/src/dragon*.rs`, host/view hooks, tests
- **Kind:** one feature

## Context (S24 map)

State 0 PressAttack (33): motion 298 then 299, `0x00A6AED0/0x00A6AF70`,
falling/ground contact, parameter-driven acceleration from
`cCharParamEmDragon`, landing consequences; its 150-frame branch can
dispatch EscapeFly (60). States 7/8 wait motion frame >= 10, then motion
end. State 2 BreathWide (21): 263 then 264, `0x00A56A30/0x00A56D10`, exit
`0x00A57270`, frame-gated breath collisions, emitter/joint targeting,
damage. State 2 sets stage LOT 80 and q0 bit 20; state 9 timer 0 >= 5.3 s;
state 10 q0 bit 21; state 6 q0 bit 5 and Kill.

## Do

Translate PressAttack and BreathWide (with their breath volumes and
damage against the party), continue the FSM to state 6, and apply the
FSM's quest/LOT/GSF requests through the host.

## Done when

A headless test plays the entrance to state 6 with real feedback, the
breath damages an Arisen in its volume, the FSM's flags reach the story,
and a capture shows the landing and the fire.

## Results (source checkpoint, acceptance still open)

The headless dragon now exposes typed PressAttack and opening BreathWide
actions. Press entry/update are translated from `0x00A6AED0/0x00A6AF70`;
Breath entry/update from `0x00A56A30/0x00A56D10`. Motion selection decodes
the native packed bank/slot IDs, including 298 -> 299 and 263 -> 264.
Primary event group zero supplies Press's falling bit through the existing
`0x01057060` interval evaluator, sampled for the next phase 91.

Press's initial/terminal velocity and acceleration are read from the
installed `charparam/em/em5800Dragon` PRPZ resource through `mtprp`. No
character record is embedded. The first installed regression caught an
incorrect raw-XFS parse; using the existing PRPZ reader corrected it.
Head approved the dependency/lockfile scope in board #4502.

Ground contact remains an explicit host observation. Its phase-90
projection is translated from `uObjModel::vf90`, `0x00885E30`: previous
terrain flags +0x1B68 bits zero/one, with bit20 additionally admitted in
ground mode +0x1B6D; an enabled axis check with bit0x400 suppresses contact.
Missing contact input rejects Press stepping. No estimated height or
landing timer stands in for the terrain solver. Landing binds 299,
notifies GSF463 when actor +0x679A permits it, and clears that flag at
frame60. The 150-frame recovery returns an explicit EscapeFly request
with the cached position shifted by the source's 500cm; action60 itself
is not implemented here.

Breath binds 263, waits for its real motion terminal, then binds 264.
Frame35 starts the effects; frame165 stops them after that frame's
collision-particle query. Typed requests represent prewarm/start/query/
stop and the phase-specific actor bookkeeping; the effect engine and
host still need to consume them. They do not manufacture a cone or hit.
The opening variant is explicit: sequence4, controller+0x0C clear.

Validation: five synthetic source regressions passed. One installed
regression passed with real parameter reads, primary falling events,
bank selection, landing motion terminal and breath clocks at 35/165.
Its terrain contact is explicitly synthetic adapter input; it does not
prove terrain integration. The two existing installed dragon entrance
regressions also passed (1.21s compile, 3.83s existing tests, 3.71s new
test). After phase-control request separation, the final five unit tests
and installed regression passed again (1.66s/3.00s compile, 4.03s installed
test). Every Cargo process actually exited zero for these final checks.

**Acceptance is not complete.** The host's entrance-only gate remains
enabled. Real-stage landing verification, action dispatch through the whole
FSM, effect request consumption, authentic particles/party damage,
state6 story feedback and the landing/fire capture remain open. This is
implemented/tested against static source and installed resources; it has
not been compared with a recording of the original.

The breath source resolves EPV index4 element1 for visual fire and
element2 for collision (`0x00A55BC0` disassembly); `0x00E6CA90` supplies
up to15 active type11 particle positions and interpolated radii. The collision effect resource
contains a single such unit, so its generator/movement/lifetime source
is being traced before translating any volume. Bulk EPV/EFL records,
private decompilation and dumps remain outside the repository.

The type11 radius substep is translated from `0x012AC1E0/0x012AC260`:
growth advances once per effect update, crossing the upper limit clamps
and stops growth, and crossing below zero retires the particle. Radius
double buffering and the four-component sample interpolation from
`0x00E6CA90` are represented without an emitter or synthetic contact.
Three additional regressions verify exact-limit versus overshoot behavior,
zero versus negative retirement, buffer retention and unclamped sample
fraction. All eight source tests and the installed actor test passed
(2.59s/4.96s compile, 3.66s installed test) after the narrow debug rebuild.

The installed scenery group is a horizontal capsule attached to joint254.
`uEmDragonBase::vf33`, `0x00A3F820`, disables the axis check (+0x1B60=0)
and sets the character's collision filter. The player's vertical capsule
must not substitute for that shape. Scenery point lookup at `0x0076C540`
passes normalize=1 through `0x00CCDE30/0x00CCE190` (verified disassembly),
preserving joint scale. A generic capsule/optional-axis extension to the
existing headless sweep is requested in board #4644 before shared edits.

Next action: translate the installed collision particle unit
before enabling the remainder of the encounter.

Head approved the shared sweep extension in board #4751. The existing
slide loop now accepts arbitrary capsule endpoints and an optional axis
query; the player API constructs its existing vertical shape/type2 query.
Two new regressions cover the horizontal far endpoint's broad-phase box
and the difference between a disabled axis and a failed enabled query.
The full DDSIM suite passed: 65 library, seven bow and six shot tests.

The default cObjCollision type0 scenery adapter is translated from
`0x0076C540`, using preserve-scale node lookup (`0x00CCDE30/0x00CCE190`).
The constructor `0x00769C70` initializes type0 and +0xD9E=1. Its radius
uses model +0x64, the Y scale, rather than the combat volume's mean scale.
The type1 length/width override path and special negative joints remain
unported; missing pose evidence is reported instead of invented.
One synthetic regression covers nonuniform scale and unsupported joints.
An installed regression reads the actual scenery node, samples Press's
real posed joint and sweeps it onto an explicitly synthetic plane. This
verifies the geometry adapter/contact projection, not stage terrain or
production phase order. All nine action/helper unit tests and both
installed tests passed (4.99s compile, 3.88s installed run), actual exit0.
The host remains entrance-only; the full acceptance gate above stays open.

The host now supplies its triangle provider to Press pass C and reads that
result at the next phase90. A missing provider is an error. Ground mode
and the constructor's ground-check offsets follow `0x00A6AF70` and
`0x0079D850/0x0079E200`; other runtime scenery type/filter/offset overrides
are still unported. `0x00A3E5E0` initializes the landing-notification
disable byte to zero. GSF463 effects now reach the host story. The FSM's
actual ActionEx33 mode0/delay lane dispatches the typed Press action.

One new installed host regression continues the real entrance states
4/5/1/3/0, lands through the posed capsule sweep on a synthetic plane,
waits for the actual motion299 terminal, and observes GSF463 set/clear.
It writes no contact/completion flag. It deliberately leaves the authored
area condition false, so it does not enter the fire states. The existing
entrance host regression also passed (6.80s compile, 5.27s existing test,
6.05s new test), actual exit0 after correcting the triangle callback's
return type. There is no real-stage capture or original landing comparison.

The collision generator emission clock is now translated from
`0x010CD0C0` and `cParticleGenerator::vf10`, `0x010D6930`. Activation,
owner delay, single emission versus restart, forced stop, per-update age,
signed delay wrapping and RNG counter consumption are explicit. A restart
advances the counter even when its variation is zero; a nonzero variation
requests the native random-table word by index from its caller. No RNG
table or seed is embedded. Binding preparation is a typed request; this
clock does not allocate particles or implement the whole generator.

The installed collision generator has no restart flag. The allocation
caller `0x010F9B10` allocates one particle for an emission request and
updates its lifetime afterward; capacity is not a per-frame spawn count.
External binding/restart behavior still needs integration evidence before
claiming the complete breath's particle count. Type4 lifetime initialization
dispatches to `0x010EDC90`, not the separate type10/11 initialization paths.
Its path is constructed by `0x010F1230`/`0x010E5620` and updated through
`0x010F1430`/`0x010E6720`; the type4 mover consumes distance along that
path (`0x010E2EB0`). These paths are still being traced. Fire motion,
visuals and party damage remain unimplemented, and the full gate stays open.

Three new clock regressions passed, covering activation/initial delay,
one-shot retirement, randomized restart gaps, forced stop, zero-variation
counter advance and signed wrap. Both installed dragon action/scenery
regressions also passed (6.12s compile, 4.63s installed run), actual exit0.
The first compile encountered a stale shared-target DDSIM artifact from
another worktree; a narrow DDSIM rebuild restored the branch's capsule API.
An initial namespace filter ran zero tests; the explicit generator filter
then ran all three new tests. No fire/contact acceptance was inferred.

The type1 movement fade state selection/update is translated from
`0x010FA8D0/0x010FAD70`. It accepts already sampled/adjusted durations;
the owner-specific adjustment at `0x010F9CB0` remains outside this helper.
Fade-in uses the native duration+1 denominator. Reaching Done from fade-in
or hold stays live for that update, while fade-out's terminal retires in
the same update. Generator-held lifetimes, detachment, hold limits and
graceful stop are represented. Three new regressions cover these boundaries.

Head approved the existing `mteffect` dependency in board #5065. The new
`dragon::particles::CollisionUnit` reader follows the installed EPV index4,
element2 to an explicitly selected list slot, then reads the generator,
particle11, movement1/tag0x12 and lifetime4 fields. It reads radius,
capacity, axis selectors, duration ranges, speed/acceleration, initial path distance
and point count from the installation; none of those records are embedded.
Unsupported kinds/inherited bodies, truncated fields and nonfinite scalar
values reject. The native LOD selector, parameter overrides, complete path
and particle simulation are not implemented by this reader.

The generator +0x30 field was initially labelled as a binding joint in
the reader, without being used to produce positions. Tracing
`0x010EC540` shows it packs two axis-selector nibbles into runtime +0x108
bits8..15. The field is corrected to `generator_axis_selectors`; +0x78's
low byte is read separately as the emission shape used by `0x010F37F0`.
The decoder regression includes nonzero upper bytes to check both packed
fields. All four particle reader/path/duration tests passed (4.69s
compile, 0.00s test run), actual exit0. This adds no guessed bone binding
or damage geometry. The native type4 path constructor/update and provider
birth transform remain the next integration step.

Raw duration sampling follows `0x010FA8D0`: three counter increments,
inclusive integer variation, zero-variation table-read suppression and
explicit rejection of unsupported hold curves. This sampling precedes the
unported owner adjustment. Two synthetic regressions cover native counter
wrapping/range behavior and malformed/unsupported units. A new installed
regression resolves the actual EPV/EFL, observes one emission request and
the separate resource hold clock through retirement, without manufacturing
any particle position, collision or damage.

All 26 dragon library regressions and all three installed action/effect/
scenery regressions passed (6.35s/5.53s compile; 6.99s installed run), actual
exit0. This validates the reader and source helpers, not the full fire
runtime or original-game acceptance. Next action remains binding the
authentic type4 path/mover before enabling the rest of the encounter.

The owner duration adjustment is now separately translated from
`0x010FA8D0/0x010F9CB0`: parent effect+0x11A low nibble2 preserves
durations; mode0 shifts by2 and other modes by1, retaining zero and
clamping positive results to at least1. The caller must provide the actual
parent state; no quality mode is guessed. One focused regression passed
(1.97s compile, actual exit0). Runtime parent-state selection remains open.

The constructed path's distance sampler is translated from `0x010E2EB0`.
It walks actual segment lengths, skips degenerate segments, preserves the
source's unclamped fraction and emits the separate 0x400 end result at
the runtime total-length boundary. It accepts supplied native path points
and emitter position; it does not construct guessed fire geometry or
decide lifetime retirement. One regression passed (2.42s compile, actual
exit0), covering corners, negative distance, zero-length segments, exact
end/overshoot and missing/nonfinite input. Path construction/binding remains
the next unresolved runtime dependency.

The unconstrained, zero-angular type4 path constructor and spring update
are translated from `0x012A1330/0x0129AF50`. They use caller-supplied
sampled fields and transformed direction/force, preserve the native
per-update velocity, taper damping, stretch correction, fixed segment
projection and total-length accumulation. No game path table is embedded.
The supported `0x010E6720` emitter update moves the first point by the
actual emitter displacement and restores relative coordinates afterward.
Collision-constrained, angular and zero-iteration construction paths are
explicitly outside this adapter.

Four new regressions cover stretching/compression, taper boundaries,
degenerate segments, normalized construction/relaxation and an emitter
moving sideways while its trail bends. All eight particle tests passed
(3.15s compile, 0.00s test run), actual exit0. These helpers are not yet
connected to the host's BreathWide query or normal damage pass. Provider
birth orientation, sampled runtime fields and effect ownership remain the
next integration step; full S30 acceptance stays open.

The production controller/condition projection now follows its actual
constructor and FSM state. `cAICtrl::ctor` (`0x0059DA00`) initializes
+0x0C's low byte to one. `0x0059F440` clears it for an active FSM frame
and restores it on Finish/Kill; the existing thinker's paused/disabled
states represent those outcomes. `uCharacterBase::ctor` (`0x0084A100`)
initializes actor +0x2738 to zero. OpeningDragon owns this condition word
and the controller guard, while retaining the explicit native-state
override for adapters. Its phase93 no longer keeps the constructor HP
protection solely because a host override was absent.

The installed entrance-to-Press host regression now observes the initial
HP lock cleared after the actual first controlled FSM frame. It passed
(10.06s compile, 6.49s run), actual exit0, alongside the existing landing,
real motion-end and GSF463 checks. The first compile used another branch's
cached DDSIM without the S30 capsule API; a narrow DDSIM rebuild restored
the correct source. This does not inject HP, completion or condition bits.
Runtime scripted condition changes must update the canonical word; the
full fire/damage route and original comparison remain open.
After integrating reviewed main `104fa88` into this task branch, all 32
dragon library tests passed (3.43s compile, 0.00s run), and the existing
installed entrance host test passed (2.90s compile, 5.60s run), actual
exit0. Main was not modified.

The breath collision-entry adapter is translated from
`0x00A56D10/0x0076F9C0/0x0076FAB0`. It accepts actual type11 samples,
caps them at15, and produces ordinary world-sphere attack volumes with
the installed attack row, flags, reset time and sequence relation/key.
The collector reads group/attack numbers from primary SeqIndex slot1
but passes slot0 for sequence metadata. Slot1 must not be mistaken for a
literal attack-table index or used as the metadata slot. Radius is already
scaled by the effect; the adapter does not apply character scale again.
The caller supplies the actual primary resource and optional slot1 fallback.

One installed regression uses the bound Breath resource with explicitly
synthetic sample positions to verify the15-entry cap, metadata, row/flags,
ordinary contact versus miss, and invalid live-radius rejection. It passed
(3.70s compile, 3.52s run), actual exit0. This validates the collision-entry
boundary, not a native effect trajectory, normal HP loss or a production
fire capture. The host still needs the complete sampled emitter/lifetime
ownership chain before enabling its BreathParticles query.

Provider quality selection is translated from `0x00CB6CF0/0x004298B0`.
It packs the resource's three criteria, honors its global-criteria selector,
clamps the requested tier against the supplied effect-manager quality, and
walks downward with the original strict distance boundary. Effect kind4
suppresses creation. The selected state and duration mode remain distinct:
assembly at `0x0042B020/0x0042D750` verifies the duration argument is packed
into effect+0x11A's low nibble, not the state nibble at +0x119's high half.
No effect-manager settings or criteria are inferred from the resource.

One synthetic regression covers resource/global precedence, tier clamping,
distance equality, suppressed creation and the resulting duration reduction.
It passed (3.98s compile, 0.00s run), actual exit0. The initial compile used
a shared cached DDSIM without this branch's capsule API; a narrow DDSIM
rebuild restored it. Runtime ownership, birth transform and full fire damage
remain open.

The owning effect's integer/fractional clock is translated from
`0x00FDF5B0/0x00E6BCA0`, reached by `uEffect::vf09` (`0x00E70710`).
It initializes interpolation to1 and uses the actual +0x112 initial-step
field on its first update, rather than consuming +0xFC's rate immediately.
The default zero initial steps therefore run no generator updates at birth.
Later updates preserve the fractional remainder and return the native
integer count; generator clocks must run only that many times. The caller
supplies the actual effect rate, independent of render FPS.

One regression covering birth, explicit pre-advance, fractional boundaries,
multiple updates and invalid context passed (4.01s compile, 0.00s run),
actual exit0. A narrow DDSIM rebuild selected this branch's source after
other shared-target work. This clock is not yet connected to production
effect allocation or BreathParticles; full fire damage remains open.

The supported point-provider binding follows `0x004298B0/0x0042D8F0`
and `0x00FE0350`. Assembly establishes that the hidden EAX argument to
42D8F0 is the actor-relative offset, rather than the already transformed
world birth point. The binding preserves the actual parent matrix and its
nonuniform scale, applies the authored effect scale to its basis, and
rejects unsupported orientation/scatter modes. It does not translate an
already transformed point a second time.

Type4 path setup now reads its authored flags, iteration count, selectors,
range pairs and curve references at life+0x80. Sampling follows
`0x010E5620`: length, length velocity, force, damping, spring, angular
velocity, then three rotation values. Every draw advances the supplied
native generator counter, including zero variation. Curved/constrained
setups remain explicit errors. These fields are runtime game data.

The opening dragon's data now exposes its real authored EnemyCheck target:
kind2, the loaded group's slot and the layout record ID. The installed
dragon is record1, not the record vector's ordinal0.

Focused validation completed with exit0: all36 dragon library tests
(2.80s compile, 0.00s tests), all4 installed attack tests (5.98s compile,
3.90s tests), and the installed host entrance/Press regression (11.35s
compile, 10.12s tests). This checks provider/path boundaries and actual
target identity, not complete production fire. Full touched-crate suites
after current-main integration are still pending.

Further source audit distinguishes the life+0x44 secondary-effect delay
from world collision delay. `0x010D7380` resolves the collision descriptor
using the ushort at life+6; `0x010CD1C0` copies its byte2 into the particle's
collision delay. `0x010F7190` advances travel with the old speed before
adding acceleration. Production effect ownership, this actual scene query,
and normal fire HP damage remain the next integration work.


### Shared contact projection and full core acceptance

Head approved this shared adapter on board6073. `ddsim::sweep::ground_contact`
now projects the real axis-present flag and ground mode using the native
`0x00885E30` masks: an enabled axis with `0x400` rejects contact; the
ground-mode mask is `0x100003`, otherwise `3`. `Swept::grounded` retains
its existing enabled-axis/non-ground-mode behaviour. Dragon delegates to
the shared helper. The preceding scenery flags and actor update gate
remain caller-owned; this does not invent terrain contact for NPCs.

After integrating reviewed main7a5c414 (task HEAD e6f88ae), the complete
`cargo test -p ddsim -p ddenemyrt -p ddhost` run finished with actual exit0:
DDSIM79 passed, enemy164 passed/2 existing ignored, host95 passed. Compile
35.19s. Private log: E:/ddda-re/notes/steps/S29/full-core-e6f88ae-contact.log.
This accepts the point binding, path sampling, target identity and shared
contact checkpoint; production fire and downstream viewer acceptance
remain open. No new original-game comparison was performed.

### World collision descriptor checkpoint

The collision reader resolves the ushort at life+6 using `0x010D7380`,
then reads the world-query flags, radius growth, initial radius and limit
extension from the installed descriptor. Its byte2 initializes the query
delay (`0x010CD1C0`). The delay update (`0x010F54F0`) suppresses every
initially nonzero counter, including the update that changes one to zero;
the life+0x44 secondary-effect travel delay is independent.

The supported query geometry follows `0x010E2A00`: bit0x1000 selects a
swept sphere only above radius0.00001, otherwise a line is extended at
both ends by the actual scaled radius. Tiny displacements retain their
unnormalized native direction. Floor-only queries and effect-owned scene
filters/traversal/response remain explicit missing adapters. This helper
does not issue a scene query or enable production fire.

The final full enemy run actually exited zero: 165 passed, two existing
diagnostics ignored (11.37s compile, 85.32s total tests). All70 library
tests included the new descriptor/delay/geometry regression; four installed
attack tests passed. Private log:
E:/ddda-re/notes/steps/S29/full-world-collision-reader-fresh-deps.log.
The downstream full host run also actually exited zero: 95 passed,
27.50s compile. Private log:
E:/ddda-re/notes/steps/S29/full-world-collision-reader-host.log.

An earlier full run reused a stale66-test library and was rejected.
Narrowly rebuilding ddenemyrt then exposed a stale DDSIM artifact missing
the source-present ground_contact API; narrowly rebuilding DDSIM selected
both current sources. No failed/stale run is counted as acceptance.
Validation is on this S30 branch's integrated main7a5c414 dependency set;
later S48/S49 integration and downstream viewer remain pending.

Next: connect the real shared effect coordinate block and generator clocks
to type4 paths before production particle queries and ordinary HP damage.
Source `0x00E6DD90/0x00E6E030/0x00E73720` establishes per-coordinate0x130
blocks; a generator's +0x14 points to that block, not the effect's final
matrix. This avoids applying parent/effect scaling twice. Full S30
landing/fire/state6/original-comparison acceptance remains open.

The subsequent current-main integration is f5017cb (reviewed maina24c1ed,
including S48 save and S49 perception). Complete DDSIM/enemy/host suites
actually exited zero in process46000: DDSIM79, enemy169/two existing
ignored, host98 passed; compile32.38s, tests0.01/97.18/200.82s. Both the
new world-query regression and S49 sensor tests executed. Private log:
E:/ddda-re/notes/steps/S29/full-current-core-collision.log. Core lease was
released after actual exit. Downstream viewer for this S30 dependency set
and production fire acceptance remain pending.

### Downstream viewer acceptance of the collision checkpoint

With reviewed main3ecd13e integrated (source47a1518), the complete default
viewer release suite and build actually exited zero in parent process38841.
All89 tests passed, with two existing diagnostics ignored; compile30.51s,
tests11.81s, release build38.57s. Private logs:
E:/ddda-re/notes/steps/S29/full-current-viewer-47a1518.log and
E:/ddda-re/notes/steps/S29/build-current-viewer-47a1518.log.
Together with the preceding346-test core run, this accepts the sourced
world-collision reader/shared contact checkpoint for review. It does not
enable production fire or establish an original-game comparison.

Next: connect the actual effect coordinate blocks and generator clocks,
then issue native-shaped scene queries and ordinary fire damage. Effect
ownership, full landing/fire/state6 and visual acceptance remain open.

### Native coordinate clock checkpoint

The effect-owned coordinate block has its own integer clock at+0x118 and
short delay at+0x11C/+0x11E. `CoordinateClock` translates `0x010FA010`:
initialization copies the already-sampled delay, every initially nonzero
delay suppresses this update (including1->0), and the first active update
increments elapsed to1. Negative short bit patterns and elapsed overflow
retain native wrapping semantics. The caller supplies the sampled delay;
resource seed selection, effect offsets and generator clocks are separate.

The complete enemy suite actually exited zero in parent81744:170 passed,
two existing diagnostics ignored, fresh compile14.43s. All71 library tests
included the new delay/activation/wrapping regression. Private log:
E:/ddda-re/notes/steps/S29/full-coordinate-clock-fresh-ddsim.log.
The first attempt exited101 because a cached older DDSIM artifact lacked
the source-present ground_contact API; rebuilding only DDSIM selected the
current source. The failed run is not counted as acceptance.

This is an unused headless ownership helper; it does not create an effect,
issue queries, change existing caller behaviour or enable production fire.
Next: resolve installed per-coordinate payloads and the native initialization
draw order, then bind the real parent/effect transform without double scale.
Ghidra is stopped for memory pressure; continue from existing private source
notes rather than starting a server.
