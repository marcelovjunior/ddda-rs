# Travel destination consumer

Source: DDDA.exe 2017-12-13 Steam, identified in METHODOLOGY.md. This
extends MAP.md's trace of map confirmation and item consumption. All executable
inspection was read-only. No game-process writes or new travel recording.

Implemented: `ddmap::travel` pure destination decoding and arrival-seed
extraction, with explicit input state. Tested: three synthetic regressions and
one installed-resource test covering all four built-in stage/start pairs.
Compared against ordinary original-game travel: **not yet**; the requested
existing/passive observation is unavailable at reporting time.

## Selection and stage request

The map constructor `0x0067fd80` initializes built-in count +0x8ac to three
(write at `0x00680358`). The destination-list entry `0x00685450` sizes the
list to that count plus occupied saved Portcrystal slots (`0x0044d170`).
`0x0068fb70` presents the built-ins first, followed by occupied slots;
`0x0068ff00` maps an occupied-row ordinal to the actual slot among ten.
The displayed built-in order remains Gran Soren, Cassardis, Bitterblack Isle,
as MAP.md establishes from the table at `0x017f15c4` and localized messages.
`0x00690060` hides a map-detail control for the built-in landmark ID243;
that is not evidence that the destination itself is unavailable.

Confirmation at `0x00691580` writes the built-in index or `3 + slot` to
actor +0x3e0c; `0x00b77be0` transfers it to active selector +0x3e00.
The throw action `0x00b06c30` recognizes item66 and item1724. Its final
request sequence at `0x00b07064..0x00b070b9` passes **stage in ECX and start
on the stack**. The decompiler's single visible argument loses the stage.

| Selector | Scenario condition | Requested stage | Start |
|---|---|---:|---:|
| 0: Gran Soren | sGameSys.SceNo <7800 | 220 | 13 |
| 0: Gran Soren | SceNo >=7800 | 230 | 8 |
| 1: Cassardis | either | 100 | 56 |
| 2: Bitterblack Isle | either | 400 | 7 |
| 3..12: placed slot | either | 999 | selector-3 |

`0x00401160` is now resolved: EAX identifies the active aStage, obtained
from sArea +0x3834. It accepts the request only while aStage +0x808 equals1.
It then writes stage +0x80c, state2 at +0x808, start +0x810, clears byte
+0x814, and sets the stage's main state at +4 to0x13. It does not itself
move an actor. The helper `ferrystone_request` supports only the menu's
0..12 domain; the executable's else path has no equivalent upper bound.

State13 handler `0x0050ef80` performs transition/fade gates. On its ordinary
path, `0x0050f25e..0x0050f299` copies +0x80c/+0x810 into a pair and invokes
`0x004fbd50` on the stage controller at sArea +0x3830. The latter stores
requested stage/start at controller +0x38/+0x3c and handles stage999:

- The **low byte** of start selects saved stage word
  `sGameSys +0xbe378 + slot*4` when below10.
- An out-of-range byte yields stage10. An empty slot yields stage0;
  this function does not reject either as an invalid travel request.
- The full start remains at +0x3c, and byte +0x40 marks placed arrival.
- Other stages clear that byte; requested stage -1 returns without changing
  stages. The dispatcher builds `aStage%03d`, looks up its DTI, and writes
  sGameSys stage +0x34 and start +0x44 before the area/object-manager calls.

`resolve_stage` preserves this decoding, including byte aliasing (start256
looks up slot0), empty-stage0 and fallback-stage10. It is not validation.

## Arrival seed and later placement

Stage initializer `0x004ffba0` calls `0x004ff910` for stage position/angle
fields +0x780/+0x790. It clamps a negative start to zero, and requires the
start-resource collection at sMain +0x1c270. If sGameSys +0xb883c has
bit0x10000000, a separate restore path uses saved position +0xac650 and
angles +0xac690; the new helper deliberately excludes that branch.

For ordinary built-ins, `0x004fd2a0` initializes defaults, then `0x007a6780`
maps the stage through the 65-entry stage table at `0x01530348`, checks the
start row against the resource count, and invokes `0x00ce8530`. The latter
reads the rStartPos row's XYZ at +0x10 and scalar angle at +0x20 into yaw,
plus the three companion offsets/angles already described in MAP.md.
The installed regression confirms rows13/8/56/7 exist respectively in
`scr/st220/etc/st220`, st230, st100 and st400, with finite position/angle.

For a placed destination, `0x0044d0d0` reads the selected saved float4 at
`sGameSys +0xbe3a0 + slot*16`; `0x004ff910` initializes arrival angles to
zero. This connects a placed menu entry through its actual slot to the
stored stage and position. Placement and pickup populate/clear those same
arrays through `0x00b12c20`/`0x00b12fd0` as traced in MAP.md.

Both paths call `0x007b0580` with vertical offsets +200/-100 and raw
threshold bits0x3f7469c7. It queries collision through `0x00dea870` and
replaces the seed with the hit position when a suitable hit exists.
The stage initializer stores the resulting position at saved +0xac650 and
yaw at +0xac694. Collision, subsequent world-origin transform and actor
creation are not translated by this brief.

`arrival_seed` therefore returns the **pre-collision** position and angle.
It returns None for missing resources/rows or invalid slots instead of
imitating zero defaults. It accepts the caller's already selected stage
resource; it cannot check that association itself. Negative fixed start
clamps to zero, but malformed placed requests are unsupported: the original
can clamp their full start later despite having used its low byte earlier.
This API does not promise fidelity for arbitrary corrupted requests.

## Consumption, cancellation and remaining gates

In `0x00b06c30`, readiness `0x00ac8e20` plus a non-null active inventory
entry starts the special throw path, motion0xff and action byte +0x80.
The update accumulates sMain.mDeltaTime at action +0x7c. At >=110 clock
units it calls `0x00b77130` **before** checking for an active aStage and
before `0x00401160` checks +0x808. A motion progression check at actor
+0x3cc >54 releases the thrown object earlier; it is not the consumption
threshold. These are clock units, not wall-clock seconds.

As independently traced in MAP.md, the generic use path decrements one
ordinary Ferrystone but explicitly exempts Eternal Ferrystone1724. The
stage queue's failure to accept a request has no demonstrated refund path.
Do not implement a refund, or assume this ordering is a safe co-op contract,
merely from expected game behavior. If readiness never advances, this
function has not yet reached that consumption call. All destructor/cancel
and upstream inventory-use restrictions are not fully traced.

The list-building path exposes three built-ins and currently occupied slots;
it does not establish that an item may be used in every current stage.
Upstream quest/stage/item-use restrictions, exceptional branches in the
transition handler, stale-slot race handling and failure cleanup remain
unresolved. No helper is named or exposed as an allowed-to-travel predicate.

## Contract for later host work

Carry an explicit actor ID, inventory entry/item identity, selected fixed
destination or saved slot, current stage/scenario, and a versioned snapshot
of shared slot stage/position state. Keep destination decoding separate from
host approval, inventory mutation and stage-transition scheduling. The host
will need to resolve the missing original use gates, recheck slot occupancy
and resource/arrival availability, and define multiplayer transition policy
with the core owners. This is a proposed input contract, not a derived
co-op policy or implemented replication behavior.

## Observation blocker and next action

Board request #444 to claude-4 asked only for existing captures or passive
reads during ordinary maintainer play, explicitly excluding inputs, teleport,
spawning, setters, injected calls and all process writes. It asks for actor
queued/active selectors, saved slot stage/position, current stage, item/count
and before/after position. Reply #534 confirms no travel capture exists.
A passive idle snapshot had queued/active selectors0, stage100 and all ten
slot stage/position records zero. That corroborates an empty saved-slot
state only; it cannot compare a travel transition or consumption.

The source-backed built-in and placed selection-to-arrival traces are
established, but actual observed travel parity remains blocked. Next action:
compare one ordinary built-in and one placed trip from an existing or
passively observed capture, then resolve upstream restrictions before core
integration. No raw executable, save, game resource or capture is committed.
