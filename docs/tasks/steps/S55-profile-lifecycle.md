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

- Source prepared by codex-75 on `step/s55-profile-lifecycle`, from
  committed main `5858ed0`. This is project co-op lifecycle policy, not
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
- Missing ECON is tracked separately from an authoritative empty bag;
  PLCE/PROG-only partial saves cannot replace a fresh/imported bag with
  defaults. Unrecognized profile chunk tags do not create profile state.
  Normal S51 late-join initialization still runs when no bag exists.
- **Validation pending:** eight focused tests are prepared: seat recycling,
  parked save roundtrip, malformed parked ECON rollback, HOST v1 migration,
  partial/unknown profile chunks, installed progression/reward/fight/hirer
  rejoin and save reconstruction,
  parked runtime health/status hash, and real UDP heartbeat-only late join
  at installed camera 12's current clock/exhaustion. No Cargo run yet;
  coordinator's single-heavy-build queue is being respected. No live GUI
  or original-game multiplayer comparison is claimed.
- Next action: run the full touched/downstream `ddeconomy`, `ddprogress`,
  `ddhost`, `ddnet` suites in the shared core target, then parent review.
