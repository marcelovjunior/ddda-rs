# L136: bind readable ports without lifted bodies

Assigned by head to codex-11, 2026-10-13. Base: FPS integration line
`23442cfd` (includes `7aa2d159`); branch `task/L136-ports-dispatch`.
Scope: runtime Registry setup, dispatch regression, emulation-log audit.
No shard builds, game runs or changes to port implementations.

## Results

Confirmed the reported cause: `Registry::with_lifted_program` previously
registered only `lifted::all()`. That list substitutes overrides for known
lifted entries, but omits port-only addresses. The six L130 duration readers
therefore reached `Registry::call_unbound` through `x86::call_ext`.

Registry construction now overlays **all finalized `x86::overrides()`** as
`Binding::Lifted`, after loading the lifted list. This includes ports, mods,
and runtime wrappers, with their existing composition order preserved.
Existing bindings are replaced; a port also supersedes the original entry's
restartable step while retaining interior entries for SEH. Session service
registration order and unknown-target emulator fallback are unchanged.

Two regressions cover:

- The production constructor with a synthetic port-only override and a
  lifted-ABI caller issuing `call_ext`: the port runs on both initial and
  cached dispatch; guest return/stack effects match. An unknown address still
  produces the existing `Unbound` report with emulator disabled. This test
  uses the light build so no lifted body or emulator can conceal a failure.
- Overlay replacement of ordinary and step-enabled lifted entries, with
  interior SEH entries retained and no duplicate binding count.

Validation: `cargo test -p ddruntime --no-default-features --no-fail-fast`:
**192 passed, one inherited fullscreen gamma-reset failure, three ignored**.
Both new regressions pass. `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`,
`CARGO_TARGET_DIR=C:/ddda-targets/L136`; full log at
`C:/ddda-targets/L136-test-all.log`. No original-game comparison
or FPS claim. Next: head integrates and verifies that the six duration-reader
emulation messages disappear in the same route; gameplay timing needs a run.

## Run-log audit

Read-only scan of every `E:/ddda-re/notes/runs/run_*.log` available at audit
time: **12 logs, 180 messages, 15 distinct entries**. The same 15 appear once
in every log. Files: `run_c15b.log`, `run_c17cmp.log`, `run_c17draw.log`,
`run_c17nat.log`, `run_c17ver.log`, `run_c18mem.log`, `run_c19cmp.log`,
`run_c19draw.log`, `run_c19nat.log`, `run_c19ver.log`, `run_c20def.log`,
`run_c20nat.log`.

| Entry | Logged occurrences | Status at this base |
|---|---:|---|
| `0x004a0c00` | 12 | Remaining candidate |
| `0x005828e0` | 12 | Remaining candidate |
| `0x00582900` | 12 | Remaining candidate |
| `0x00582920` | 12 | Remaining candidate |
| `0x005829a0` | 12 | Remaining candidate |
| `0x005abdc0` | 12 | Remaining candidate |
| `0x00815c60` | 12 | Remaining candidate |
| `0x00c1d790` | 12 | Remaining candidate |
| `0x0112bb40` | 12 | L130 port; activation fixed here |
| `0x0112bb60` | 12 | L130 port; activation fixed here |
| `0x0112bb80` | 12 | L130 port; activation fixed here |
| `0x0112bba0` | 12 | L130 port; activation fixed here |
| `0x0112bbc0` | 12 | L130 port; activation fixed here |
| `0x0112bbe0` | 12 | L130 port; activation fixed here |
| `0x01300534` | 12 | Remaining candidate |

These are **first-seen log counts, not invocation counts**: `emu.rs::note`
prints only when a process's count for an entry becomes one. All 15 messages
precede `stage: gameplay` in every log; none first appears afterward.
Thus the logs establish boot/load hits in runs that reached gameplay, not
which entries continue running during gameplay. In `run_c20def.log`, the
six readers are lines 61-66, the other nine lines 116-132, and the gameplay
marker is line 139. Audit and qualification posted to head/codex-10 in board
messages 11879 and 11883. No raw logs or game data are included here.

Shared documentation: MILESTONES append requested from owner codex-7
(11870); RUNTIME dispatch append permitted by owner codex-9 (11888).
