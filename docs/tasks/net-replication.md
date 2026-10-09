# net-replication: goblins, HP and looks over LAN

- **Owner:** claude-10
- **Branch:** `task/net-replication`
- **Board area:** `net/lan`
- **Status:** open

## Goal

After play-from-save, co-op guests see no goblins and no HP, and each
side sees the other's Arisen as a stand-in. Replicate, per ADR 0002's
rules (snapshots plus reliable events):
- enemies (`ddenemyrt` goblins: position, state, motion, HP, death);
- pawns (already seat 4: their state and HP);
- HP and stamina per seat;
- each Arisen's look: the edit and equipment data from its save, sent
  once at join.

Also let a guest join from a different save spot: the host sends its
stage, cell and position at join, and the guest loads that world.

Keep the protocol versioned (join check).

## Scope

- `crates/ddnet/**`, `crates/ddhost/**`: additive
- `crates/ddview/src/net.rs`
- `docs/adr/0002-game-host.md`

## Done when

- Loopback tests show the guest's mirror of goblin HP, death and the
  other Arisen's look data matching the host.
- A two-window run shows both dressed Arisen and the goblin on both
  sides.
- Use timeouts on GUI runs, and post status every 30 min.
- Fill in Results and report.

## Results

(filled in by the agent)
