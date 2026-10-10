# S55: Seats and profiles: leave/rejoin without inheriting another player's state

- **Branch:** `step/s55-profile-lifecycle`
- **Files:** `crates/ddhost` (roster join/leave, economy/progression keys),
  `crates/ddeconomy`, `crates/ddprogress`, `crates/ddnet`; tests (headless)
- **Kind:** one feature (co-op correctness)

## Context (codex-71 #6831, S51 Results)

`Host::leave` keeps seat-keyed wallets, bags and grant metadata;
`Host::join` can reuse the numeric seat for a different `ProfileId`, which
then sees the previous player's wallet/bag/progress. Saves already file
per-seat parts by profile; runtime state should follow the profile too.
Related: S51's late-join bag policy, S44's guest late start (a guest joining
during a quest camera sees stale playback).

## Do

1. Key per-player runtime state (economy wallet/bags/deliveries,
   progression character, hired-by, fight seat data) by `ProfileId`, with
   seats only as the current mapping; on leave, keep the profile's state
   (for rejoin) but detach it from the seat; on join, attach the joining
   profile's own state or create it (S51 policy).
2. LAN: a guest joining mid-camera gets the current camera state, not an
   exhausted stale one (coordinate with S44's protocol).
3. Tests: A leaves, B joins into the same seat and sees only B's state; A
   rejoins later and gets A's state back; save/load round trip.

## Done when

No seat reuse leaks state between profiles, rejoin restores a profile's own
state, and LAN late join is coherent, with tests.

## Results

- Implemented by codex-75 on `step/s55-profile-lifecycle`, from
  committed main `5858ed0`, with reviewed main `7ea18c9` integrated in
  branch-only merge `3c0a548`. Validated Rust source is `3e804bd`.
  This is project co-op lifecycle policy, not
  a native multiplayer rule. Head approved runtime profile parking and
  retained the existing fight-save limits in board #7130.
- `Host` parks unseated state by `ProfileId`; active system APIs remain
  seat views. Departure detaches wallet, Arisen bag/storage, first-choice
  and quest-delivery protection, and the progression character, with
  remaining character indices repaired. Interaction/input/order state
  is released instead of inherited by a reused seat.
- Runtime Arisen HP/status/endurance and hirer-owned pawn brain/body/fighter
  are parked. All old-seat projectile families and permanent/current-frame
  contact histories, including the dragon, are cancelled. Reattachment
  changes fighter/AI owner references and starts a fresh simulation action.
  New or saved profiles receive their own reconstructed combat actor,
  independent of the shared `apply_job_kit` early return. Parked runtime
  fighters/pawns/endurance participate in `state_hash`.
- Pawn bodies left behind by the older Rift-release adapter also lose
  their old owner's seat on departure; they cannot become a replacement
  profile's hire or continue under its AI owner id.
- A fresh profile also receives its own level-1 progression record when
  tables/current vocation are available. Arisen progression members are
  aligned to the current roster's party slots after leave/join/load; a
  surviving or swapped-seat Arisen cannot occupy the next profile's slot.
- Hirers are persistent profiles. An unseated hirer's pawn is detached from
  the active party, rather than silently transferred to seat 0 or a new
  occupant. HOST version 2 stores profile hirers and reads version 1's
  seat hirers through its saved roster. ECON/PROG/PLCE parts retain unseated
  profiles; an independently saved Arisen character remains loadable even
  when absent from shared PROG character order.
- Rejoin uses the current-stage spawn passed by the host; PLCE remains
  profile-owned and restores when the profile is seated during load.
  HP/status/endurance remain outside the durable save format, as head
  required. No new fight chunk or change to vocation selection was made.
- LAN admission and heartbeat send a freshly built current snapshot,
  with S44's current camera generation/elapsed clock or explicit `None`.
  They do not replay a remembered quest-camera start. No protocol change.
- The shared camera lock matches S40/S44's production `OpeningWorld`:
  `player_actions_blocked(_seat)` ignores the seat and reads the one
  authored camera's option flags. Applying it to every occupied seat is
  explicit co-op policy, not original multiplayer evidence. A different
  `StoryWorld` with individual seat restrictions would need a separate
  protocol policy; this field does not claim to represent those rules.
- Missing ECON is tracked separately from an authoritative empty bag;
  PLCE/PROG-only partial saves cannot replace a fresh/imported bag with
  defaults. Unrecognized profile chunk tags do not create profile state.
  Normal S51 late-join initialization still runs when no bag exists.
- **Tested:** full `cargo test -p ddeconomy -p ddprogress -p ddhost -p ddnet`
  completed with actual exit 0: **164 passed, 0 ignored**, 294.17 seconds
  total (33.38 seconds compilation). All eight new source-present test
  names were confirmed in the log: six host integration tests, one parked
  health/status hash test, and one real UDP late-camera test. They cover
  seat recycling, parked save roundtrip, malformed parked ECON/PROG/PLCE
  rejection before mutation, HOST v1 migration, partial/unknown profile
  chunks, installed progression/reward/fight/hirer restoration and save
  reconstruction, and installed camera 12's current clock/exhaustion
  without advancing the host. The install was present and explicitly
  selected with `DDDA_NATIVE_PC`; installed fixtures executed.
- **Downstream tested:** full `cargo test --release -p ddview` completed
  with actual exit 0: **103 passed, 2 ignored**, 39.04 seconds total
  (25.26 seconds compilation, 13.13 seconds tests). Then
  `cargo build --release -p ddview` completed with actual exit 0 in
  40.34 seconds. Existing viewer warnings about `Args` visibility and
  `forget_actor` remain. No viewer source changes were made by S55.
- Validation used the normal coordinated `build/core` and `build/m5`
  leases and existing shared targets; both were released after actual
  completion. Private logs are under `E:/ddda-re/notes/steps/S55`, named
  `full-core-3e804bd575dbbf0e6133b77b1e822075279a999c.log`,
  `viewer-3e804bd575dbbf0e6133b77b1e822075279a999c.log` and
  `build-3e804bd575dbbf0e6133b77b1e822075279a999c.log`.
  The first compile on `b8d923f` failed on the new fixture's borrowed
  path; `3e804bd` fixed it before the successful full validation.
- The private release executable was frozen only after actual build
  success as `ddview-s55-profile-3e804bd575dbbf0e6133b77b1e822075279a999c.exe`
  in that evidence directory, SHA-256
  `B8C3D7897EA86F5B10383DD787C11976AFB7DA88CC58AE78E1E14C3D49C66639`.
- **Comparison limits:** headless tests consume installed game data, but
  no live GUI or original-game multiplayer comparison is claimed. Runtime
  HP/status/endurance parking is tested; durable HP/status/endurance
  remains an explicit save gap required by head #7130. Vocation kits
  outside the existing Fighter/Strider/Mage implementation remain a
  separate gap. Shared-camera locking is the project policy described
  above. No game installation files or original-game process were changed.
- Next action: codex-1/head review and integrate this branch. Hand off
  claimed host hooks to S53 after review; S53 must keep terminal-death
  state profile-owned and observe damage after `Fight::hits`, per head
  #7481. Head alone updates milestones and merges into main.
