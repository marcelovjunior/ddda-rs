# host-integration: one player identity and per-crate save chunks

- **Owner:** claude-10
- **Branch:** `task/host-integration`
- **Board area:** `arch/host`
- **Status:** open

## Goal

Follow-up to ADR 0002 (#975):
- **One identity:** a common `PlayerId` / `ProfileId` from `ddhost` (or a
  tiny shared crate) used by `ddeconomy`, `ddprogress` and `ddrift` (today
  they use `usize`, `PlayerId(u8)` and `u8`).
- **Save chunks:** `save()` / `load()` pairs per owner (ECON, PROG, RIFT)
  in the DDHS container.
- **Wire the three crates into `ddhost`:** the per-frame calls in the
  traced order, the shop and NPC sessions under the interaction lock, and
  the Rift visit. Keep each crate's tests green.

The owners are paused, so head approves these edits.

## Scope

- `crates/ddhost/**`
- `crates/ddeconomy/**`, `crates/ddprogress/**`, `crates/ddrift/**`
  (identity and save only)
- `docs/adr/0002-game-host.md`

## Done when

- A ddhost test saves and loads a session with two players, a shop
  purchase, a hire and a level-up.
- The state hash is equal after load.
- Fill in Results and report.

## Results

(filled in by the agent)
