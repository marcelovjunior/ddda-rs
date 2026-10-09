# net-replication: goblins, HP and looks over LAN

- **Owner:** claude-10
- **Branch:** `task/net-replication`
- **Board area:** `net/lan`
- **Status:** review

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

claude-10, branch `task/net-replication`. Commits:
- 5758e70 ddnet protocol 2;
- 7baf1e0 ddview `net.rs`;
- a4cd95f prejoin and ADR.

### Protocol 2 (`crates/ddnet`; ADR 0002 section 8.10)

- **Versioned.** `PROTOCOL` = 2. It also covers head's attack input and
  Attack/Shot states from the m3-lan merge, which had kept version 1. A
  version-1 `Hello` is refused with "protocol 1, host speaks 2".
- **Looks.**
  - `LookData` is the save's `cSAVE_DATA_EDIT` fields plus the equipped
    item numbers.
  - The guest sends its own in `Hello`.
  - The host sets its Arisen's and its pawn's (`Server::set_look`).
  - Every look reaches every guest once, as a reliable `Look { seat }`
    event, at join or when it appears.
  - Each side rebuilds the model from its own install
    (`ddlook::save::edit_look`).
- **Spot.** `Welcome` carries the host's stage, origin cell, position and
  yaw.
- **Snapshots** add:
  - per-seat HP (the Arisen and pawns of `Host::fight`; stamina was
    already in the bodies);
  - every enemy: owner id, place, yaw, action, motion (bank, slot,
    frame), main-pool HP and maximum, death. An unchanged enemy is
    delta-coded to five bytes.
  - The client interpolates enemies 100 ms behind (`Client::enemies`), and
    `snapshot_hash` covers everything.
- **New module** `mirror.rs`, which builds the replicated fight state
  from the host's `Fight`. Also new: `Client::set_ground`.

### Tests (`cargo test -p ddnet -p ddhost`: all pass)

`ddnet/tests/replicate.rs` (install), real UDP, 30 ms ± 10 ms each way,
5% loss both ways:
- the host's Arisen fights a goblin: its HP goes from 1,000 to 0 by
  blows, and it dies;
- the guest's Arisen joins the fight and has HP on the host;
- after settling:
  - the guest's newest snapshot hash equals the host's;
  - its enemies and HP equal the host's `Fight`;
  - it sees the death;
  - both looks (host's to guest, guest's to host) and the spot arrive
    intact.

The m3-lan loopback tests still pass (they now compare `snapshot_hash`).

### ddview (`net.rs`, plus the one approved line in `main.rs`, #1182)

- **Host:**
  - announces its spot and its save's Arisen and pawn looks;
  - dresses a guest's character when the guest's look arrives (a runtime
    `Loader` from net.rs's own Vfs and shader package);
  - gives joined guests HP maxima for the HUD.
- **Guest:**
  - joins after its first frame, its look read from its save (or earlier,
    with the prejoin);
  - dresses the host's Arisen and pawn from their looks;
  - shows the replicated goblins on its `--goblin` views (place, motion,
    death);
  - its HUD shows its HP, its pawn's HP and goblin HP from the host;
  - its own local fight is dropped (the host's is the authority).
- **`--join` prejoins** (`net::prejoin`, at most 5 s): it joins before the
  world loads, takes the host's stage, cell and spawn, and keeps the
  connection answered from a thread until the session takes it over.
- **The release build of ddview succeeds.**

### Not done

- **The two-window run is not done.** It was skipped at head's request,
  for time: there are no screenshots of both dressed Arisen and the goblin
  on both sides. Untested in the GUI: the prejoin, dressing on arrival,
  and the guest's goblin views. The headless test above is the evidence
  for the data path.
- **The pawn needs nothing new.** It already travels as a body (seat 4);
  its HP is now in the snapshots.
- **Limits:**
  - looks are sent once, so equipment changes during play are not
    replicated;
  - enemies other than the fight's goblins are not replicated;
  - the guest's goblin views come from its own `--goblin` count.
