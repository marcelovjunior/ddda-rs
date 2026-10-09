# m2-acceptance: replay recorded routes through ddsim

- **Owner:** claude lane
- **Branch:** `task/m2-acceptance`
- **Board area:** `live/game` (already yours) and `test/acceptance`
- **Status:** review (claude-4, 2026-10-09)

## Goal

The M2 gate in `docs/MILESTONES.md`, as an automated test: "recorded
paths, speeds, jump arcs and stamina match within tolerance". This is
nv-rs's acceptance-route idea (`scripts/acceptance.ps1` there).

1. **Record** a few short scripted routes in the original through the
   oracle, each with a frame-indexed key script and a per-frame trace
   (position, velocity, action id, motion frame, stamina, `mDeltaTime`):
   - run straight for 3 s;
   - sprint 4 s, then stop;
   - a standing jump;
   - a running jump;
   - walk off a 2-3 m drop (find one with `ddda-ledges`).

   Use the same start pose and the town or field mode the player is in.
2. **Store** each trace privately (`E:\ddda-re\notes\acceptance\`, never
   committed). Commit a compact derived summary only if it holds no game
   data: our own numeric tolerances and the script.
3. **Replay:** a test in `ddsim` (or a small `ddaccept` crate) that loads
   the same stage collision around the start pose (`ddworld`), feeds the
   same input per frame with the recorded `mDeltaTime` steps, and
   compares:
   - path,
   - speed,
   - jump apex and airtime,
   - landing time,
   - stamina curve.

   The test skips when the private traces are absent.
4. **Report** the measured differences per route in
   `docs/ACCEPTANCE.md`. Turn real mismatches into board messages to
   head, with numbers.

## Scope

- `crates/ddaccept/**` (new) or `crates/ddsim/tests/**`
- `docs/ACCEPTANCE.md`
- `research/ddda-oracle/**` (record scripts)

Do not change ddsim rules yourself. Report mismatches, and head assigns
the fixes.

## Done when

`cargo test -p ddaccept` runs all the routes against the private
traces. ACCEPTANCE.md lists every quantity with original vs ours and the
tolerance, and the mismatches are reported. Fill in Results and report.

## Results

**Built:**
- `crates/ddaccept`:
  - trace format;
  - Sim and world setup from the install, as the viewer's `--play`;
  - replay with the recorded keys and sMain deltas;
  - per-route comparison against our tolerances.
- `research/ddda-oracle/routes.py`: the recorder.
- `ddda-ledges` modes `LEDGES_LINES`, `LEDGES_AT` and `LEDGES_FLAT`.
- `docs/ACCEPTANCE.md`.

The tests:
- `cargo test -p ddaccept` runs all 5 routes against the private traces
  in `E:\ddda-re\notes\acceptance\` and prints the table. It skips when
  the traces are absent.
- `-- --ignored` is the strict gate. It fails today.

**Recorded** in stage 100, cell st100_58m49n: run 3 s, sprint 4 s then
stop, standing jump, running jump, and a 2.9 m drop.

**Matches** (16 of 44 quantities): standing and running jumps.
- apex 93.4 cm original vs 92.6 cm ours;
- airtime 0.638 / 0.704 s vs 0.639 / 0.714 s;
- takeoff, landing time and jump distance all within tolerance.

**Mismatches**, reported to head with numbers:
1. Ground speed is 15% low: run 302 vs 342 cm/s, sprint 671 vs 747. The
   original's player delta (+0x14) is 1.15 x sMain's while running or
   sprinting, and the step is |v| x that delta.
2. No sprint stop: the original slides 198 cm in 0.98 s (action 3), ours 0.
3. Stamina is not modelled in ddsim. The original spends -45.000/s
   sprinting, holds 0.618 s, then recovers +48.750/s.
4. No slope limit: ddsim walks down the steep side that the original
   falls from (287 cm in 0.623 s).
5. Collision at the drop is 34 cm lower in ddworld than the surface the
   original stood on (placed scenery).

ddsim rules were not changed.

**Unknown or next:**
- ledge routes (#418 item 1 still needs a clean ledge);
- turning, walking and field-run routes;
- the town/field area attribute; the replay infers it from the run speed.
