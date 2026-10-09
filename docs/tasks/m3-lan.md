# m3-lan: the LAN co-op network layer

- **Owner:** claude-10
- **Branch:** `task/m3-lan`
- **Board area:** `net/lan`
- **Status:** open

## Goal

M3 (MILESTONES.md): host and client on LAN, two Arisen in the world,
following ADR 0002's protocol:
- UDP with a reliable and an unreliable channel;
- join checks the exe build and the data;
- clients upload held-button inputs, sent redundantly;
- the host sends delta snapshots plus reliable events;
- the client predicts its own Arisen and corrects to the host;
- other players and NPCs are shown 100 ms behind.

Build it as a headless crate `ddnet` over `ddhost`, plus `ddview`
flags: `--host <port>` and `--join <addr>`.

## Scope

- `crates/ddnet/**` (new)
- `crates/ddhost/**`
- `crates/ddview/src/net.rs` (new)
- `crates/ddview/src/main.rs`: flags only (claude-3 also edits it: claim
  the lines and coordinate)
- `crates/ddview/src/play.rs`: input routing (unclaimed now)
- `docs/adr/0002-game-host.md`, `docs/COOP_DESIGN.md`

## Done when

- A headless test runs host and client over loopback UDP, with simulated
  latency and loss.
- The client's Arisen moves under the host's authority, the client's
  prediction error stays bounded, and the state hashes agree after the
  input stops.
- Two `ddview` instances on one PC can join and see each other move (a
  screenshot from each).
- Fill in Results and report.

## Results

(filled in by the agent)
