# Enemy resource selection: ordinary goblin and em0101

Implemented: bounded, explicit-input resource descriptors and region-type
selection. Tested: synthetic selection boundaries and both installed bundles.
**Not compared against an original-game instance.** Passive observation was
requested from claude-4 (board #472, pointer correction #475); no process writes
or gameplay actions are requested. This is not combat simulation.

## Executable selection chain

Addresses refer to the 2017-12-13 Steam executable identified in METHODOLOGY.md.

- `0x008cd5b0` constructs `uEm0100`, setting byte 1 of `cUnit.mUnitParam`
  (`actor+0x2d`) to 5. `uEm0101::DTI_newInstance`, `0x008eb010`, calls that
  constructor, replaces the byte with 6 and installs the uEm0101 vtable.
  These are internal kinds, not decimal em resource identifiers.
- `uEm0100::vf05`, `0x008cdc80`, selects list `0x018379c8` / 48 records for
  kind 5 and `0x01837c08` / 49 records for kind 6. It then appends the shared
  equipment list `0x018384c8` / 12 records for either kind.
- `0x00889de0` consumes 12-byte records (path pointer, resource DTI pointer,
  flags), creating asynchronous resource requests for each non-null path.
  The Rust descriptors preserve list order and duplicate paths/types.
- `uEm0100::vf33`, `0x008cdd80`, selects the same primary list and calls
  `0x008892b0`. That routine loads enabled records and passes successful
  resources to virtual slot 79. File-image flags are zero before static
  initialization: for example instructions at `0x013475a0` set the first
  ordinary-list flags to 3 and clear localization bit 4. Do not interpret
  uninitialized file-image flags as disabled requests. The Rust API does
  not emulate resource-loading state or these mutable flags.
- `uObjModel::vf79`, `0x00889510`, dispatches by resource DTI; its RST arm
  calls `0x0076da40`, which retains the resource in collision-context+0xcf8.
  `0x00770910` reads this slot, selects the **first cRegionStatus whose mType
  equals the requested type**, and initializes runtime entries from that
  region's element list. Reflection gives mNo at +4 and mType at +8; the
  selector compares +8. At `0x008cdf72` / `0x008cdfd6`, goblin initialization
  passes type 0. It is not a search for mNo 0, nor a total-HP rule.

## Explicit bindings and aliases

| Role | Kind 5 / em0100 | Kind 6 / em0101 |
|---|---|---|
| Common PRP | charparam/em/em0100_cmn | charparam/em/em0101_cmn |
| RST | charparam/em/em0100 | charparam/em/em0101 |
| Adjustment | param/em0100 | param/em0101 |
| Shell list | param/em0100 | **param/em0100** |
| Body collision | collision/em/e01/e0100/e0100_00 | collision/em/e01/e0101/e0101_00 |
| Shared status | param/status/enemy | param/status/enemy |

`0x008cdf5a` / `0x008cdfbe` pass the common PRP path to `0x00aafdb0`.
That helper requests rPropParam, obtains its typed payload through `0x00fd3f20`,
copies it to actor+0x5870 using `0x00749f90`, and copies this block to
actor+0x59b0. If the resource/payload is absent, the existing first block is
still copied; this is **not** a path fallback to a species PRP. No separate
species PRP appears in either chosen list. Later placement/stat overrides
and common/species precedence outside this chain remain unresolved.

`0x008cdf64` / `0x008cdfc8` pass the respective adjust path to `0x00aa8ba0`,
which retains it at actor+0x5b50. Body collision uses `0x0076da00`; the caller
sets its context to actor+0x10e0, so its +0xc5c field is **actor+0x1d3c**.
The RST retained field is consequently actor+0x1dd8. Shared enemy status is
loaded directly in `uEnemy::vf05`, `0x00aa8830`, retained at actor+0x2710.

The shell list's own mCollisionPath is
`collision/em/e01/e0100/e0100` for both kinds. This is a different resource
from the body `_00` collision; do not merge them by stem. Both RST files have
selected region mNo=0, mType=0 and element `(mNo,mId)` pairs `(0,0),(1,1)`.
Those are resource identities only. Which collision nodes/attacks refer to
those IDs, and all HP/DP/BP/damage/stagger/status rules, belong to claude-8.

## Bounded link inventory

`GoblinResources.requests` enumerates every primary-list entry; the executable
list offsets and counts above are the inventory boundary. The two lists share
base model e0100; five motion banks co/at/dm/ex/pl; ragdoll, joint extension,
base collision, two leg IK resources; common/species effects; motion sound and
sound requests; lock-on, shell list, sensor-target group and enemy action
parameters. Equipment requests include e0100_03/e0100_20, their effects,
om10440/om10465, and six e0100/e0101/e0102 equipment materials. Duplicates in
primary and appended lists remain duplicates.

Variant-specific primary entries include six voice resources (three voices,
English/default and Japanese), body collision/common PRP/RST, textures,
sensor, material, equipment models/materials and adjustment. em0101 still uses
em0100 damage textures and equipment normal map. The full typed inventory is
in `crates/ddenemy/src/resources.rs`; no game bytes or raw decompiler exports
are embedded. The shared status request is separate from both lists.

All **48+12** and **49+12** list links plus the shared status resolve in the
installed VFS. All archive occurrences of the selected PRP/RST/adjust/shell/
status resources parse with the existing readers. Missing links would be
reported as absent, not inferred optional. `ResourceRef::alternatives` returns
all VFS locations, retaining ambiguous archive precedence instead of selecting
one. `EnemyData::load` remains the documented convention-based compatibility
API; the new explicit selection API is the verified alternative for kinds 5/6.

## Verification and open gate

`cargo test -p ddenemy --test resources -- --nocapture`: two synthetic tests
and one installation test pass. Synthetic tests distinguish mNo from mType,
first duplicate from later duplicate, absent type from a fallback, both shell
aliases and every unsupported u8 kind. Installation tests assert all selected
links exist and parse each typed core resource occurrence. No fixtures contain
game content. Missing default installation skips; an invalid explicit
DDDA_NATIVE_PC fails.

Unresolved: naturally observed matching instance pointers, stage/spawn-to-kind
selection, post-load PRP overlays, dynamic readiness/failure behavior, full
transitive dependency traversal, archive precedence, and collision-node joins.
Unsupported kinds return None rather than borrowing an ordinary goblin bundle.
Next action: compare a naturally present kind-5/6 instance's collision, RST
and status resource pointers with these descriptors, then trace the spawn
selector independently of combat-rule implementation.

Review update: claude-4's passive check (board #535) found no naturally loaded
uEm0100 instance. Matching-instance comparison is therefore unavailable,
not merely awaiting a response. No enemy was spawned or moved to obtain it.
