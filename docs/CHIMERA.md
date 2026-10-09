# Chimera runtime evidence

All addresses refer to DDDA.exe 2017-12-13 Steam. This is an incremental
source translation, not a playable encounter or an original-game comparison.

## Ownership and death

`uEm5200::vf33` (`0x009a0880`) creates child units at body +0x72e0 and
+0x72e4, writes their parent pointers at +0x7278 and +0x72c0 respectively,
and copies the body's scale to each. The classes are `uEm5200_00` and
`uEm5200_01`. Calls to `0x00478d80` create `sObjCollision::cObjAdjIgnore`
records for body/child pairs; these are collision-adjustment exclusions,
not persistent placement-key assignments. Anatomical names and complete
collision-to-life routing still need confirmation.

Death selectors `0x009a4dc0`, `0x009b46b0`, and `0x009bd060` require a
damage context and a not-dead actor. They set bit 2 at +0x2e38 and accept
only when no lower bit is set in either +0x2e34 or +0x2e38. These are
common action-priority masks, not part-state masks. Blocked requests remain
pending. Accepted callbacks call common `uEnemy::vf72` before selecting
the family action.

The body chooses 0x57 during action 0x89, otherwise 0x3a when byte +0x1ed0
is zero. With that byte set, motions 0x212/0x213/0x227/0x228/0x239 map to
0x55/0x56/0x53/0x54/0x52; the fallback is 0x58. The byte's semantic name
is not yet established by this work.

Child 00 selects 0x32, calls parent reaction `0x009a5880`, and clears bit 1
of its first collision object. Child 01 selects 0x38, copies the impact
position, calls parent reaction `0x009a5a00`, and clears bit 1 of its first
three collision objects through `0x009bd850`. Child 01 additionally rejects
death when mode +0x72c4 is nonzero, action is 0x35/0x36, or its existing
parent reports dead. Its other modes instantiate detached/remnant behavior;
they must not become extra encounter kills.

Parent reaction selectors request priority 11 or 12 according to +0x1ed0.
The body's pending death at priority 2 therefore suppresses those reactions.
The action depends on which child died, current motion, and, in fallback
cases, the sign returned by `0x0099e7d0`. The typed selectors retain this
unresolved geometric result as an explicit input. The additional Unit01
notification through `0x0099e9b0` is not implemented.

Common death `0x00aab3b0` passes actor placement keys +0x20ec/+0x20f0 to
`0x004a7910` -> `0x004a7ad0` -> `0x0049f5d0`. The writer requires an
active manager, valid layout group and nonnegative placement index. A
child's placement identity cannot be replaced by its parent's. Cleanup
`0x00aad0c0` queues linked-object message 4 and does not produce this fact.
The stage-380 group-10/index-0 story condition must consume only the body's
accepted, correctly bound persistent death.

## Actions and damage leads

The installed action tables use 0x44-byte records with DTI at **+0**,
different from the goblin table's +0x28. Initialization `0x009a0880`,
`0x009b0f50`, and `0x009b8f10` registers:

| Actor | Table | Count |
|---|---|---|
| body | 0x0184bcd8 | 142 |
| child 00 | 0x0184ec00 | 85 |
| child 01 | 0x01850630 | 90 |

These are runtime-read bulk records, not constants to copy into the repo.
The catalogue reader loads all 317 action records and all 147 non-null
resource requests (including the separately appended collision requests)
from the installed exe. Resource-list bases/counts come from
`0x009a0820`, `0x009b0ef0`, and `0x009b8eb0`; the 12-byte request walker
is `0x00889de0`. The body's list also prepares the two children, so its
models and region resources must not all become one body's data.
The on-disk resource flag words need CRT fixups: body initializer
`0x0135a120..0x0135a6e0` sets binding on indices 0, 2..39 and 56;
child 00 initializer `0x0135b8a0..0x0135badc` on 0..25 and 32;
child 01 initializer `0x0135c440..0x0135c602` on all 26. The appended
collision requests use `0x01371900` and `0x013716b0`. Binding entries
apply `(flags & ~4) | 3`; preload-only entries `(flags & ~5) | 2`.
The reader preserves upper bits. Effective binding counts, including
appended requests, are 41/28/27. Bit 0 gates `0x008892b0` binding, so
using the file's zero words directly would silently omit bound resources.

Body action 0x3a is `cEm5200ActFall` variant 1, **not** `cEm5200ActDie`.
Fall entry/update (`0x009ad030`/`0x009ad270`) start motion 0x2b7 and wait
for the source's contact/displacement condition before requesting action
0x57. Treating 0x3a as an immediate standard death motion skips this path.
Actions 0x52..0x58 use `cEm5200ActDie` variants 0..6. Entry/update
`0x009aec20`/`0x009aef20` drive motions 0x239/0x23b/0x23c/0x2b8 and a
180-unit cleanup timer. The timer runs after accepted death and must never
be used to advance the prologue by itself.

Body damage callback `0x009a44f0` scales HitInfo +0x78 only when actor
+0x7406 is zero and HitInfo +0x68 equals 1. The multiplier comes from
actor +0x7268 while child 00 is alive, or +0x726c otherwise. The guard's
HitInfo +0x68 is the resolved pool **ID**: `0x0076db10` writes it from
the resolved region's +4, after mapping raw HitInfo +0x6c via `0x00770f00`.
`0x009a0918` copies `charparam/em/em5200` into actor +0x7260 through
`0x00756990`. Reflection `0x0074ec30` identifies +8/+0xc as lion-head-only
magick bonuses (two heads / one head), with labels at `0x0158eb70` and
`0x0158ebc8`. `BodyDamageBonus` loads both values from that resource and
implements the guarded post-mitigation modifier. A synthetic regression
covers liveness, suppression and pool IDs, preserving every other hit field;
the installed constructor loads it only for the body. The combat-pass hook
is now available through the configured combat pass. Its child state remains
an explicit actor observation: vf117 `0x004c7590` reads byte +0x5bd0, not HP.
Child-00 vf34 (write at `0x009b18eb`) can initialize
that marker for an already-empty pool. Combat death calls common vf72
`0x00aab3b0`: its vtable +0x1dc dispatch at `0x00aab4c0` invokes vf119
`0x00ab37d0`, whose first write sets +0x5bd0 to one. The persistent-kill
call at `0x00aab4e4` follows. Body and child-00 use this same vf119.
Vf118 `0x00ab3790` separately tests marker != 1 and main HP <= 0, making
pending HP depletion and accepted death observably distinct. The host must
apply accepted death synchronously after each local/linked result before
later modifiers; the immediate policy observer provides this ordering,
while the HP snapshot alone cannot establish acceptance.
Body post-hit `0x009a4560` maps
HitInfo +0x6c values 1/2 to life pool 1, 8..11 to pools 2..5, and other
values to pool 0 for visual damage. Child post-hit callbacks
`0x009b42f0`/`0x009bcb80` select pool 1 for hit kinds 4/6 respectively.
These are post-hit visual consumers; they alone do not establish the full
damage-routing algorithm.

Initialization selects region **kind 0** in all three units: EAX is zero
at calls `0x009a0bd1`, `0x009b1118`, `0x009b909c` to `0x00770910`.
Installed files have 5/4/2 region kinds for body/00/01; kind zero contains
6/2/2 life pools respectively. Pool IDs must remain separate from vector
indices. The three 12-entry hit-region maps at `0x014fe1e0`, `0x014fde28`,
`0x014fda78` are now read at runtime. Initializers write them to collision
+0xcf4 and set +0xd78 to 12. `0x00770f00` looks up the mapped **ID** in
the selected pool list.

Child 00 initialization at `0x009b116a` sets child +0x1da8 (collision
+0xcc8) to parent +0x10e0. Collision pass `0x0076b7e0` applies the local
hit with `0x0076db10(..., false)`, then traverses that link and applies
the same hit with `0x0076db10(..., true)`. The linked pass skips damage
valuation/mitigation, optionally remaps the region through +0xd34/+0xd38,
applies main-pool break/status effects, and applies HP only when its
resolved region has attribute 8. It must not call `resolve_enemy_hit`
again and mitigate twice. Child 01's initialization does not install this
link; its lifecycle and any later link writer still need tracing. Existing
within-actor region propagation is not this cross-actor relay.

The mirrored branch is now implemented as a separate combat helper, fed a
`ValuedEnemyHit` retained by `resolve_enemy_hit_valued`. The legacy
`resolve_enemy_hit` API delegates to that function and retains its old
return type. The helper receives a host-resolved optional destination pool,
the existing source event, and the destination status rates. It applies
break and status to the main pool even when the mapped pool is absent,
resolves frozen cancellation, then conditionally applies the carried total
to main HP. It never mutates the mapped non-main pool or calls mitigation.
As in the ordinary hit pass, event total is valued damage, not observed HP
loss after no-HP flags, floors or clamping. Missing LINK_HP reports zero;
present LINK_HP preserves the carried total even when HP loss is prevented.
The additive `hit_pass_configured` now collects normal contacts and accepts
explicit per-owner `EnemyCollisionConfig`: installed hit-to-pool IDs, optional
mirror region remap, an ordered complete linked-owner chain, and two status
rates. It retains the valued local payload and immediately applies linked
hits before the next local victim. Destination defense, HP scaling and the
post-mitigation callback are not run a second time. The host still supplies
the actual child/body identities and chain; this emits no story facts.

`resolve_enemy_hit_valued_with` supplies a hook immediately after common
mitigation, passing the resolved pool ID. The configured host hook also sees
current `CombatOwnerState::has_hp`, refreshed after earlier local/linked hits.
That snapshot is an HP observation, **not** the original actor death marker.
The regression policy uses HP to exercise ordering only; real body bonus
policy must use the actor +0x5bd0 observation. Legacy public wrappers retain
their previous signatures and behavior.

`EnemyHitPolicy` and `hit_pass_configured_with_policy` add `after_hit` to the
post-mitigation hook. Execution is local hit, local observer, each linked hit
and its observer, then the next local victim. Both callbacks remain exclusive
to configured enemies. Actor state is owned by the caller policy, allowing
the observer to run the sourced death selector and retain accepted markers
before a later modifier. A regression gives child 00 identical zero HP in
two runs but blocks priority acceptance in one: the later body's loaded-rule
modifier sees the different accepted state. This is a synthetic policy test;
the host's action, placement and persistence integration is still absent.

Configured validation rejects missing/duplicate owner bindings, invalid
chains and unmapped ordinary hurt regions before changing HP, history or RNG.
Ordinary missing mapping is deliberately unsupported: `0x0076db10` skips
local valuation and may retain an earlier HitInfo +0x7c value that `MergedHit`
does not represent. No guessed total or main-pool fallback is used. Missing
**mirrored** mapping retains sourced main status/break processing without HP.
Synthetic regressions exercise nonidentity maps/remaps, three simultaneous
owners, immediate event order, hook timing, legacy equivalence, and atomic
rejection. This does not establish a live chimera encounter.

`cEm5200_01ActDie` entry `0x009c0d40`, variant 10, has a separate prologue
side effect: at `0x009c0ead..0x009c0ef0` it tests its parent's packed
placement for stage 380/group 10/index 0, then passes GSF **705** (`0x2c1`)
to `0x00401280`. This is child death, not the body's persistent-kill bit.
The same variant calls `0x009b9af0` to create a detached child-01 unit in
mode 1 and copies its impact pose. Neither fact authorizes treating that
remnant as a new layout enemy. The flag side effect is traced only, not yet
emitted by the host.

Child think roots depend on the parent's +0x5c14 and +0x5c18 == 2:
child 00 uses 0x016fe6e8 or 0x016ff368; child 01 uses 0x016f8ee8 or
0x016f9a28. Body array 0x016cd234 and fallback 0x0170f060 are selected
by `0x009a0880`. Read records through the shared table walker.
`ThinkInitialization` now reads those selected roots with the existing
`Tables::read_roots` extension. Explicit controller input represents body
+0x5c14/+0x5c18; it is not inferred from a stage or unit name. When enabled,
the body locks the selected current table but still ends initialization with
default/base reset to `0x0170f060`. Children retain their selected table as
current/default/base. The initializer also applies the sourced range-scale
gate to a fresh shared manager; it supplies no targets, think flags or unit
action driver. Installed regressions cover disabled and controller 0/1/2
selection, child special-root gates, manager fallback reset and scale guards.

## Implemented and tested

### Wait action handoff (source traced, not implemented)

The three Wait actions do not wait for motion completion to become
replaceable. Body entry `0x009a6900` clears actor +0x2530/+0x2540 vectors
and binds motion 0 with blend 5 and speed 1. Body update `0x009a69d0`
sets action +0x50 to 1 immediately and, when the owner exists, ORs bit 0
into both actor +0x2360 and +0x2370. The child Wait updates resolve through
their installed vtables to `0x009c5b10`, which only sets action +0x50 to 1.
All three vf07 slots resolve to the no-op `0x005f35c0`.

Child 00 entry `0x009b4a40` binds motion 0 with blend 5. Child 01 entry
`0x009bda90` uses its own actor +0x72e4: values 0/4/5/6 bind motion 0,
1/8 bind 0xc9, 2/7 bind 0xca, each with blend 10 and speed 1. Other
values do not issue a motion request; do not substitute motion 0. The
body's +0x72e4 is a child pointer, so these same offsets have different
meanings in the different owning classes.

Base entry cannot be omitted from an eventual driver. `0x009a67b0`,
`0x009b4920`, and `0x009bd970` call `cActionBase::vf05` (`0x007f1890`),
reset action state, and bind a checked class owner. The common entry
clears action completion and actor +0x2370/+0x2374; the enemy-class branch
also resets +0x273c and restores/remaps the priority mask from +0x2e38 to
+0x2e34 (0x40 to 0x100000 and 0x400 to 0x4000000). Body base entry further
resets its +0x2578/+0x257c values and writes +0x2574 = 0x100. Child base
entries do not perform that body-specific reset. No new actor driver or
completion behavior is claimed by this source handoff.

Isolated `ddenemyrt::chimera` source primitives select body/child death and
parent reactions, preserving the masks and early-return behavior. Five
regressions pass: all arbitration bits in both masks, missing damage and
dead-state rejection, body motion overrides, child-01 guards, and the two
parent reaction families (including zero/negative/missing impact side).
The installed catalogue regression resolves every action DTI,
checks the sourced death/fall variants, locates all referenced model,
motion and object-collision resources, and parses all requested region files.
It also verifies all mapped pool IDs, shuffled pool order and missing-region
rejection. A CRT regression pins binding exceptions and upper-bit preservation.
Five linked-hit regressions cover the unchanged ordinary resolver, distinct
carried total, no repeated mitigation, untouched mapped pool, missing mapping,
attribute-8 gate, no-death floor, break guards, physical/magick status rates,
frozen cancellation and attacker identities 0, 1 and 37. These are synthetic
contract checks; they do not constitute a two-player chimera fight.
The module is exported through the agreed `lib.rs` hook. `ChimeraData::load`
constructs each unit's skeleton, ordered motion banks, bound collision files
and common/region combat values from its installed resource bindings.
Kind zero is selected by all three initializers (0x009a0bd1, 0x009b1118,
0x009b909c). The common-stat loader accepts explicit child resource paths
without widening the `EnemyId` type. Installed regressions check legacy
common-stat equivalence, kind-zero pools, missing paths, bank ordering and
independent owner pools. Scale and collision rates/flags remain explicit
caller inputs; this constructor does not claim an initialization policy.
No game observation was performed.

Still open: spawn binding and initialization flags; complete damage routing;
child anatomical mapping; action execution, root movement and collision;
think driver and N-Arisen fight integration; story/live acceptance.
Next action: connect the sourced child-00 linked-hit contract to the host's
combat pass before binding combat death to the stage-380 placement.
