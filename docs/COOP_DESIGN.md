# Co-op design

Decisions from the maintainer (2026-10-08). These shape the architecture
from the first line of code, because retrofitting multiplayer into a
single-player design is the expensive mistake to avoid.

## Decided

- **Two full players**, Baldur's Gate 3 style: each has their own Arisen,
  character, inventory, skills, vocation and camera. Not a puppet and not a
  pawn.
- **Party composition:** P1 Arisen + P2 Arisen + one shared main pawn + one
  hired pawn. The original party is four slots (Arisen, main pawn, two
  hired pawns), so P2 takes the second hired-pawn slot.
- **The main pawn is shared.** Either player can give it commands.
- Pawn orders issued in one host frame are delivered in ascending seat
  order. Each pawn's mailbox retains the last authorized order, so the
  highest valid seat wins a simultaneous shared-main-pawn tie. Hired and
  support pawns obey their owning seat. This is a co-op decision; the
  original has one commanding Arisen. Held input uploads do not repeat an
  order, and conflicting directions on one seat are suppressed according
  to the native sender (both Help directions together remain valid).
- **NPC interaction lock:** while one player is talking to, trading with or
  otherwise interacting with an NPC, that NPC is locked for the other player.
- **LAN play**, each player on their own PC with their own copy of the game
  data.

## Architecture consequences

- **Host-authoritative simulation.** One machine (the host) runs the world
  simulation. The other client sends inputs and receives replicated state.
  Single-player is the same code path with one local player and no remote
  client.
- **No global player.** Systems that the original ties to "the player"
  (camera, HUD, quest log, inventory, lock-on, enemy targeting and aggro,
  pawn knowledge, grab and climb, carry) take a player ID.
- **The world's shared state is host-owned:** time of day, quest state,
  NPC schedules, enemy AI, Riftstone pawn pool.

## The host and the protocol

[ADR 0002](adr/0002-game-host.md) (proposed) turns the decisions above
into an architecture. [FRAME.md](FRAME.md) is the original's frame order
it follows.

- **`ddhost::Host`** owns the session: the seats, the party, `ddsim`,
  `ddstory`, the world adapter and the interaction locks. It steps them
  at a fixed frame (1/30 s by default) in the traced order.
- **Identities.**
  - `PlayerId` is the seat in this session (0 = the host).
  - `ProfileId` is the Arisen across sessions and saves.
- **Party.** Host Arisen, main pawn, one hired pawn, guest Arisen in the
  last hired-pawn slot.
- **The interaction lock** is one host-owned table (NPC id → seat).
  Systems acquire through the host; leaving releases.
- **Saves** are a chunked container: shared chunks plus per-profile
  chunks. The host saves everything; each guest takes home its own
  profile's chunks.
- **LAN.**
  - UDP, with input upload (held states, redundant).
  - Snapshots, delta-encoded against acked baselines, plus reliable
    events.
  - Clients predict their own Arisen with ddsim, interpolate everyone
    else, and run camera, HUD and cutscenes locally.

## Open questions (to decide as the systems are reached)

- Who "owns" the story when the original assumes one Arisen: dialogue
  choices, quest acceptance, the Beloved (affinity) system, the ending.
- Cutscenes: do both players watch together, and whose character appears?
- Main-pawn learning (inclinations, knowledge) when two Arisen command it.
- Fast travel and Portcrystals: does travel pull both players?
- Saves: host-only, or each player keeps their own character? *Proposed in ADR 0002: the host saves all; each player keeps their own character's chunks.*
- Pause and menus in a shared world. *Proposed in ADR 0002: personal menus freeze nothing; events freeze the shared world for both, as the original's group bit 57 does (FRAME.md).*
- Pawn rental between the two players' Riftstones.
