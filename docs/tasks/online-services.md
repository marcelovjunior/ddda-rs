# online-services: The game's Capcom protocol (sDDCaProto)

- **Owner:** codex lane
- **Branch:** `task/online-services`
- **Board area:** `net/ddca`
- **Status:** open (research; lower priority than the opening route)

## Goal

docs/EXTERNAL.md: the exe connects `sDDCaProto` to
dune.dragonsdogma.com (strings 0x016130EC/0x016130F8; port 12501 per the
dinput8 mod), with a packet set of line check, connect/disconnect,
authentication (header/data/footer) and TUS common/user area
read/write/add (Atvaark's packets.tsv). docs/PAWNS.md traced pawn sharing
through Steam UGC and leaderboards. Trace this second service: its
packet handlers and senders, what each TUS area holds (online Ur-Dragon
HP and generations? rift crystals? ratings? notices?), when the client
talks to it, and what the game does offline. Document it in a new
docs/ONLINE.md and say, for our co-op design, what the host must provide
instead (LAN replacement) for a 1:1 single-player and co-op experience.
Read only: no packets sent to Capcom's servers, no live network traffic.

**Rules.** Trace in the exe and label any guesses (AGENTS.md).

## Scope

`docs/ONLINE.md`, `docs/PAWNS.md` and `docs/COOP_DESIGN.md` notes, this
brief's Results. No code unless a parser is needed.

## Done when

Every packet in packets.tsv is mapped to its sender/handler and the data
it carries, and the offline behaviour is stated. Fill in Results and
report to `head`.

## Results

(filled in by the agent)
