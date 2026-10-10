# S51: Bags and items belong to the host (saved with checkpoints and LAN-safe)

- **Branch:** `step/s51-host-bags`
- **Files:** `crates/ddeconomy` / `crates/ddhost` (authoritative bags, save
  chunk), `crates/ddview/src/items.rs` (becomes a view), tests; headless
- **Kind:** refactor to the original's ownership + save

## Context (codex-1 #6474)

S28 put the opening's new-game bags and quick use in the viewer
(`items::QuickItems`). S46 checkpoints save the host (S48 scripts included)
but not those viewer bags, so a resumed route has empty or re-manufactured
bags; S38's item menu and S39's reward also write viewer state. In the
original the bags are game state saved with the character (`cSAVE_DATA_*`
item sections), and in co-op each Arisen's bag lives with the host.

## Do

1. Move the bags (per Arisen and pawn, with the new-game contents, rewards,
   first-choice items) into host state (ddeconomy already models inventory)
   and route quick use / item menu / quest rewards through host requests.
2. Save/load them in the host save (existing ECON chunk or a new versioned
   one), so S46 checkpoints restore them.
3. Viewer `items.rs` becomes a view of host bags (S38 builds on that).
4. Tests: a checkpoint saved after Reynard's reward restores item 1058 in
   seat 0's bag; quick use from a restored bag heals through the host.

## Done when

Bags are host state, saved with checkpoints, used through host requests,
and no viewer path manufactures items.


## Results (codex-71, 2026-10-10)

Implemented and tested; ready for parent/head review. Source commits `1bc8bc1`
and `fb6a6f9`, plus reviewed-main integration `730db59`; separate shared S44
StoryWorld getter `201ec44` was agreed with head and its owner. Head alone merges.

The opening now uses `Economy.inventory` as its sole bag state. Host tick gives
native first-vocation items and runtime quest rewards. NPC 1058 already follows
the native host talk path, so the viewer's duplicate grant is removed. Opening
quick use and demo heal submit the host `ItemRequest`, which validates seat,
action lock, bag owner, exact expected slot and live recovery recipients before
spending. The standalone `--play` imported-save/recovery adapter now lives in
headless ddhost; the thin viewer wrapper has no new-game or reward mutation.

ECON v2 persists opening phase and per-profile first-choice/reward delivery
identities. ECON v1 loads its actual bags without manufactured phase or items.
Sizes, occupied items, pawn identities, mixed versions and delivery metadata are
validated before bad inventory data can mutate the host. Profile remapping moves
bags and delivery identities together. Head #6662 approved the explicit co-op
late-join policy: initialize a new bag from the saved opening phase and current
vocation, without retroactive quest rewards.

Validation, all actual process exit 0:

- Full `ddeconomy`, `ddhost`, `ddnet` run: **137 passed**, no ignored/failures
  (16 / 104 / 17 respectively), session 87393, freshly compiled ddhost/ddnet.
- Full `ddview --release` suite: **84 passed, 2 existing ignored**, session
  17894; fresh source compile 45.40 s, tests 14.52 s. The five former viewer
  item tests were removed; their meaningful behavior is covered in the host.
- Default release build: session 45128, 45.87 s.
- Parent independent installed item gate: **3 passed**, actual exit 0 (#6803).
- Installed regressions execute real Reynard dialogue -> 1058 -> checkpoint
  restore, host recovery from a restored native curative bag, profile-seat
  swaps, stale/foreign/full-HP requests without spending, replayed delivery
  callbacks after load, late joining, legacy import validation and potion/
  stamina/poison recovery. Rescued-state prerequisites and curative grants in
  tests are explicit installed-data fixtures, not completed-route claims.

Private logs and default pin live in `E:/ddda-re/notes/steps/S51` (never committed).
Pin `ddview-s51-host-bags-730db59.exe` SHA-256
`D0DF991BE1B3F23B65A6A54F65A79AC534477476AB1F583B31B3B1136E22390E`.
No original-game comparison or GPU capture was performed for this refactor.

Limits and next action:

- LAN inventory transport remains outside this head-approved API refactor;
  guests cannot consume a viewer-local bag. S38 will use the same host request.
- Late-join fight kit/global story vocation and hired-pawn recovery augment
  conversion are inherited, explicitly labelled follow-ups.
- Existing live leave/rejoin retains economy by numeric seat, so a different
  profile can inherit retained seat state. Reported #6831 for a dedicated
  profile-bound live join/leave brief, including wallet/bag/delivery metadata;
  checkpoint profile remapping is covered here.
- Head #6740 will regenerate ECON v2 route checkpoints after this lands. Then
  finish S38's full viewer/release gate and real post-reward screenshot; old
  checkpoints are not backfilled with invented items.
