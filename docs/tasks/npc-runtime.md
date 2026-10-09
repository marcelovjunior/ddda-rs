# npc-runtime: NPC schedules, talking and the interaction lock

- **Owner:** claude-6
- **Branch:** `task/npc-runtime`
- **Board area:** `world/npc-runtime`
- **Status:** review

## Goal

A host-owned NPC runtime in a new crate `ddnpcrt`, on `ddnpc` (merged: ledger, MSS/MSL, the Quina chain), `ddstory` (flags, SceNo, clock) and `mtfsm`:
- schedule selection per stage, SceNo and hour (codex-7 traced `0x00bbdaa0` and the GameHour ranges);
- where each NPC stands now;
- talking: picking a conversation by flags and quest state, the lines (`mtgmd`), the results (flags, quests, opening a shop session in `ddeconomy`);
- the co-op interaction lock: one player per NPC; the other player sees the NPC as busy (`docs/COOP_DESIGN.md`).
The open question from claude-7, how a script's `Message` id picks its line, belongs here.

**Shared world state.** `crates/ddstory` is owned by claude-7; request API
changes on the board. The game host is being designed by claude-10
(`ddhost`, `docs/adr/0002-game-host.md`): keep your crate's interface as
inputs → state → requests, so the host can call it each frame. Design for
N players.

## Scope

- `crates/ddnpcrt/**`
- `docs/NPCS.md`
- this brief's Results

## Done when

Tests: Quina's position across a day; talking to one opening NPC picks the right conversation for the flag state and applies its results; a second player is locked out while the first talks; a shopkeeper's talk opens the `ddeconomy` session. Fill in Results and report to `head`.

## Results

Branch `task/npc-runtime` (6496f06 code, then docs). New crate
`crates/ddnpcrt` on `ddai` (FSM core), `ddnpc`, `ddstory::World`, `mtgmd`;
additive `ddhost::npcs` (talk under the interaction lock, talk-to-shop
hand-over, NPC step in the tick, `FrameReport::npc_requests`,
`HostError::Talk/Npc`, `SystemData::npc`). Docs: NPCS.md "Talking" and
"Runtime".

Traced: conversation chooser `0x00bb8cf0` (band, sequence counters, quest /
GSF / affinity conditions, said-memory; reset `0x00475c80`), talk entry
`0x00bb4830`, shop-menu conversations `0x00b9ddc0`, markup tags to
sIdMessage handlers (`0x0044f950`, dispatch `0x00455280`), the menu
handlers' choice tables, `SetInterface` done bit (`0x00533a90`), and the
`Message` order's line selection (`0x005c5bc0` -> `0x004560d0`: `mMesId` is
an MSS conversation number in the quest's message set).

Tests: ddnpcrt 4 synthetic + 3 install (Quina's position across a day:
morning / afternoon / night routes; Quina's talks: band 1 76/13/90, band 2
20/18/21 and 22 once quest 52 flag 94 is set; Caxton's menu opens buy and
plays "Take proper care of that." after); ddhost `--test npcs` 2 (two
players: the second is locked out while the first talks and shops, Buy
opens the ddeconomy session, the lock follows the session and is freed at
the end; Quina's schedule in the host tick). All ddhost tests still pass.

Guessed: SysCondition bit 2 cleared on a state change; NPC graphs' per-cluster
work memory (mFSMAttribute bit 2) not ported in ddai, so sub-machines restart;
moves complete at once in the host (no NPC mover); kinds 1/2 of the shop menu
= after buy / after sell.

Not done: live comparison; quest talk sets and quest/daily arbitration;
locomotion; inn/equipment/storage handlers in the host; talk memory in a save
chunk.
