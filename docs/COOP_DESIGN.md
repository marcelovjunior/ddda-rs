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

## Open questions (to decide as the systems are reached)

- Who "owns" the story when the original assumes one Arisen: dialogue
  choices, quest acceptance, the Beloved (affinity) system, the ending.
- Cutscenes: do both players watch together, and whose character appears?
- Main-pawn learning (inclinations, knowledge) when two Arisen command it.
- Fast travel and Portcrystals: does travel pull both players?
- Saves: host-only, or each player keeps their own character?
- Pause and menus in a shared world.
- Pawn rental between the two players' Riftstones.
