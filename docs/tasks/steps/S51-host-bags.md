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


## Results (codex-71, source checkpoint; UNTESTED)

- Reuses `Economy.inventory` as the sole opening bag owner. Host tick gives
  sourced first-vocation and runtime quest rewards; NPC 1058 already follows
  the native host talk path, so the viewer's duplicate grant is removed.
- ECON v2 adds opening phase and per-profile delivery identities; v1 bags load
  without manufactured items. Candidate bags, sizes, item identities and
  delivery metadata are checked before inventory-related load mutates the host.
- Host `ItemRequest` validates seat, action lock, bag ownership, expected slot
  and live recovery recipients. Opening quick use and demo heal call this path.
  The standalone `--play` imported-save adapter now lives in headless ddhost;
  the viewer wrapper has no new-game or reward mutation.
- Regression sources cover installed Reynard dialogue -> 1058 -> checkpoint
  restore -> native curative recovery, profile-seat remapping, failed/stale/
  foreign requests without spending, replayed reward/kit callbacks after load,
  legacy ECON v1 and malformed/mixed-version metadata.
- **Not tested yet:** Cargo is held behind codex-66's quiet benchmark lease.
  No original-game comparison, GPU run or Ready claim. LAN inventory request
  transport remains outside this brief; guests cannot consume a viewer bag.
  Hired-pawn recovery augment conversion is unresolved and labelled in code.
- Next: full ddeconomy/ddhost/ddnet checks under the shared core lease, followed
  by full viewer suite and release build under the M5 lease; resume S38 on this
  host request API only after accepted source tests.
