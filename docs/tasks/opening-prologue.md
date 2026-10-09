# opening-prologue: The playable prologue (stage 380)

- **Owner:** codex lane
- **Branch:** `task/opening-prologue`
- **Board area:** `story/prologue`
- **Status:** in progress (headless setup checkpoint; playable route incomplete)

Part of the opening slice. Read `docs/OPENING_SLICE.md` first,
especially its rule: build the system, not the step.

## Goal

Replace the prologue fast-forward with the real prologue: stage 380, the pre-made hero and party, its fights, the `fsm\fix\*` scripts (SceNo 200 -> 900), event st380 ev00 and the prologue end (0x00444b20 -> stage 800). Trace the hero's setup (equipment, vocation and party) and the prologue's enemies and their spawns from the data.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks may control the game, with the safety rules in `docs/CODEX.md`.
Test runs use `--unattended`.

## Scope

- `crates/ddstory/**`
- `crates/ddhost/**`
- `crates/ddview/src/opening.rs` (claim)
- `docs/QUESTS.md` (prologue section)
- this brief's Results

Shared viewer files need a board claim and coordination with the
agent that holds them.

## Done when

`--opening` starts at the new game and plays the prologue through to stage 800, with SceNo and GSF matching the traced sequence at each step. Enemy families that are missing are listed, or built if they're small. Fill in Results and report to `head`.

## Results

### Implemented and tested (codex-27, 2026-10-09)

- `3b9c873`: `ddstory::prologue::party_from_stock` selects the original's
  stock entries 10..13 and replaces their names from NPC ledger entries
  896..899. Source: new-game `0x00445ab0`, disassembly checked at
  `0x00445c58..0x00445d0c` and `0x00445d96..0x0044601e`. Hero overrides
  at `0x00445e16` are gold 37000, Rift Crystals 785 and discipline 1068
  (`cPlayerInfo.mJobPoint` at +0x344; not experience);
  `0x00446020..0x0044608c` adds `rng % 5000 + 1000` gold. The RNG value
  comes from the host. Templates stay reusable for each co-op Arisen;
  the companions are shared session records, not per-player globals.
- `1e2a310`: `story_stock(data, index, language)` reads the requested
  built-in record from `etc\editPattern\romPawnInfo`, names from the
  runtime name/ledger files, and equipment slots from `etc\item\itemList`.
  It applies the existing sourced `0x00796420` edit/equipment overrides.
  `load_party` composes the four prologue records. It does not load the
  entire offline Rift pool. Codex-26 consumes the same reader for Rook.
- Installed-data test: the hero and companions have vocations
  Fighter/Fighter/Strider/Mage and level 20. Parameter/equipment records
  match their installed templates, with names and edit overrides applied;
  Rook's fixed equipment is checked for Japanese and alphabet-name modes.
  No appearance/stat/equipment records were copied into the repository.
- `EncounterFacts` is an explicit `ddstory::Host` adapter, keyed by stage
  and placement. Enemy checks translate `0x005bf3d0`: existence, persistent
  kill record, positive HP and death are distinct. Applied-script
  completion translates `0x005c8460`: an absent NPC is not completion;
  missing party members and recorded enemy kills are special cases.
  Group completion/activation and NPC intel/active units are separate
  facts reported by their owning runtime. Nothing completes on a timer.
- `ddhost::prologue::prepare_prologue(language, random)` installs the
  wallet and shared pawn records for a fresh seated stage-380/Sce100
  session and returns per-seat hero records. All reads and validation
  occur before mutation. It refuses occupied parties or a session that
  has already ticked. One-player sessions retain all three companions;
  the existing co-op policy replaces the final companion with the guest.
  This is separate from the original stock-selection rules. The helper
  is not called by the Cassardis fast-forward path.
- Validation: five new core regressions, three installed-data/order tests
  and three host startup tests passed. Ten existing story unit tests and
  codex-25's three dragon-check regressions also passed after shared API
  integration. This is source/data validation, **not a comparison of a
  playable prologue against the running original**.

### Source and installed-stage findings

- New-game `0x00445be1` sets stage 380 layout flag 0: the write to
  `sGameSys + 0xaee78` is `0xadee8 + 12 * 0x14c`, bit 31. The old
  `Story::new_game` omitted this layout permission. Group 0 also has its
  separate disabled bit set, so this flag alone does not automatically
  spawn it: script-driven placement activation remains necessary.
  Codex-26 owns the coordinated `story.rs` correction.
- `scr\st380\etc\st380_e` has 18 groups and its layouts contain 59
  placements, all in cell `00m00n`. These totals include later-game
  reuse of the stage; they are not simultaneous prologue enemies.
- Groups 0..10 used around the prologue include dragon resources
  `em5800`/`em5802`, goblins `em0100`/`em0101`, harpies `em0601`, and
  chimera `em5200`. Groups 5 and 10 use `em5200` with layout gates 37
  and 5 respectively. Most later groups use scene windows 7000..10000.
- `scr\st380\fsm\fix\set_03` checks `EnemyCheck` flag 1 for enemy
  group 10, index 0: the **persistent kill record**, not just zero HP or
  absence. It subsequently requests event st380 ev00 with scene 900.
  Existing `Story` end-event handling performs the scene-1000/stage-800
  handoff through the previously traced `0x00444b20` path.

### Remaining work and acceptance limits

- The viewer still fast-forwards the prologue. These commits do not
  claim the brief's playable `--opening` acceptance gate.
- Chimera combat is missing and cannot be replaced by a synthetic death.
  Stage 380 streaming/collision, pawn/NPC placement and control, the
  scripted cameras/tutorials, and the event transition still need real
  host/view adapters. The dragon and harpy runtime lanes are separate.
- Party records remain setup descriptions. Active player conversion,
  equipment-derived stat recomputation (`0x00446860`), inventory setup
  (`0x004460d0`) and actor pawn-presence flags still need integration.
- `0x004460d0` additionally creates/equips item ranges 601..608,
  609..616, 617..624 and 625..631 for the original four party slots,
  plus consumables for the hero and a lantern for the first companion.
  Calls to `0x0045ece0` (bag insert then `0x00459100` equipment activation)
  were checked in disassembly. The loader preserves stock equipment;
  active bag/equipment conversion is not claimed by `prepare_prologue`.
- Next action: connect the explicit encounter facts to actual placements
  and kill records, then implement the missing chimera family before
  removing the viewer's fast-forward. Compare original SceNo/GSF and
  captures at each boundary only after those systems run.

Recovery note: E: disconnected during work because of the SATA cable.
After head's resume instruction #2015, this worktree passed
`git fsck --no-dangling`; the interrupted fixture rewrite was restored.
Rust discarded damaged incremental artifacts automatically and rebuilt
the installed-data tests successfully. No game-install writes occurred.
