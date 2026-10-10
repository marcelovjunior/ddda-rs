# S25: The bow, part 1: bow mode and the action's phases (research + sim)

- **Branch:** `step/s25-bow-mode`
- **Files:** `crates/ddsim/src/attack.rs`, `lib.rs` (a new `Weapon` slot for
  the secondary bow, the bow action state), tests; `docs/MOVEMENT.md`
- **Kind:** one feature (no arrows yet)
- **Status:** review

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

## Results (codex-57, 2026-10-09)

Implemented the stationary shortbow action through normal per-seat
`ddsim::Input::{bow,bow_toggle}`. A secondary `Weapon::Bow` slot keeps
primary daggers equipped. `Player::bow` carries independent upper/lower
motion clocks; `State::Bow` carries the runtime lower motion and phase.
Draw, ready and release use installed record fields and installed bank-9
motion end times. An early release cannot cancel draw. Missing motions
report an unsupported branch rather than using a fallback duration.

The source trace is in `docs/MOVEMENT.md`, including all five +0xC4
phases and the separate +0xB8 movement modes. Entry is `0x00B16FE0`,
record-zero selection `0x00B16DC0`, draw/ready `0x00B1C220` /
`0x00B1C7D0`, release `0x00B17940` / `0x00B1AC90`, lower-layer selection
`0x00B18CB0`; check 34 is `0x00B72DB0` case 0x22. Draw completion uses
the upper layer (+0x4C0, model motion base +0x380 plus stride 0x140),
not the lower draw or an invented timer. Release rate 1.2 is sourced.

Head **#3882** approved runtime reader plumbing in `ddhost/src/install.rs`
while keeping ddsim dependency-free. The reader follows
`rBowActParamList.mParamList -> MtArray.mpArray`, reads all 30 bow records,
selects record zero and loads bank 9 from `m0006_at`. Bulk parameters and
motion data remain in the player's install. Head **#3933** required normal
input/state and LAN integration: protocol 6 carries the input bits,
secondary slot, latch and full bow layers in whole/delta body snapshots.
Viewer constructors use default false for the new input fields.

Validation: `cargo test -p ddsim -p ddnet -p ddhost` passed. ddsim has
63 existing unit tests plus seven bow regressions; ddnet has 11
tests including bow input/full/delta/apply round trips; ddhost has 56
including installed draw-ready-release with two distinct seats. The
installed test reads actual motions, confirms slots 0/1 -> 6/7 -> 4/5,
and proves no volley or primary dagger attack is fired. No viewer build,
GPU capture or running-original comparison was performed for S25.
The final source-order correction (lower handler before upper phase
handler, so ready lower starts in the next update) and two additional
upper/lower scheduling regressions passed in a final focused rerun:
seven ddsim tests and the installed host test (4.10 s), exit 0.

Feature/source documentation commit: **bdcad08** on `step/s25-bow-mode`.

Remaining: phases 2 shot and 3 change are represented/documented but not
executed. Aim/turn, movement/air, target lock, global release-restriction
projection and full stamina recovery/status update remain explicit gaps.
Moving/shot input reports unsupported instead of substituting locomotion
or a dagger attack. Viewer key mapping, equipment selection and layered
pose presentation remain subsequent integration work. No game process,
installation, saves or main branch were modified.

Next action: S26's sourced shot scheduler and arrows can extend the typed
parameter projection and per-seat bow state; S27 can consume the normal
input/body fields for key bindings, aim and visual layer masks.
