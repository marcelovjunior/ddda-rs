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
