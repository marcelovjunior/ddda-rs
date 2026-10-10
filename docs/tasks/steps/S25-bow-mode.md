# S25: The bow, part 1: bow mode and the action's phases (research + sim)

- **Branch:** `step/s25-bow-mode`
- **Files:** `crates/ddsim/src/attack.rs`, `lib.rs` (a new `Weapon` slot for
  the secondary bow, the bow action state), tests; `docs/MOVEMENT.md`
- **Kind:** one feature (no arrows yet)

## Known (head, docs/MOVEMENT.md "Record words" and "The bow")

- Secondary-weapon list 0x014F8198: `cPlActWpnBow` (action 0x02060000) on
  **command 7 held** (bit 0x8 of uPlayer +0x32E4), check 34 (0x22), slot 1,
  ground 0x301 and air 0x302; 0x014F81F8 is 0x02060002. The longbow is the
  same class with word 0 = 1, the magick bow `cPlActWpnMagicBow`.
- Bit 0x8 comes from `0x00B52A00`: the bow-mode toggle (`0x018D8BB0` +0x1085)
  while the secondary weapon is a bow, longbow or magick bow (categories
  10-12); see its PC key bindings (`STG_TOGGLE_BOW_MODE`, `STG_SUB_WEP` in
  config.ini).
- `cPlActWpnBow` vtable 0x015E2F08: vf05 **0x00B16FE0** (sets +0x54 = 0x14A,
  reads three parameter records through 0x00B16DC0 / 0x00B16EE0 /
  0x00B16F60 into +0x90/+0xA0/+0xA4, creates a `uAimCheck` unit, plays
  motions from the record's bytes +0xE8/+0xE9/+0xF1 in bank 0x900),
  vf06 **0x00B17940** (state at +0xC4 = param_1[0x31], 0..4; release of bit
  0x8 ends it; calls 0x00B1D010, 0x00B17C20, 0x00B17DA0, 0x00B19540,
  0x00B1AC90, 0x00B1E480 every frame).

## Do

1. Trace check 34, the bow-mode toggle and its key, and the five states of
   vf06 (what each plays, its exits, the turn/aim handling), and write them
   into MOVEMENT.md under "The bow".
2. Model in ddsim: the Strider's secondary bow (shortbow 0x1F8), the bow
   toggle input, and the bow action's states with their motions (bank 9 =
   the bow's `m0006_at`), without firing yet.
3. Tests for entering and leaving the bow action.

## Done when

Holding the bow input with a Strider enters the bow action and its draw
states play their motions in a ddsim test, and MOVEMENT.md documents the
states with addresses.
