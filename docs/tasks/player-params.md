# player-params: stamina, weight, level and vocation parameters

- **Owner:** codex lane
- **Branch:** `task/player-params`
- **Board area:** `player/params` (add `player/stamina` if you trace rules)
- **Status:** review

## Goal

Typed, tested readers for the Arisen's parameter files, plus the rules
that use them: how stamina drains and recovers, how weight class is chosen
and what it changes. These feed the stamina, weight, customization and
vocation systems in `docs/ROADMAP.md`.

## Evidence and starting points

All under `nativePC`, listed with `vfs.paths_of_type(mtarc::type_hash(..))`:

| Path pattern | Type | Count |
|---|---|---|
| `param\pl\stamina\plstamina*` | `rPlStamina` | 31: per weapon (`_sword`, `_wand`, ...) and per action (`dash`, `dashjump`, `run`, `walk`, `liftl`, `enemyclimb`, `recovernormal`, `max`, ...) |
| `param\pl\weight\pltotalweight{ss,s,m,l,ll}`, `pleditweight` | `rPlWeight` | 6. `pltotalweightm` is `mWeight` = 0, 85, 95, 115, 125, 135 |
| `param\pl\level\lv<vocation>`, `exp` | `rPlLevel`, `rPlExp` | 10 |
| `param\pl\joblevel\joblv<vocation>` | `rPlJobLevel` | 9 |
| `param\pl\other\plabilityparam`, `pldefendparam`, `plneckpos` | XFS | 3 |

- Most of these are XFS. Dump any of them with
  `cargo run --release -p ddtool -- xfs-get <nativePC> 'param\pl\weight\pltotalweightm'`.
- For typed access through `mtxfs`, copy the pattern of `jump_params` in
  `crates/ddview/src/play.rs`.
- **Already known:** the walk/run speed factor uPlayer +0xDA8 was recorded
  as 0.8471090. No instruction in the exe stores to `[reg+0xDA8]` directly;
  its writer is unknown. It may come from body size or weight. Find it.
- **Exe starting points:**
  - The `rPlStamina` class, and the callers of its getters
    (`dti-labels.csv`).
  - Xrefs to the `plstamina*` path strings.
  - `cPlActDash::vf06` (0x00ACAF80) calls player vfunc +0x210 with 3 and
    0xB7, which may be stamina-consumption ids.
  - `pltotalweight*` path strings and their loader.

## Scope (files you may create or edit)

- `crates/ddparam/**` (new crate)
- `docs/PARAMS.md` (new)
- `crates/ddtool/src/main.rs`: you may add one `param-dump` subcommand

## Deliverables

1. `ddparam`: a typed struct and loader per type, from bytes. Tests that
   load **every** file of each type from the install, skipped when the
   install path does not exist. Plus one value-checked test per type
   (e.g. `pltotalweightm` is 0, 85, 95, 115, 125, 135).
2. `docs/PARAMS.md`:
   - every field with its meaning, marked *traced* (exe address) or
     *guessed*;
   - tables of the shipped values;
   - the stamina rules (drain and recovery per action, units, per-frame or
     per-second, which file each action uses), each with its exe address;
   - weight class selection and its effects;
   - the writer of +0xDA8 if found.

## Done when

`cargo test -p ddparam` passes, every file of every listed type parses
with 0 failures, and PARAMS.md says which rules are traced and which are
not. Fill in Results below, then report to `head` on the board.

## Results

Implemented `ddparam`: strict typed loaders for rPlStamina, rPlWeight,
rPlLevel, rPlExp, rPlJobLevel, rPlAbilityParam, rPlDefendParam and rPlNeckPos.
All fields, order, resource quality and four Vector3 storage lanes are
preserved. Added `ddtool param-dump` in its own module and minimal hooks.

Verified on 2026-10-09: `cargo test -p ddparam -- --nocapture` passes
12 tests (3 synthetic schema/malformed tests, 8 value-checked type tests,
1 exhaustive install test). All **61 paths / 1,021 archive entries** parse
with zero failures: stamina 33/987, weight 6/6, level 9/15, exp 1/1,
job level 9/9, ability 1/1, defend 1/1, neck 1/1. The brief's stamina
count 31 is corrected to 33. `cargo check -p ddtool` passes; CLI smoke
test prints PlTotalWeightM = 0,85,95,115,125,135. Install tests skip with
a diagnostic when nativePC is absent (DDDA_NATIVE_PC override supported).

`docs/PARAMS.md` contains every field, normalized shipped-value tables,
and traced versus unresolved rules. Key evidence:
- 0x0047e2b0 / 0x00b5bc40 load/map resources to per-player pointers.
- 0x00b81120 selects body class; 0x00b81270 selects encumbrance using
  body-relative thresholds plus an ability adjustment.
- 0x00b81810 computes recovery from body and encumbrance rows.
- 0x00b81920 computes continuous drain and motion-speed modifiers.
- 0x00b81450 scales rates and applies recovery, then drain.
- 0x00b81e30 clamps stamina and manages exhaustion flags.
- +0xDA8 direct MOVSS writers exist at 0x00b6b3d6/0x00b6b477/
  0x00b6b4d1, through player+0x2e54 -> object+4 -> owner.
  0x0081c170 derives horizontal scale from two 0xd0-stride records'
  +0x84 values times owner+0x64. The recorded 0.8471090 is not yet
  reproduced and the exact customization records remain unidentified.

Unknown/unverified: scaled-update units to wall-clock seconds; full
status/augment effects; one-shot jump/weapon consumers and skill IDs;
RecoverLift consumers; level/experience growth formulas; archive override
precedence; exact owner/record identities in the +0xDA8 chain. No gameplay
simulation translated, no comparison with original performed. No game
data, raw exports or private research files committed.

Next action: capture original stamina plus the three timing-factor fields
during standing/sprinting/climbing, then finish one-shot/exhaustion consumers.
Head should fold the +0xDA8 correction and parse evidence into shared docs
after review; this branch does not edit them.
