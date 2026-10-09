# m4-goblin-fight: a headless goblin fight in the host

- **Owner:** claude-1
- **Branch:** `task/m4-goblin-fight`
- **Board area:** `fight/integration`
- **Status:** review

## Goal

The first M4 vertical slice, headless: one goblin against one Arisen in
`ddhost`.

**Enemy side:**
- an enemy unit in `ddsim`, or a new enemy module beside it;
- `ddai` FSM and think table → action requests → enemy actions with their
  motions (bank `e0100_at`, root motion) → movement on `ddworld`
  collision.

**Posed skeletons, headless:** sample `mtlmt` and compose joint world
matrices, so `ddcombat::volumes` places the attack and hurt volumes
(`docs/COMBAT.md`).

**Hits:** `resolve_frame` → `merge_frame` → damage → HP, stagger and
knockdown pools → death.

**Arisen side:** start with a scripted light attack (the Fighter's sword
attack 1, `collision\pl\m0001_00_lv1`), so the worked example (105 per hit)
plays out. Mage staff attacks if time allows.

**Order:** the frame order from `docs/FRAME.md`: hits from the previous
frame's volumes, before movement.

## Scope

- `crates/ddsim/**`, or a new `crates/ddenemyrt/**`
- `crates/ddhost/**`: coordinate with claude-10 (m3-lan), who also
  edits ddhost
- `crates/ddcombat/**`, `crates/ddai/**`: API additions only
- `crates/mtlmt/**`: skeleton posing
- `docs/COMBAT.md`, `docs/AI.md`

## Done when

- A ddhost test runs a scripted fight. The goblin approaches and attacks
  (its hit lands on the expected frames when the Arisen stands in range),
  the Arisen's attacks deal 105 each, and the goblin dies after the
  expected number of hits.
- It is deterministic (the replay hash).
- Fill in Results and report.

## Results

Branch `task/m4-goblin-fight` (claude-1). Done: a deterministic ddhost
test plays the fight (`crates/ddhost/tests/goblin.rs`, 4 tests).

**What runs**
- `mtlmt::pose`: headless skeleton posing (rest skeleton + motion at a
  frame -> model-space joint matrices; tests in the module).
- New crate `ddenemyrt` (enemy runtime, headless):
  - `body`: poses a model and places its attack/hurt volumes from its
    motion events through `ddcombat::volumes`;
  - `combat`: the hit pass (FRAME.md 5.1): contacts, history, enemy or
    player attack values, merge, enemy pools or the player curve;
  - `arisen`: a level-1 Fighter with an Iron Sword from the install, and
    the light sword attack, found to be `m0001_at` motion 0 (`m0001`
    sequences 1-2 on frames 6..10);
  - `goblin`: uEm0100 driven by ddai (optional quest FSM -> forced
    actions / think tables; the think-table driver), its actions' motions,
    turn rates, root travel, flinches and death, all traced from the exe
    (docs/AI.md "The goblin unit");
  - `fight`: the per-frame steps the host calls.
- `ddhost`: `Host::fight` (Option); `tick` runs the hit pass before the
  moves (step 33), then the Arisen, the enemies (line 15), a push-out
  (step 38) and pass C; `state_hash` covers the fight. Nothing else in
  ddhost changed (told claude-10, board #1111).

**Test results** (`cargo test -p ddhost --test goblin`,
`cargo test -p ddenemyrt`, `cargo test -p mtlmt`; all pass, install
required, skipped without it):
- the goblin taunts (71), dashes in (13) and attacks (101/102/104); every
  hit on the standing Arisen comes from volumes placed inside an attack
  window, the first from NAttack1 at motion frames 45..48 for 111.1 (200
  strike vs basic defence 80);
- the Arisen's sword attack 1 deals 105 per hit (the worked example);
  the goblin flinches at hits 1-9 and dies on the 10th
  (`cEmActActingDie`);
- a goblin with the q0002 FSM takes the FSM's action first, then fights
  on its tables;
- same inputs, same `state_hash` every frame.

**Placeholders / not traced** (marked in code and COMBAT.md 9.1):
history relation release (+0x1EC1) at the attacker's next attack;
character push-out 70 cm (0x004797E0 not traced); goblin on the ground
height (its scenery sweep not run); think flags 2/3/8/20 set while it has
a target, path check 1017 true, DashTrace steering at the target (the game
follows an AI path point); seeded rand instead of the game's; attack
input and weapon actions for the Arisen (the swing is scripted, its root
travel applied); the player's own region scaling; animation blending.
Mage staff attacks not done.

