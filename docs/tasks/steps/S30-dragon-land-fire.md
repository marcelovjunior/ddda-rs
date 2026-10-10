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
enabled. Production terrain contact, action dispatch through the whole
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

Next action: connect the native terrain-result flags to Press's prior
contact input, then translate the installed collision particle unit
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
