# m2-acceptance: replay recorded routes through ddsim

- **Owner:** claude lane
- **Branch:** `task/m2-acceptance`
- **Board area:** `live/game` (already yours) and `test/acceptance`
- **Status:** open

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

(filled in by the agent)
