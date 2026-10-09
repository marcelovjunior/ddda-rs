# progression: experience, levels, discipline, learning and vocation change

- **Owner:** claude-9
- **Branch:** `task/progression`
- **Board area:** `player/progression`
- **Status:** review

## Goal

Proposals 1 and 3 in `docs/VOCATIONS.md`, as a host-owned runtime in a
new crate `ddprogress`, on top of `ddskill` and the `ddstory` world state.
Per character (each Arisen and each main pawn):
- experience and level-ups that add the current vocation's stat row
  (`ddskill::growth`);
- discipline from experience bands;
- the party share (`0x00443470` / `0x004437d0`), including support pawns'
  Rift Crystals to their owner;
- learning skills and augments (cost and rank);
- equipping active skills and augments;
- vocation change (the gates, the first-time cost per flags 540-554, and
  what is cleared and what carries over).

**Shared world state:** `crates/ddstory` is owned by claude-7. Request API
changes there on the board.

## Scope

- `crates/ddprogress/**`
- `crates/ddskill/**`
- `docs/VOCATIONS.md`

## Done when

- Tests cover a level 1-10 run with a vocation change at 10, learning and
  equipping, and two players sharing experience with a hired pawn.
- Values come from the traced rules and the install data.
- The live checks already made (the experience curve, discipline bands)
  are regression tests.
- Fill in Results and report.

## Results

claude-9, branch `task/progression`. Details: `docs/VOCATIONS.md`,
"Progression runtime (`crates/ddprogress`)".

**Built:** `crates/ddprogress` (host-owned, characters by id, party roles
name their player):
- `Tables::load(vfs, exe)`: level, rank, exp, augment tables,
  `rSkillList`, the exe's skill records, `param\game`, `param\dlc`.
- `Character`: award (0x00782d10 with Weal and discipline gear), `step`
  (one level-up then one rank-up per frame, vf90), augment values
  (0x00783340).
- `learn`: check / learn (0x006fd540, 0x006fdb40, 0x006fdc30, confirm
  0x00702b94, pay 0x006fc758), `equip_augment`, `equip_skill`,
  `active_skills`.
- `change`: check / change (0x00675780, 0x00675e8a, 0x00671b70,
  0x00672c10, 0x00675da0, 0x00673740, 0x00780840), starting items.
- `Progression::award` (0x00443470 / 0x004437d0) by role; `sync_to_world`
  / `sync_from_world` with `ddstory::World` (`gsf` 540..554,
  `player_job`).

**Newly traced:** pawn catch-up `k` = `param\game`[0] = 0.7; discipline
gear: Asura Armor (item 0x361) x `param\dlc`[7] = 2, Ring of Perseverance
(0x47f) x `param\dlc`[0x1c] = 1.5 per ring; learn confirm needs cost <=
discipline (0x00702b94); a vocation is selectable when not current and
affordable (0x00675e8a); the party share's exact precision (x87 then SSE)
and the hired pawn's Rift Crystals from the untruncated share.

**Co-op design (not from the exe):** roles instead of slot numbers (P2's
Arisen in slot 3 earns experience; the main pawn catches up toward its
owner); vocation flags kept per character and mirrored into `World` for
player 0. One player reproduces the original layout exactly.

**Tested:** `cargo test -p ddprogress`: 2 unit + 6 install tests (skip
without the game or exe): level 1-10 run with a vocation change at 10 and
level 11 on the new vocation's row; learning and equipping; Mystic Knight
change keeping shared skills; first choice; pawn limits; two players with
the main pawn and a hired pawn sharing an award; single-player layout and
World mirror; live level-7 Mage values and discipline bands (#722).
`ddskill` unchanged. Clippy clean for `ddprogress`.

**Guessed / open:** refusing duplicate augments; filling active slots from
the weapon sets; how the save marks starting skills; crediting a hired
pawn's Rift Crystals to its owner (Rift code); none of the state-changing
paths has been compared live yet.
