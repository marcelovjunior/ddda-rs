# Statuses: what a status does once it fires

Traced from DDDA.exe (2017-12-13 Steam build, see METHODOLOGY.md) with
read-only Ghidra, and from `param\status\enemy` / `param\status\player`
read with `ddenemy::StatusParam`. The build-up that fires a status is
COMBAT.md section 7. Implemented in `ddcombat::cstatus` (timed statuses)
and hooked into `ddenemyrt`'s fight (`combat::status_pass` and the hit
pass). **Not compared against the running game**; the live checks stay
queued with claude-4 (ORACLE.md).

## 1. The pipeline

1. A hit builds up a region slot (COMBAT.md 7); a slot that reaches its
   threshold fires: `FUN_0076E340` calls the owner's vf74 with the slot.
2. `uEnemy::vf74` (0x00AAB230) maps the slot through `0x015098C0` to a
   **kind**; slots 12..14 (lightning, holy, dark) have no kind and are
   handled in `uCharacterBase::vf74` (0x0084E560): lightning spawns a
   `uShlElemThunder` chain, holy and dark their own shells (*not
   modelled*).
3. `cStatus::add` (0x00837090) adds the kind to the character's status
   list (`cStatus` at actor+0x2698) with the kind's `rStatusParam` entry.
4. Each frame `cStatus::update` (0x00836C70) counts every status down;
   the character's per-kind handlers (vtable slots 137 + kind: vf137
   poison .. vf149 petrify) apply the effects; damage and defence code
   reads the active kinds (bit mask actor+0x26A0) and their parameters.

### `rStatusParam` and `cStatus::add` (0x00837090)

`cStatusParam` fields as `add` reads them: +0x04 `mCategory`, +0x08
`mTimer`, +0x0C `mParam0`, +0x10 `mParam1`, +0x14 `mIsOverwrite`,
+0x18 `mOverwriteNum`. The entry index is the kind (0..0x27). The
enemy table is loaded by `uEnemy::vf05` (0x00AA8B2E, path at
0x015DF1E0) into actor+0x2710 (= `cStatus`+0x78).

- Kinds above 0x27 are refused; so is everything for a player-side owner
  with +0x4CF0 set.
- Kind 0x22 first removes kinds 0..0x11.
- A kind already active is **refreshed**: timer back to `mTimer` x 30
  frames, parameters reloaded. With `mIsOverwrite`, a further **stack**
  is added while fewer than `mOverwriteNum` are active (the downs and
  ups stack to 4).
- A new entry is added only if `0x00837730` allows it, after
  `0x00837810` removed what it displaces. Both read the conflict table
  `0x01509F20` (9 x 9, by category group = low byte): an active group-2
  status (petrify) blocks groups 1, 3, 4, 6; group 7 (kind 0x22) blocks
  1..4; group 8 (kind 13) blocks 0..7. A new status removes the active
  ones of the groups its own row names, and one of the same group and
  non-zero subgroup (byte 1) when the subgroup is 0x1A or equal (bit 16
  gives priority).
- The work gets: timer = `mTimer` x 30 (frames), `mParam0`, `mParam1`,
  speed factors 1, 1.

### `cStatus::update` (0x00836C70)

`timer -= rate x speed0 x speed1` per frame, where `rate` is the global
scaled step (`*(0x018FA524)+0x24 x +0x20 x sMain.mDeltaTime`, 1 per
1/30 s at normal speed). A timer >= 10000 frames never runs out. A
status at <= 0 is removed. The update also caches the products used
below and the added / removed masks. Statuses are therefore timed in
**seconds of `mTimer`**, independent of the owner's own step (slow does
not lengthen them).

## 2. The kinds

`mParam0` / `mParam1` and `mTimer` (seconds) as shipped, enemy table /
player table where they differ.

| Kind | Status | Category | Timer s | p0 | p1 | Effect (address) |
|---:|---|---|---|---|---|---|
| 0 | Poison | 0x1 | 180 / 150 | 5 / 20 | 1 / 6 | HP -p0 every p1 s (enemy vf137 0x00AAFE10: at once, then when the counter, counted down by the unit step, goes below 0; player vf137 0x00B75070: first 3 frames after onset, then every p1 x 30 frames on the scaled step); via 0x00888DA0 on the main pool |
| 1 | Slow | 0x1 | 30 | 0.5 | 20 | the owner's step (`cUnit.mDeltaTime`, +0x14) x p0 every frame (vf138 0x00852670; player 0x00B75320 except in action 0xE4): everything it does runs at half speed |
| 2 | Blind | 0x1 | 30 / 60 | 0 | 0 | effect cue only in vf139 (0x00852730); the consequence is in AI / targeting (*not traced*) |
| 3 | Silence | 0x10101 | 60 | 0 | 0 | effect cue (vf140); spell lock in action code (*not traced*) |
| 4 | Sleep | 0x1 | 30 / 15 | 1.5 | 2 | damage taken x p1, stagger and knockdown x p0 (`uCharacterBase::vf68` 0x0084D8B0); enemies also x `param\game`[0x12] (3) and stagger x [0x13] (10) (`uEnemy::vf68` 0x00AAA490) |
| 5 | Oil | 0x10104 | 90 | 2 | 2 | fire hits x p0 on both parts (0x0084D8B0) |
| 6 | Drenched | 0x104 | 90 | 0.5 | 2 | fire x p0, ice and thunder x p1 (0x0084D8B0); vf143 (0x00852B10) sets its countdown speed to 2 while the owner's +0x2697 is set (*cause not traced*) |
| 7 | Possession (`敵化`) | 0x1A01 | 180 | 0.2 | 0.35 | a player-side victim hit by a player-side attacker takes x p1 (`uPlayerBase::vf68` 0x00B7AD90); the turning itself is AI (*not traced*) |
| 8 | Skill seal | 0x101 | 90 | 0 | 0 | effect cue (vf145); skill lock in action code (*not traced*) |
| 9 | Curse | 0x3 | 90 | 0.6 | 1.4 | attack x p0 (0x0088B530, 0x0088AA20: status 9 with flag 0x200), damage taken x p1 (0x00AAA490, 0x00B7AD90) |
| 10 | Burning | 0x104 | 15 | 33 / 12 | 1 / 0.5 | HP -p0 per second, p0 x step / 30 per frame (vf147 0x00852F40 on the unit step; player 0x00B75890 on the scaled step); the owner carries a fire attack entry (+0x2750) that can set others alight (*not modelled*) |
| 11 | Frozen | 0x104 | 15 | 2 | 0.083 | motion stops (+0x1F6E cleared, vf148 0x00853060; p1 x 30 frames ramp the ice in); physical damage taken x p0 (0x0084D8B0); cured items unfreeze first (0x00B75EC0) |
| 12 | Petrify | 0x2 | 10000 (no expiry) | 10 / 40 | 180 / 80 | staged progress (vf149 0x00853260, player 0x00B75F30) driven by p0 / p1; blocks and displaces groups 1, 3, 4, 6; the end state is *not traced* |
| 13 | (group 8) | 0x8 | 10000 | 180 | 0 | blocks every other group; not reached by build-up; *meaning not traced* |
| 14 | Attack down | 0x3, stack 4 | 90 | 0.7 | 0 | physical attack x p0 per stack (product, 0x00837480 types 0x0E / 0x12) |
| 15 | Defence down | 0x3, stack 4 | 90 | 1.3 | 0 | physical damage taken x p0 per stack (0x0F / 0x13) |
| 16 | Magick attack down | 0x3, stack 4 | 90 | 0.7 | 0 | magick attack x p0 (0x10 / 0x14) |
| 17 | Magick defence down | 0x3, stack 4 | 90 | 1.3 | 0 | magick damage taken x p0 (0x11 / 0x15) |
| 18 | Attack up | 0x3, stack 4 | 60 | 1.2 | 0 | as kind 14 (same product) |
| 19 | Defence up | 0x3, stack 4 | 60 | 0.8 | 0 | as kind 15 |
| 20 | Magick attack up | 0x3, stack 4 | 60 | 1.2 | 0 | as kind 16 |
| 21 | Magick defence up | 0x3, stack 4 | 60 | 0.8 | 0 | as kind 17 |
| 22..26 | Weapon enchant fire, ice, thunder, holy, dark | 0x10106 | 60 / 1 | 10 | 0 | read by the player attack setup 0x00B6BC80 (P+0x58..+0x60) |
| 27..31 | (group 6, 0x10206) | | 60 / 1 | 50 | 5 | *not traced* |
| 32 | (group 6, 0x10306) | | 0 / 60 | 0 | 0 | *not traced* |
| 33..39 | (group 5) | | 30..300 | | | *not traced*; 34 (0x22, group 7, 90 s) is the cleanse below |

The "down" and "up" kinds are the `cStatus` types the damage rules
multiply by (COMBAT.md 3 and 4): attack `Π(14, 18)`, defence
`Π(15, 19)`, magick attack `Π(16, 20)`, magick defence `Π(17, 21)`.
The player-table timers 1 for kinds 22..31 suggest the casting skill
sets its own duration (*not traced*).

## 3. Cures

- **Time**: every kind with a timer < 10000 s wears off (section 1).
- **Broad effect messages** (`uPlayerBase::vf98` 0x00B79FC0, applied by 0x00B5C3B0 on
  the next update): effect messages 0x12 / 0x13 / 0x14 / 0x15 remove the
  kinds of masks 0xC0B (poison, slow, silence, burning, frozen), 0xF9F
  (poison, slow, blind, silence, sleep, possession, seal, curse, burning,
  frozen), 0x3C000 (the four downs) and 0x3D000 (the downs and petrify).
  These are **not the ordinary consumable cure path**. The producers of
  these four messages remain untraced; do not assign items to them.
- **Inventory consumables**: menu `0x0066CCD0` calls `0x00B77130`, which
  dispatches `vf183`; `uPlayerBase::vf183` (`0x00B77780`) applies HP and
  stamina, then calls `0x00856050`. That routine directly removes each
  status whose signed item value is in [1,2); >=2 adds the status. The
  earlier description of `0x00856050` as a debug table was incorrect.
  Its getters `0x0045A8D0..0x0045B370` read the installed ITL record:
  +1d burning, +1e frozen, +25 poison, +26 slow, +27 blind, +28 sleep,
  **+29 drenched, +2a oil**, +2b possession (pawns only), +2c silence,
  +2d skill seal, +2e curse, +2f petrify, +30 attack down, +31 defence
  down, +32 magick attack down, +33 magick defence down. The last four
  are confirmed by the EAX kind arguments at `0x00856D98..0x00856DE0`.
  Removal calls `0x00837540(kind,0)`: all matching stacks and active masks
  are cleared; damage build-up is not cleared. Poison's interval resets
  on its next status update when poison is absent.
  `ddcombat::Statuses::cure_item` handles direct removal and reports ended
  kinds. `ddeconomy::item_use` currently accepts cure-only values, rejecting
  harmful/buff/special effects rather than applying a partial item.
- **Healing water** (`FUN_008542C0`, from `uCharacterBase::vf93`): a
  character standing in water deeper than `param\game`[9] on ground with
  attribute 0x3000000 and collision flags 0x2B30 / 0x2368 bit 0 heals HP
  over time, has kinds 0..0x11 removed and gets kind 0x22, whose group 7
  blocks new debilitations for its 90 s.

## 4. Player and pawn build-up

The Arisen and pawns build up through the same `FUN_007772B0` on their
own collision, after `uPlayerBase::vf68` scaled the hit's build-up
values by their equipment (0x00B7AD90, sums from 0x00B87450):

- stagger and knockdown damage x (100 - player+0x3EE8) / 100 and
  x (100 - +0x3EE4) / 100;
- the element build-up of the hit's element x (100 - that element's
  debilitation resistance) / 100;
- each of the 15 status values (`mPoison`..`mMgcDefDown`) x
  (100 - its resistance) / 100.

No writer of a player region's thresholds was found, so they keep the
constructor values (999; 999.9 for slots 17 and 19) (*inferred*): an
unresisted 1000-point attack fires at once, a 50 % resistance needs two
hits inside the decay window. `ddenemyrt` builds the Arisen up with these
defaults and without equipment resistances (`PlayerDefender` carries
none yet).

## 5. Wall check (`FUN_00779270`)

Before damage, for victims whose collision has +0xD9C set and attackers
whose `uObjModel`+0x1E7C is set (set by `uHumanEnemy` / `uNpc` /
`uMultiNpc` vf90 0x00BAFB00 and `uEm5800`): a segment from the
attacker's position to the hit's position, both raised by 100 cm, is
tested against the scenery (`sCollision`, 0x00FAF5B0); if it is blocked
the hit deals nothing. Results are cached per (attacker, hit) for the
frame (+0xC18). Points closer than 0.1 cm are never blocked. Not modelled
in `ddenemyrt` (the goblin attacker does not set +0x1E7C).

## 6. Friendly-fire teams

The friendly-hit path of the pair callback (COMBAT.md 2.3) reads the
team matrix `0x0150A630` (131 x 131 bytes) by the two collisions' team
ids (`cObjCollision`+0xD30, `uObjModel`+0x1E10). Species set it in their
vf33: goblins 1 or 3 (`uEm0100::vf33` 0x008CDD80), 4, 0x0B..0x0E for
other families, 0x32, 0x5B, 0x82 for others; 0 by default. The matrix
is 0 on the diagonal (a team never hurts itself), 2 in columns 31 and
49..72 (the hit counts but its damage factor is 0), 1 elsewhere.

## 7. In the fight (`ddenemyrt`)

- `Fight::hits` runs `combat::status_pass` first: statuses count down,
  enemy and player poison and burning bite (main pool / Arisen HP),
  build-ups decay; then the hit pass.
- The hit pass multiplies the attacker by its downs / ups / curse, the
  victim by its downs / ups / curse and (enemies) sleep / frozen / oil /
  drenched, then builds up the hit region and adds fired kinds from
  `param\status\enemy` / `player`.
- Test `crates/ddenemyrt/tests/status.rs`: poison on a goblin bites 5 HP
  on frames 0, 31, 62, ... and wears off on frame 5399 (180 s);
  burning takes 33 HP per second and ends after 15 s.

## 8. Gaps

- Blind, silence, skill seal, possession: their action / AI consequences.
- Petrify's stages and end state; kind 13 and kinds 27..39.
- Producers of the broad cure messages; skill-cast durations of buffs and
  enchants.
- Lightning chains, holy and dark effects (`uCharacterBase::vf74`).
- Player equipment status resistances in `PlayerDefender` (claude-6 /
  claude-9 data) and the player region thresholds.
- Live checks: goblin hit, head hit, magick hit (queued with claude-4).
