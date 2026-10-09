# m3-lan: the LAN co-op network layer

- **Owner:** claude-10
- **Branch:** `task/m3-lan`
- **Board area:** `net/lan`
- **Status:** review

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

claude-10, 2026-10-09, branch `task/m3-lan`. Commits:
- 97f7f19 ddnet;
- 8c9e451 ddview WIP;
- 5e85234 demo input.

### `crates/ddnet` (new, headless)

ADR 0002 section 8, built as decided. The implementation notes are in
ADR section 8.9.

- **`link`:**
  - header with connection id, sequence, acks and 32 ack bits;
  - a reliable ordered channel, resent until acknowledged;
  - an unreliable newest-wins payload;
  - round trip measured from acks.
- **`transport`:**
  - non-blocking UDP;
  - `Lossy`, a seeded wrapper adding latency, jitter and loss.
- **`proto`:**
  - Hello, Welcome, Refuse, Joined, Left, Story events, Bye;
  - redundant inputs: the newest 8 commands stamped with host frames,
    plus the snapshot ack;
  - snapshots.
- **`body`:** the public `ddsim::Player` fields in ten field groups,
  delta-encoded against the client's acked snapshot (an unchanged body
  costs 3 bytes).
- **`server`:**
  - `ddhost::Host` behind the link;
  - join checks: protocol, exe build, data fingerprint (`.arc` paths and
    sizes), roster full or profile already seated;
  - per-seat command queues, repeating the last command when one is
    missing;
  - 2 s timeout and Bye;
  - 64 frames of baselines;
  - story requests sent as reliable events.
- **`client`:**
  - commands kept 1-4 frames ahead of the host by lead control;
  - its own Arisen predicted with ddsim and replayed on each snapshot;
  - the others interpolated 3 frames (100 ms) behind.

### Tests (`cargo test -p ddnet -p ddhost`: all pass)

`tests/loopback.rs`, real UDP on 127.0.0.1:
- **Prediction under latency and loss** (40 ms ± 20 ms each way, 10% loss
  both ways).
  - The client's Arisen moved 1,228 cm on the host, under the host's
    authority.
  - Prediction error at most 5.7 cm, mean 0.4 cm, over 264 snapshots.
  - 10 frames of 270 ran on a repeated command.
  - After 3 s without input, the client's newest snapshot hashes equal to
    the host's state for that frame, and the prediction equals the host's
    body, apart from the motion frame of the frames it runs ahead.
  - Identical over three runs.
- **Join checks:** wrong data, a full session and the same profile twice
  are refused with a reason; Bye frees the seat.
- **Timeout:** a silent client is dropped after 2 s.

### ddview

- **Flags:** `--host <port>`, `--join <addr[:port]>` (default port
  47810), `--profile <id>`; both imply `--play`. `--demo` drives
  unattended input.
- **`net.rs`:**
  - steps the server or client at 30 Hz;
  - the host shows the host's simulation; the client shows its predicted
    Arisen and the others interpolated;
  - characters exist for both seats and are hidden until their seat is
    taken.
- **`play.rs`:** split into `orbit`, `p1_input` and `show`, and steps
  aside during a network session. `ddhost` `Ground` boxes are now `Send`,
  because the session lives in a Bevy resource.
- **Two instances on one PC:**
  - the client joined as seat 1 within a second;
  - both logs show both Arisen moving, the client's view of the host
    slightly behind as designed;
  - both sides announced exe build 1513180209 and the same data
    fingerprint;
  - screenshots were taken, but in that run each camera showed only its
    own Arisen (the two had walked apart).

### Open

- **Fall-through in the demo.** In one two-window run, seat 1 sank below
  the ground and fell without end (y to -181,000).
  - Diagnosis: not the network. The host's own simulation did it, with
    plain demo commands; seat 0 behaved the same way at other spots.
  - The ground exists along the whole path. A probe of
    `ddworld::Terrain` around the spot (stage 100, cells around
    58m50n, x 131-462, z 83545-83782) gives heights of 40,939-41,040,
    single-layer, with nothing below 40,000.
  - The body got below that surface while walking and jumping across
    uneven ground (y 38,393 at x 227 where the ground is at 41,040), then
    found nothing underneath.
  - Both seats share one collision set (5x5 cells), and the guest spawns
    1.5 m from the host, so loading cells only around seat 0 is not the
    cause here.
  - This is a ddsim landing/tunnelling case for its owner (claude-1),
    with these coordinates.
  - The demo now walks in small circles without jumping. A screenshot run
    with both Arisen in one frame is still to do.
- **Not built:**
  - LAN discovery (the 47811 broadcast);
  - fragmentation;
  - the views do not yet perform story events (sent as text);
  - pawns and NPCs are not replicated yet (only the Arisen exist in
    ddsim).
- **Live fact for FRAME.md (claude-4, #1059):** the original runs 7
  worker threads (`sMain+0x98`, `mJobThreadNum` 7), so a line's jobs
  really run concurrently. Not added to FRAME.md (outside this brief's
  claim).
