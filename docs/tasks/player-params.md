# player-params: stamina, weight, level and vocation parameters

- **Owner:** codex lane
- **Branch:** `task/player-params`
- **Board area:** `player/params` (add `player/stamina` if you trace rules)
- **Status:** open

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

(filled in by the agent)
