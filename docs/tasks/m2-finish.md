# m2-finish: the last M2 gaps

- **Owner:** claude-1
- **Branch:** `task/m2-finish`
- **Board area:** `player/movement`
- **Status:** review

## Goal

Close the remaining M2 items after m2-fixes (`docs/MOVEMENT.md`; 40 of
44 acceptance rows pass):

1. **The character's collision shape:** the body capsule or sphere and how
   it rests and slides on edges and walls. This fixes the 4 failing drop
   rows (the wall slide that breaks free at frame 195, and the rest on a
   strip edge). It replaces the `BODY_RADIUS` / `BODY_HEIGHT` placeholders.
2. **The landing hold** with the stick held (about 0.1 s in the original).
3. **Walk/run start-frame sync** (`0x00B80260`, claude-2's finding) and
   the two-frame delay before the 1.15 motion speed applies.
4. **Load and body class from carried weight and body** (`docs/PARAMS.md`,
   0x00B81120 / 0x00B81270), feeding stamina and motion speed. The inputs
   come from the inventory; until `ddeconomy` exists, take them as
   parameters.
5. **Exhaustion:** the tired action, if the trace is short.

Also update `docs/ACCEPTANCE.md`'s results table to the current output.

## Scope

- `crates/ddsim/**`
- `crates/ddworld/**`
- `crates/ddaccept/**`
- `crates/mtlmt/**`
- `docs/MOVEMENT.md`
- `docs/ACCEPTANCE.md`

## Done when

- 44 of 44 acceptance rows pass, or the remaining ones have a traced
  reason.
- Each item is translated with its source and tests.
- Fill in Results and report.

## Results

Branch `task/m2-finish` (claude-1). Acceptance: **40 of 44** (unchanged in
count; the 4 drop path rows remain, with a traced reason). `cargo test -p
ddsim -p ddworld -p ddaccept -p mtlmt` passes (36 + 4 + 4 + 10); clippy
clean. docs/ACCEPTANCE.md's table is refreshed; docs/MOVEMENT.md has the
sources.

1. **Collision shape** (traced, not translated). The player moves through
   `cScrAdjust` at uObjModel +0x19C0 (property names from its vf03
   0x0079DB30). Its radius comes from `collision\pl\m0001` group 109
   (`mIsScrAdj`, a sphere of 35 at joint -1); read live by claude-4 (#931)
   the capsule runs 35-145 cm above the feet, radius 35, ground check
   offset -20 (adjust 0x7E), axis check type 2. 0x0079E200 sweeps it with
   the generic MtCollision collide-and-slide (0x010B4960 -> 0x010B3000,
   ~530 decompiled lines plus GJK/MPR callbacks), which I did not
   translate. `BODY_RADIUS` (35) and `BODY_HEIGHT` (now 145, from the live
   read; drop gap 119 -> 116 cm) are cited; the wall test still starts at
   60 cm because without the sweep a lower start blocks on steps (tried:
   gap 197 cm). The 4 drop rows stay: after landing the original frees
   itself from a wall slide at frame 195, ddsim does not. The strip-edge
   rest (34 cm) is the capsule's lower sphere on the edge (predicted 2 cm
   below the top, recorded 0.7).
2. **Landing hold** (done). Event group 0 bit 0x10000 lets the stick cancel
   an action (`cPlActWpnShieldGuardJumpLand::vf06` 0x00B36140 tests stick
   and +0x490 & 0x10000); landing motions 12 / 15 set it from frame 3 (the
   recorded ~3.3-frame hold), the sprint stop from 18. Applied to landings,
   the sprint-jump landing and the sprint stop (for cPlActLand
   *assumed*). Test.
3. **Start frames and the delay** (done). `Sim::step` is one engine frame;
   the unit-time factor a step ends with applies two steps later
   (`Player::unit_rate`), matching the traces; it scales the player's
   actions, motions and physics, stamina stays in game time. Start frames
   from 0x00B80260 / cPlActWalk::vf05 / cPlActRun::vf05: phase sync between
   walk/run motions, else 18 / 0 / 20 (the feet test 0x00AC5FB0 needs a
   skeleton: false branch, *placeholder*). Tests.
4. **Weight classes** (done). `WeightTables` (pleditweight,
   pltotalweight*): body class 0x00B81120, load class 0x00B81270 (with the
   ability capacity and the body's excess weight); `Sim::set_weights(id,
   body, total, capacity)` for the host until ddeconomy exists. Defaults
   stay class 1 / 1 (the recorded character). Test.
5. **Exhaustion** (not done: not short). `cPlActTired` (0x00ACB8D0 /
   0x00ACBAF0 / 0x00ACC3E0) has five phases, four motions, a staggering
   speed blended by a motion curve and a recovery debt the stamina update
   follows; its entry is not traced. ddsim flags `ran_out` only.

**Open:** translate the scenery sweep 0x010B3000 (the 4 drop rows); the
running capsule read claude-4 offered; exhaustion; the feet test.

**For head:** ddsim API additions: `Player::unit_rate`, `Sim::set_weights`,
`WeightTables`, consts `MOVE_CANCEL_*`, `LOCOMOTION_*`. Hosts must call
`Sim::step` once per engine frame (the delay counts steps). claude-10's
`ddhost::install::sim` should replace `ddaccept::new_sim` after merge (#916).
